use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::Stream;
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use uom::si::f32::{Frequency, Ratio, Time};
use uom::si::frequency::hertz;
use uom::si::ratio::ratio;
use uom::si::time::second;

const SUMMARY_INTERVAL: Duration = Duration::from_secs(1);
const MEASUREMENT_WINDOW: Duration = Duration::from_secs(10);
pub const NOTICEABLE_GAP: Duration = Duration::from_millis(20);
const LARGEST_REPORT_SIZE: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReportTiming {
    pub report_rate: Frequency,
    pub longest_gap: Duration,
    pub lagging_share: Ratio,
    pub streaming: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReportTimingSample {
    Unreadable(std::io::ErrorKind),
    Measured(ReportTiming),
}

fn streaming_threshold() -> Frequency {
    Frequency::new::<hertz>(50.0)
}

pub fn summarize_arrivals(
    arrivals: &[Instant],
    measured_until: Instant,
    window: Duration,
) -> ReportTiming {
    let intervals: Vec<Duration> = arrivals
        .windows(2)
        .map(|arrival_pair| arrival_pair[1].duration_since(arrival_pair[0]))
        .collect();
    let trailing_gap = arrivals.last().map_or(window, |last_arrival| {
        measured_until.duration_since(*last_arrival)
    });
    let longest_gap = intervals
        .iter()
        .copied()
        .chain(std::iter::once(trailing_gap))
        .max()
        .unwrap_or(window);
    let lagging_time: Duration = intervals
        .iter()
        .copied()
        .chain(std::iter::once(trailing_gap))
        .filter(|gap| *gap > NOTICEABLE_GAP)
        .sum();
    let lagging_share: Ratio = (Time::new::<second>(lagging_time.as_secs_f32())
        / Time::new::<second>(window.as_secs_f32()))
    .min(Ratio::new::<ratio>(1.0));
    let report_rate =
        Ratio::new::<ratio>(arrivals.len() as f32) / Time::new::<second>(window.as_secs_f32());
    ReportTiming {
        report_rate,
        longest_gap,
        lagging_share,
        streaming: report_rate >= streaming_threshold(),
    }
}

fn open_nonblocking(hidraw_node: &Path) -> std::io::Result<AsyncFd<File>> {
    let hidraw_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(hidraw_node)?;
    Ok(unsafe { AsyncFd::register_with_interest(hidraw_file, Interest::READABLE) }?)
}

fn forget_old_arrivals(arrivals: &mut VecDeque<Instant>, now: Instant) {
    while arrivals
        .front()
        .is_some_and(|oldest_arrival| now.duration_since(*oldest_arrival) > MEASUREMENT_WINDOW)
    {
        arrivals.pop_front();
    }
}

pub fn report_timing(
    hidraw_node: PathBuf,
) -> impl Stream<Item = ReportTimingSample> + Send + 'static {
    async_stream::stream! {
        let hidraw_file = match open_nonblocking(&hidraw_node) {
            Ok(hidraw_file) => hidraw_file,
            Err(open_error) => {
                yield ReportTimingSample::Unreadable(open_error.kind());
                return;
            }
        };
        let measuring_since = Instant::now();
        let mut arrivals = VecDeque::new();
        let mut report_buffer = [0_u8; LARGEST_REPORT_SIZE];
        let mut summary_ticks = tokio::time::interval(SUMMARY_INTERVAL);
        summary_ticks.tick().await;
        loop {
            tokio::select! {
                readiness = hidraw_file.readable() => {
                    let mut ready_guard = match readiness {
                        Ok(ready_guard) => ready_guard,
                        Err(readiness_error) => {
                            yield ReportTimingSample::Unreadable(readiness_error.kind());
                            return;
                        }
                    };
                    match ready_guard.try_io(|ready_file| ready_file.get_ref().read(&mut report_buffer)) {
                        Ok(Ok(0)) => return,
                        Ok(Ok(_report_length)) => arrivals.push_back(Instant::now()),
                        Ok(Err(read_error)) => {
                            yield ReportTimingSample::Unreadable(read_error.kind());
                            return;
                        }
                        Err(_would_block) => {}
                    }
                }
                _summary_tick = summary_ticks.tick() => {
                    let now = Instant::now();
                    forget_old_arrivals(&mut arrivals, now);
                    let window = now.duration_since(measuring_since).min(MEASUREMENT_WINDOW);
                    yield ReportTimingSample::Measured(summarize_arrivals(arrivals.make_contiguous(), now, window));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uom::si::ratio::percent;

    fn steady_arrivals(start: Instant, interval: Duration, count: u32) -> Vec<Instant> {
        (0..count)
            .map(|arrival_number| start + interval * arrival_number)
            .collect()
    }

    #[test]
    fn steady_stream_is_never_lagging() {
        let start = Instant::now();
        let arrivals = steady_arrivals(start, Duration::from_millis(4), 2500);
        let measured_until = *arrivals.last().expect("arrivals exist") + Duration::from_millis(4);
        let timing = summarize_arrivals(&arrivals, measured_until, Duration::from_secs(10));
        assert!((timing.report_rate.get::<hertz>() - 250.0).abs() < 1.0);
        assert_eq!(timing.longest_gap, Duration::from_millis(4));
        assert!(timing.lagging_share.get::<percent>() < f32::EPSILON);
        assert!(timing.streaming);
    }

    #[test]
    fn stall_counts_as_time_lagging() {
        let start = Instant::now();
        let mut arrivals = steady_arrivals(start, Duration::from_millis(4), 100);
        let resumed_at = *arrivals.last().expect("arrivals exist") + Duration::from_millis(150);
        arrivals.extend(steady_arrivals(resumed_at, Duration::from_millis(4), 100));
        let measured_until = *arrivals.last().expect("arrivals exist");
        let timing = summarize_arrivals(&arrivals, measured_until, Duration::from_secs(1));
        assert_eq!(timing.longest_gap, Duration::from_millis(150));
        assert!((timing.lagging_share.get::<percent>() - 15.0).abs() < 0.5);
    }

    #[test]
    fn bunched_reports_below_the_noticeable_gap_are_not_lagging() {
        let start = Instant::now();
        let arrivals: Vec<Instant> = (0..500_u32)
            .flat_map(|bunch_number| {
                let bunch_start = start + Duration::from_millis(10) * bunch_number;
                [bunch_start, bunch_start + Duration::from_micros(500)]
            })
            .collect();
        let measured_until = *arrivals.last().expect("arrivals exist");
        let timing = summarize_arrivals(&arrivals, measured_until, Duration::from_secs(5));
        assert!(timing.lagging_share.get::<percent>() < f32::EPSILON);
        assert!(timing.streaming);
    }

    #[test]
    fn idle_device_is_not_streaming() {
        let start = Instant::now();
        let arrivals = steady_arrivals(start, Duration::from_millis(500), 20);
        let measured_until = *arrivals.last().expect("arrivals exist");
        let timing = summarize_arrivals(&arrivals, measured_until, Duration::from_secs(10));
        assert!(!timing.streaming);
    }

    #[test]
    fn current_silence_counts_as_a_gap() {
        let start = Instant::now();
        let arrivals = steady_arrivals(start, Duration::from_millis(4), 100);
        let measured_until = *arrivals.last().expect("arrivals exist") + Duration::from_millis(300);
        let timing = summarize_arrivals(&arrivals, measured_until, Duration::from_secs(1));
        assert_eq!(timing.longest_gap, Duration::from_millis(300));
    }
}
