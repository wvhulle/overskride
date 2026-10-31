use std::fmt;

#[derive(Debug)]
pub enum BluetoothError {
    Bluez(bluer::Error),
    Dbus(dbus::Error),
    Filesystem(std::io::Error),
    AudioServer(AudioServerFailure),
    Connection(ConnectionFailure),
    MissingLinkKey,
    TrustNeedsBond,
    BackgroundTaskStopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionFailure {
    DeviceUnreachable,
    LinkKeyMissing,
    RefusedByDevice,
    ProfileUnavailable,
    AdapterOff,
    AttemptInProgress,
    Canceled,
    AuthenticationRejected,
    AuthenticationTimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioServerFailure {
    ContextUnavailable,
    CardNotFound,
    ProfileRejected,
}

impl fmt::Display for BluetoothError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bluez(bluez_error) => write!(formatter, "{}", bluez_error.message),
            Self::Dbus(dbus_error) => write!(
                formatter,
                "{}",
                dbus_error.message().unwrap_or("D-Bus call failed")
            ),
            Self::Filesystem(io_error) => write!(formatter, "{io_error}"),
            Self::AudioServer(audio_failure) => write!(formatter, "{audio_failure}"),
            Self::Connection(connection_failure) => write!(formatter, "{connection_failure}"),
            Self::MissingLinkKey => write!(
                formatter,
                "Pairing finished, but BlueZ did not store a link key. Put the device in pairing mode and press Pair Again."
            ),
            Self::TrustNeedsBond => write!(
                formatter,
                "Trust can't be saved: this device is paired but not bonded (no stored link key). Use Pair Again."
            ),
            Self::BackgroundTaskStopped => write!(
                formatter,
                "A background Bluetooth task stopped unexpectedly"
            ),
        }
    }
}

impl fmt::Display for ConnectionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let explanation = match self {
            Self::DeviceUnreachable => {
                "The device did not answer. Turn it on and keep it within a few meters, then press Connect again."
            }
            Self::LinkKeyMissing => {
                "The device no longer knows this computer, for example after it was paired with another one. Put it in pairing mode and press Pair Again."
            }
            Self::RefusedByDevice => {
                "The device refused the connection. Disconnect it from other phones or computers, then try again."
            }
            Self::ProfileUnavailable => {
                "The device offers no connection this computer can use yet. Wait a few seconds after turning it on, then try again."
            }
            Self::AdapterOff => {
                "Bluetooth is turned off on this radio. Turn on Powered in Bluetooth Settings."
            }
            Self::AttemptInProgress => {
                "Another connection attempt is still running. Wait a few seconds, then try again."
            }
            Self::Canceled => "The attempt was canceled.",
            Self::AuthenticationRejected => {
                "Pairing was rejected: the PIN or passkey did not match, or the device left pairing mode. Put it in pairing mode and press Pair again."
            }
            Self::AuthenticationTimedOut => {
                "The device did not finish pairing in time. Keep it in pairing mode and press Pair again."
            }
        };
        formatter.write_str(explanation)
    }
}

const BLUEZ_REASON_PREFIXES: [&str; 2] = ["br-connection-", "le-connection-"];

fn classify_bluez_reason(reason: &str) -> Option<ConnectionFailure> {
    match reason {
        "page-timeout" | "create-socket" | "timeout" | "bad-socket" | "lmp-protocol-error" => {
            Some(ConnectionFailure::DeviceUnreachable)
        }
        "key-missing" => Some(ConnectionFailure::LinkKeyMissing),
        "refused" | "aborted-by-remote" | "abort-by-remote" => {
            Some(ConnectionFailure::RefusedByDevice)
        }
        "profile-unavailable" | "sdp-search" | "gatt-browsing" => {
            Some(ConnectionFailure::ProfileUnavailable)
        }
        "adapter-not-powered" => Some(ConnectionFailure::AdapterOff),
        "busy" | "concurrent-connection-limit" => Some(ConnectionFailure::AttemptInProgress),
        "canceled" | "aborted-by-local" | "abort-by-local" => Some(ConnectionFailure::Canceled),
        _ => None,
    }
}

fn classify_bluez_error(bluez_error: &bluer::Error) -> Option<ConnectionFailure> {
    let failure_from_kind = match bluez_error.kind {
        bluer::ErrorKind::ConnectionAttemptFailed => Some(ConnectionFailure::DeviceUnreachable),
        bluer::ErrorKind::AuthenticationFailed | bluer::ErrorKind::AuthenticationRejected => {
            Some(ConnectionFailure::AuthenticationRejected)
        }
        bluer::ErrorKind::AuthenticationTimeout => Some(ConnectionFailure::AuthenticationTimedOut),
        bluer::ErrorKind::AuthenticationCanceled => Some(ConnectionFailure::Canceled),
        bluer::ErrorKind::InProgress => Some(ConnectionFailure::AttemptInProgress),
        bluer::ErrorKind::NotReady => Some(ConnectionFailure::AdapterOff),
        _ => None,
    };
    let failure_from_reason = BLUEZ_REASON_PREFIXES
        .iter()
        .find_map(|reason_prefix| bluez_error.message.strip_prefix(reason_prefix))
        .and_then(classify_bluez_reason);
    failure_from_reason.or(failure_from_kind)
}

impl fmt::Display for AudioServerFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContextUnavailable => {
                write!(formatter, "Cannot connect to the PulseAudio server")
            }
            Self::CardNotFound => write!(formatter, "The audio server does not know this device"),
            Self::ProfileRejected => write!(formatter, "The audio server rejected the profile"),
        }
    }
}

impl std::error::Error for BluetoothError {}

impl From<bluer::Error> for BluetoothError {
    fn from(bluez_error: bluer::Error) -> Self {
        match classify_bluez_error(&bluez_error) {
            Some(connection_failure) => Self::Connection(connection_failure),
            None => Self::Bluez(bluez_error),
        }
    }
}

impl From<dbus::Error> for BluetoothError {
    fn from(dbus_error: dbus::Error) -> Self {
        Self::Dbus(dbus_error)
    }
}

impl From<std::io::Error> for BluetoothError {
    fn from(io_error: std::io::Error) -> Self {
        Self::Filesystem(io_error)
    }
}

impl From<AudioServerFailure> for BluetoothError {
    fn from(audio_failure: AudioServerFailure) -> Self {
        Self::AudioServer(audio_failure)
    }
}

impl From<tokio::task::JoinError> for BluetoothError {
    fn from(_join_error: tokio::task::JoinError) -> Self {
        Self::BackgroundTaskStopped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bluez_failure(kind: bluer::ErrorKind, message: &str) -> BluetoothError {
        BluetoothError::from(bluer::Error {
            kind,
            message: message.to_string(),
        })
    }

    #[test]
    fn switched_off_device_is_reported_as_unreachable() {
        let connection_error =
            bluez_failure(bluer::ErrorKind::Failed, "br-connection-create-socket");
        assert!(matches!(
            connection_error,
            BluetoothError::Connection(ConnectionFailure::DeviceUnreachable)
        ));
        assert!(connection_error.to_string().contains("Turn it on"));
    }

    #[test]
    fn missing_key_on_low_energy_link_suggests_pairing_again() {
        assert!(matches!(
            bluez_failure(bluer::ErrorKind::Failed, "le-connection-key-missing"),
            BluetoothError::Connection(ConnectionFailure::LinkKeyMissing)
        ));
    }

    #[test]
    fn rejected_authentication_is_classified_by_kind() {
        assert!(matches!(
            bluez_failure(
                bluer::ErrorKind::AuthenticationRejected,
                "Authentication Rejected"
            ),
            BluetoothError::Connection(ConnectionFailure::AuthenticationRejected)
        ));
    }

    #[test]
    fn unknown_reasons_keep_the_original_message() {
        let unknown_error = bluez_failure(bluer::ErrorKind::Failed, "br-connection-unknown");
        assert!(matches!(unknown_error, BluetoothError::Bluez(_)));
        assert_eq!(unknown_error.to_string(), "br-connection-unknown");
    }
}
