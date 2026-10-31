use std::{
    collections::VecDeque,
    str::FromStr,
    time::{Duration, Instant},
};

use dbus::message::MatchRule;
use futures::{Stream, StreamExt, future};

use crate::bluez_operation_error::BluetoothError;

const DEVICE_INTERFACE: &str = "org.bluez.Device1";
const DISCONNECTED_SIGNAL: &str = "Disconnected";
const DEVICE_PATH_PREFIX: &str = "/org/bluez/";
const DEVICE_SEGMENT_PREFIX: &str = "dev_";
const REMEMBERED_DROP_WINDOW: Duration = Duration::from_secs(3600);
const MOST_REMEMBERED_DROPS: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisconnectReason {
    SignalLost,
    ClosedHere,
    ClosedByDevice,
    AuthenticationFailed,
    Suspended,
    Unknown,
}

impl DisconnectReason {
    pub fn from_bluez_reason(reason_name: &str) -> Self {
        match reason_name {
            "org.bluez.Reason.Timeout" => Self::SignalLost,
            "org.bluez.Reason.Local" => Self::ClosedHere,
            "org.bluez.Reason.Remote" => Self::ClosedByDevice,
            "org.bluez.Reason.Authentication" => Self::AuthenticationFailed,
            "org.bluez.Reason.Suspend" => Self::Suspended,
            _ => Self::Unknown,
        }
    }

    pub fn reason_label(self) -> &'static str {
        match self {
            Self::SignalLost => "signal lost",
            Self::ClosedHere => "disconnected here",
            Self::ClosedByDevice => "disconnected by the device",
            Self::AuthenticationFailed => "authentication failed",
            Self::Suspended => "computer suspended",
            Self::Unknown => "unknown reason",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionDrop {
    pub adapter_name: String,
    pub address: bluer::Address,
    pub reason: DisconnectReason,
    pub observed_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionSpan {
    Disconnected,
    ConnectedSince(Instant),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionHistory {
    pub connection_span: ConnectionSpan,
    pub recent_drops: VecDeque<ConnectionDrop>,
}

impl Default for ConnectionHistory {
    fn default() -> Self {
        Self {
            connection_span: ConnectionSpan::Disconnected,
            recent_drops: VecDeque::new(),
        }
    }
}

impl ConnectionHistory {
    pub fn record_connected(&mut self, connected_at: Instant) {
        if self.connection_span == ConnectionSpan::Disconnected {
            self.connection_span = ConnectionSpan::ConnectedSince(connected_at);
        }
    }

    pub fn record_disconnected(&mut self) {
        self.connection_span = ConnectionSpan::Disconnected;
    }

    pub fn record_drop(&mut self, connection_drop: ConnectionDrop) {
        self.connection_span = ConnectionSpan::Disconnected;
        let observed_at = connection_drop.observed_at;
        self.recent_drops.push_back(connection_drop);
        self.forget_old_drops(observed_at);
    }

    fn forget_old_drops(&mut self, now: Instant) {
        while self.recent_drops.len() > MOST_REMEMBERED_DROPS
            || self.recent_drops.front().is_some_and(|oldest_drop| {
                now.duration_since(oldest_drop.observed_at) > REMEMBERED_DROP_WINDOW
            })
        {
            self.recent_drops.pop_front();
        }
    }

    pub fn drops_within(
        &self,
        now: Instant,
        window: Duration,
    ) -> impl Iterator<Item = &ConnectionDrop> {
        self.recent_drops.iter().filter(move |connection_drop| {
            now.duration_since(connection_drop.observed_at) <= window
        })
    }

    pub fn signal_losses_within(&self, now: Instant, window: Duration) -> usize {
        self.drops_within(now, window)
            .filter(|connection_drop| connection_drop.reason == DisconnectReason::SignalLost)
            .count()
    }

    pub fn connected_for(&self, now: Instant) -> Option<Duration> {
        match self.connection_span {
            ConnectionSpan::ConnectedSince(connected_at) => Some(now.duration_since(connected_at)),
            ConnectionSpan::Disconnected => None,
        }
    }
}

fn parse_device_path(object_path: &str) -> Option<(String, bluer::Address)> {
    let (adapter_name, device_segment) = object_path
        .strip_prefix(DEVICE_PATH_PREFIX)?
        .split_once('/')?;
    let address_text = device_segment
        .strip_prefix(DEVICE_SEGMENT_PREFIX)?
        .replace('_', ":");
    Some((
        adapter_name.to_string(),
        bluer::Address::from_str(&address_text).ok()?,
    ))
}

pub async fn connection_drops()
-> Result<impl Stream<Item = ConnectionDrop> + Send + 'static, BluetoothError> {
    let (connection_resource, connection) = dbus_tokio::connection::new_system_sync()
        .map_err(|_unavailable_bus| BluetoothError::SystemBusUnavailable)?;
    let connection_task = tokio::spawn(connection_resource);
    let disconnect_rule = MatchRule::new_signal(DEVICE_INTERFACE, DISCONNECTED_SIGNAL);
    let (signal_match, disconnect_signals) = connection
        .add_match(disconnect_rule)
        .await
        .map_err(|_refused_match| BluetoothError::SystemBusUnavailable)?
        .stream::<(String, String)>();
    let connection_drops =
        disconnect_signals.filter_map(|(signal_message, (reason_name, _reason_message))| {
            let observed_drop = signal_message
                .path()
                .and_then(|object_path| parse_device_path(&object_path))
                .map(|(adapter_name, address)| ConnectionDrop {
                    adapter_name,
                    address,
                    reason: DisconnectReason::from_bluez_reason(&reason_name),
                    observed_at: Instant::now(),
                });
            future::ready(observed_drop)
        });
    Ok(async_stream::stream! {
        let _signal_subscription = signal_match;
        let _bus_connection = connection;
        let _connection_task = AbortOnDrop(connection_task);
        let mut connection_drops = std::pin::pin!(connection_drops);
        while let Some(connection_drop) = connection_drops.next().await {
            yield connection_drop;
        }
    })
}

struct AbortOnDrop(tokio::task::JoinHandle<dbus_tokio::connection::IOResourceError>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_address() -> bluer::Address {
        bluer::Address::new([0xD4, 0x2F, 0x4B, 0xAE, 0x2F, 0x50])
    }

    fn drop_at(observed_at: Instant, reason: DisconnectReason) -> ConnectionDrop {
        ConnectionDrop {
            adapter_name: "hci1".to_string(),
            address: test_address(),
            reason,
            observed_at,
        }
    }

    #[test]
    fn bluez_reasons_map_to_drop_causes() {
        assert_eq!(
            DisconnectReason::from_bluez_reason("org.bluez.Reason.Timeout"),
            DisconnectReason::SignalLost
        );
        assert_eq!(
            DisconnectReason::from_bluez_reason("org.bluez.Reason.Local"),
            DisconnectReason::ClosedHere
        );
        assert_eq!(
            DisconnectReason::from_bluez_reason("org.bluez.Reason.Something"),
            DisconnectReason::Unknown
        );
    }

    #[test]
    fn device_paths_give_adapter_and_address() {
        assert_eq!(
            parse_device_path("/org/bluez/hci1/dev_D4_2F_4B_AE_2F_50"),
            Some(("hci1".to_string(), test_address()))
        );
        assert_eq!(parse_device_path("/org/bluez/hci1"), None);
        assert_eq!(
            parse_device_path("/org/bluez/hci1/dev_D4_2F_4B_AE_2F_50/service0001"),
            None
        );
    }

    #[test]
    fn signal_losses_are_counted_within_the_window() {
        let start = Instant::now();
        let mut history = ConnectionHistory::default();
        history.record_drop(drop_at(start, DisconnectReason::SignalLost));
        history.record_drop(drop_at(
            start + Duration::from_secs(600),
            DisconnectReason::ClosedHere,
        ));
        history.record_drop(drop_at(
            start + Duration::from_secs(1200),
            DisconnectReason::SignalLost,
        ));
        let later = start + Duration::from_secs(1800);
        assert_eq!(
            history.signal_losses_within(later, Duration::from_secs(3600)),
            2
        );
        assert_eq!(
            history.signal_losses_within(later, Duration::from_secs(900)),
            1
        );
        assert_eq!(
            history
                .drops_within(later, Duration::from_secs(3600))
                .count(),
            3
        );
    }

    #[test]
    fn drops_older_than_an_hour_are_forgotten() {
        let start = Instant::now();
        let mut history = ConnectionHistory::default();
        history.record_drop(drop_at(start, DisconnectReason::SignalLost));
        history.record_drop(drop_at(
            start + Duration::from_secs(4000),
            DisconnectReason::SignalLost,
        ));
        assert_eq!(history.recent_drops.len(), 1);
    }

    #[test]
    fn connected_time_starts_at_the_first_connection() {
        let start = Instant::now();
        let mut history = ConnectionHistory::default();
        history.record_connected(start);
        history.record_connected(start + Duration::from_secs(5));
        assert_eq!(
            history.connected_for(start + Duration::from_secs(60)),
            Some(Duration::from_secs(60))
        );
        history.record_disconnected();
        assert_eq!(history.connected_for(start + Duration::from_secs(61)), None);
    }
}
