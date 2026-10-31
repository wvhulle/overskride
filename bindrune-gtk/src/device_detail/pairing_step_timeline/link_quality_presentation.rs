use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use bindrune_bluez::{
    link_quality::{
        disconnect_reason_history::ConnectionHistory,
        hidraw_report_timing::{NOTICEABLE_GAP, ReportTiming, ReportTimingSample},
    },
    remote_device::remote_device_snapshot::{DeviceSnapshot, DistanceEstimate, SignalStrength},
};
use uom::{
    fmt::DisplayStyle,
    si::{
        f32::{Ratio, Time},
        frequency::hertz,
        length::meter,
        ratio::percent,
        time::{millisecond, second},
    },
};

use crate::{
    device_detail::pairing_step_timeline::pairing_timeline_view_model::{
        PropertyHint, PropertyRow, StepButtonAction, StepButtonSlot, StepNodeState, StepParts,
        TimelineInputs, hinted_property, inline_property,
    },
    shared_device_appearance::SignalQuality,
};

const DROP_COUNTING_WINDOW: Duration = Duration::from_secs(3600);
const UNSTABLE_SIGNAL_LOSS_COUNT: usize = 2;

const UNSTABLE_WORST_GAP: Duration = Duration::from_millis(100);

fn unstable_lagging_share() -> Ratio {
    Ratio::new::<percent>(2.0)
}

#[derive(Clone, Debug, PartialEq)]
pub enum ReportTimingState {
    NotApplicable,
    Waiting(PathBuf),
    Sampled {
        hidraw_node: PathBuf,
        sample: ReportTimingSample,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinkMeasurements {
    pub connection_history: ConnectionHistory,
    pub report_timing: ReportTimingState,
    pub measured_at: Instant,
}

impl LinkMeasurements {
    pub fn unmeasured(connection_history: ConnectionHistory) -> Self {
        Self {
            connection_history,
            report_timing: ReportTimingState::NotApplicable,
            measured_at: Instant::now(),
        }
    }
}

impl TimelineInputs<'_> {
    pub(super) fn signal_step(&self, default_state: StepNodeState) -> StepParts {
        let snapshot = self.snapshot;
        let (status_text, property_rows) = match snapshot.signal_strength {
            SignalStrength::Measured(rssi) => {
                let signal_quality = SignalQuality::from_rssi(rssi);
                (
                    format!("{} signal, in range", signal_quality.quality_label()),
                    vec![
                        PropertyRow::InlineWithIcon {
                            key: "Signal strength".to_string(),
                            value: format!("{rssi} dBm"),
                            hint: PropertyHint::Shown("Measured while scanning".to_string()),
                            icon_name: signal_quality.quality_icon_name().to_string(),
                        },
                        distance_row(snapshot.distance_estimate()),
                    ],
                )
            }
            SignalStrength::Unknown if snapshot.connected => {
                ("Connected, strength not reported".to_string(), Vec::new())
            }
            SignalStrength::Unknown => (
                "Not measured: press Scan to check whether it is nearby".to_string(),
                Vec::new(),
            ),
        };
        StepParts {
            node_state: if default_state == StepNodeState::Next && self.is_busy() {
                StepNodeState::Pending
            } else {
                default_state
            },
            status_text,
            button: StepButtonSlot::Hidden,
            property_rows,
        }
    }

    pub(super) fn connection_step(&self, default_state: StepNodeState) -> StepParts {
        let snapshot = self.snapshot;
        let waiting_for_device = !snapshot.connected && snapshot.bonded && snapshot.trusted;
        let ready_to_connect = waiting_for_device && in_range(snapshot);
        let unstable_link = snapshot.connected && self.link_is_unstable();
        let status_text = if unstable_link {
            "Connected, unstable link"
        } else if snapshot.connected {
            "Connected"
        } else if ready_to_connect {
            "In range: press Connect if it doesn't connect by itself"
        } else if waiting_for_device {
            "Waiting: turn the device on or bring it closer"
        } else {
            "Not connected"
        };
        let button = if snapshot.connected {
            self.button(
                StepButtonAction::Disconnect,
                "Disconnect",
                snapshot.paired,
                false,
            )
        } else {
            self.button(
                StepButtonAction::Connect,
                "Connect",
                snapshot.paired,
                ready_to_connect,
            )
        };
        StepParts {
            node_state: if unstable_link {
                StepNodeState::Warning
            } else if ready_to_connect {
                StepNodeState::Next
            } else {
                default_state
            },
            status_text: status_text.to_string(),
            button,
            property_rows: self
                .link_rows()
                .into_iter()
                .chain(self.battery_rows())
                .collect(),
        }
    }

    fn measured_report_timing(&self) -> Option<&ReportTiming> {
        match &self.link_measurements.report_timing {
            ReportTimingState::Sampled {
                sample: ReportTimingSample::Measured(report_timing),
                ..
            } => Some(report_timing),
            ReportTimingState::NotApplicable
            | ReportTimingState::Waiting(_)
            | ReportTimingState::Sampled { .. } => None,
        }
    }

    fn link_is_unstable(&self) -> bool {
        let measurements = self.link_measurements;
        let frequent_signal_losses = measurements
            .connection_history
            .signal_losses_within(measurements.measured_at, DROP_COUNTING_WINDOW)
            >= UNSTABLE_SIGNAL_LOSS_COUNT;
        let noticeable_lag = self.measured_report_timing().is_some_and(|report_timing| {
            report_timing.streaming
                && (report_timing.longest_gap >= UNSTABLE_WORST_GAP
                    || report_timing.lagging_share >= unstable_lagging_share())
        });
        frequent_signal_losses || noticeable_lag
    }

    fn link_rows(&self) -> Vec<PropertyRow> {
        let measurements = self.link_measurements;
        let history = &measurements.connection_history;
        let measured_at = measurements.measured_at;
        let connected_row = history
            .connected_for(measured_at)
            .filter(|_connected_time| self.snapshot.connected)
            .map(|connected_time| {
                inline_property("Connected for", describe_whole_seconds(connected_time))
            });
        let recent_drops: Vec<_> = history
            .drops_within(measured_at, DROP_COUNTING_WINDOW)
            .collect();
        let drops_row = (self.snapshot.connected || !recent_drops.is_empty()).then(|| {
            let signal_losses = history.signal_losses_within(measured_at, DROP_COUNTING_WINDOW);
            let drop_summary = match (recent_drops.len(), signal_losses) {
                (0, _) => "None".to_string(),
                (drop_count, 0) => drop_count.to_string(),
                (drop_count, signal_losses) => {
                    format!("{drop_count} ({signal_losses} signal lost)")
                }
            };
            match recent_drops.last() {
                Some(last_drop) => hinted_property(
                    "Drops in the last hour",
                    drop_summary,
                    &format!(
                        "Last: {}, {} ago",
                        last_drop.reason.reason_label(),
                        describe_whole_seconds(measured_at.duration_since(last_drop.observed_at))
                    ),
                ),
                None => inline_property("Drops in the last hour", drop_summary),
            }
        });
        let report_rows = if self.snapshot.connected {
            describe_report_timing(&measurements.report_timing)
        } else {
            Vec::new()
        };
        connected_row
            .into_iter()
            .chain(drops_row)
            .chain(report_rows)
            .collect()
    }
}

fn describe_whole_seconds(elapsed: Duration) -> String {
    humantime::format_duration(Duration::from_secs(elapsed.as_secs())).to_string()
}

fn describe_report_rate(report_timing: &ReportTiming) -> String {
    format!(
        "{:.0}",
        report_timing
            .report_rate
            .into_format_args(hertz, DisplayStyle::Abbreviation)
    )
}

fn describe_report_timing(report_timing: &ReportTimingState) -> Vec<PropertyRow> {
    match report_timing {
        ReportTimingState::NotApplicable => Vec::new(),
        ReportTimingState::Waiting(_hidraw_node) => {
            vec![inline_property("Input reports", "Measuring…")]
        }
        ReportTimingState::Sampled {
            sample: ReportTimingSample::Measured(measured_timing),
            ..
        } if measured_timing.streaming => vec![
            hinted_property(
                "Report rate",
                describe_report_rate(measured_timing),
                "Input reports per second, over the last 10 seconds",
            ),
            hinted_property(
                "Worst gap",
                format!(
                    "{:.0}",
                    Time::new::<second>(measured_timing.longest_gap.as_secs_f32())
                        .into_format_args(millisecond, DisplayStyle::Abbreviation)
                ),
                "Longest time without a report, felt as lag",
            ),
            hinted_property(
                "Time lagging",
                format!(
                    "{:.1}",
                    measured_timing
                        .lagging_share
                        .into_format_args(percent, DisplayStyle::Abbreviation)
                ),
                &format!(
                    "Time spent in gaps longer than {:.0}",
                    Time::new::<second>(NOTICEABLE_GAP.as_secs_f32())
                        .into_format_args(millisecond, DisplayStyle::Abbreviation)
                ),
            ),
        ],
        ReportTimingState::Sampled {
            sample: ReportTimingSample::Measured(measured_timing),
            ..
        } => vec![hinted_property(
            "Report rate",
            describe_report_rate(measured_timing),
            "Only sent while the device is in use, so gaps are not measured",
        )],
        ReportTimingState::Sampled {
            hidraw_node,
            sample: ReportTimingSample::Unreadable(read_error),
        } => vec![hinted_property(
            "Input reports",
            format!("No access to {}", hidraw_node.display()),
            &std::io::Error::from(*read_error).to_string(),
        )],
    }
}

pub(super) fn in_range(snapshot: &DeviceSnapshot) -> bool {
    matches!(snapshot.signal_strength, SignalStrength::Measured(_))
}

fn distance_row(distance_estimate: DistanceEstimate) -> PropertyRow {
    match distance_estimate {
        DistanceEstimate::Estimated(distance) => hinted_property(
            "Distance",
            format!(
                "≈ {:.1}",
                distance.into_format_args(meter, DisplayStyle::Abbreviation)
            ),
            "Rough estimate from signal strength",
        ),
        DistanceEstimate::Unknown => inline_property("Distance", "Unknown"),
    }
}

#[cfg(test)]
mod tests {
    use uom::si::length::meter;

    use super::*;

    #[test]
    fn distance_is_rendered_by_uom_in_meters() {
        let distance = uom::si::f32::Length::new::<meter>(2.04);
        assert_eq!(
            distance_row(DistanceEstimate::Estimated(distance)),
            hinted_property("Distance", "≈ 2.0 m", "Rough estimate from signal strength")
        );
    }
}
