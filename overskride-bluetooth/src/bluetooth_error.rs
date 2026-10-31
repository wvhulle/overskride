use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BluetoothError {
    Bluez(BluezFailure),
    ObexService(ObexServiceFailure),
    Filesystem(std::io::ErrorKind),
    AudioServer(AudioServerFailure),
    BackgroundTaskStopped,
    SystemBusUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BluezFailure {
    Connection(BluezConnectionFailure),
    Request(BluezRequestFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BluezRequestFailure {
    AlreadyConnected,
    AlreadyExists,
    AuthenticationCanceled,
    AuthenticationFailed,
    AuthenticationRejected,
    AuthenticationTimeout,
    ConnectionAttemptFailed,
    DoesNotExist,
    InProgress,
    InvalidArguments,
    NotAvailable,
    NotAuthorized,
    NotReady,
    NotSupported,
    ServiceConnectionLost,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTransport {
    BasicRate,
    LowEnergy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionReason {
    AlreadyConnected,
    PageTimeout,
    ServiceSearch,
    CreateSocket,
    InvalidArguments,
    AdapterNotPowered,
    NotSupported,
    BadSocket,
    MemoryAllocation,
    Busy,
    ConcurrentConnectionLimit,
    Timeout,
    Refused,
    AbortedByRemote,
    AbortedByLocal,
    LinkProtocolError,
    Canceled,
    GattBrowsing,
    KeyMissing,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BluezConnectionFailure {
    pub transport: LinkTransport,
    pub reason: ConnectionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObexServiceFailure {
    SessionBusUnavailable,
    ServiceMissing,
    NoReply,
    InvalidArguments,
    Forbidden,
    InProgress,
    TransferGone,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioServerFailure {
    ContextUnavailable,
    CardNotFound,
    ProfileRejected,
}

const BREDR_REASON_PREFIX: &str = "br-connection-";
const LOW_ENERGY_REASON_PREFIX: &str = "le-connection-";

fn parse_connection_reason(reason_code: &str) -> Option<ConnectionReason> {
    let reason = match reason_code {
        "already-connected" => ConnectionReason::AlreadyConnected,
        "page-timeout" => ConnectionReason::PageTimeout,
        "sdp-search" => ConnectionReason::ServiceSearch,
        "create-socket" => ConnectionReason::CreateSocket,
        "invalid-argument" | "invalid-arguments" => ConnectionReason::InvalidArguments,
        "adapter-not-powered" => ConnectionReason::AdapterNotPowered,
        "not-supported" => ConnectionReason::NotSupported,
        "bad-socket" => ConnectionReason::BadSocket,
        "memory-allocation" => ConnectionReason::MemoryAllocation,
        "busy" => ConnectionReason::Busy,
        "concurrent-connection-limit" => ConnectionReason::ConcurrentConnectionLimit,
        "timeout" => ConnectionReason::Timeout,
        "refused" => ConnectionReason::Refused,
        "aborted-by-remote" | "abort-by-remote" => ConnectionReason::AbortedByRemote,
        "aborted-by-local" | "abort-by-local" => ConnectionReason::AbortedByLocal,
        "lmp-protocol-error" | "link-layer-protocol-error" => ConnectionReason::LinkProtocolError,
        "canceled" => ConnectionReason::Canceled,
        "gatt-browsing" => ConnectionReason::GattBrowsing,
        "key-missing" => ConnectionReason::KeyMissing,
        "unknown" => ConnectionReason::Unknown,
        _ => return None,
    };
    Some(reason)
}

fn parse_connection_failure(bluez_message: &str) -> Option<BluezConnectionFailure> {
    if let Some(reason_code) = bluez_message.strip_prefix(BREDR_REASON_PREFIX) {
        return Some(BluezConnectionFailure {
            transport: LinkTransport::BasicRate,
            reason: parse_connection_reason(reason_code)?,
        });
    }
    let reason_code = bluez_message.strip_prefix(LOW_ENERGY_REASON_PREFIX)?;
    Some(BluezConnectionFailure {
        transport: LinkTransport::LowEnergy,
        reason: parse_connection_reason(reason_code)?,
    })
}

impl From<bluer::ErrorKind> for BluezRequestFailure {
    fn from(bluez_kind: bluer::ErrorKind) -> Self {
        match bluez_kind {
            bluer::ErrorKind::AlreadyConnected => Self::AlreadyConnected,
            bluer::ErrorKind::AlreadyExists => Self::AlreadyExists,
            bluer::ErrorKind::AuthenticationCanceled => Self::AuthenticationCanceled,
            bluer::ErrorKind::AuthenticationFailed => Self::AuthenticationFailed,
            bluer::ErrorKind::AuthenticationRejected => Self::AuthenticationRejected,
            bluer::ErrorKind::AuthenticationTimeout => Self::AuthenticationTimeout,
            bluer::ErrorKind::ConnectionAttemptFailed => Self::ConnectionAttemptFailed,
            bluer::ErrorKind::DoesNotExist | bluer::ErrorKind::NotFound => Self::DoesNotExist,
            bluer::ErrorKind::InProgress => Self::InProgress,
            bluer::ErrorKind::InvalidArguments => Self::InvalidArguments,
            bluer::ErrorKind::NotAvailable => Self::NotAvailable,
            bluer::ErrorKind::NotAuthorized | bluer::ErrorKind::NotPermitted => Self::NotAuthorized,
            bluer::ErrorKind::NotReady => Self::NotReady,
            bluer::ErrorKind::NotSupported => Self::NotSupported,
            bluer::ErrorKind::Internal(bluer::InternalErrorKind::DBusConnectionLost) => {
                Self::ServiceConnectionLost
            }
            _ => Self::Failed,
        }
    }
}

impl From<&dbus::Error> for ObexServiceFailure {
    fn from(dbus_error: &dbus::Error) -> Self {
        match dbus_error.name().unwrap_or_default() {
            "org.freedesktop.DBus.Error.NoServer"
            | "org.freedesktop.DBus.Error.FileNotFound"
            | "org.freedesktop.DBus.Error.Disconnected" => Self::SessionBusUnavailable,
            "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NameHasNoOwner" => Self::ServiceMissing,
            "org.freedesktop.DBus.Error.NoReply"
            | "org.freedesktop.DBus.Error.Timeout"
            | "org.freedesktop.DBus.Error.TimedOut" => Self::NoReply,
            "org.bluez.obex.Error.InvalidArguments" | "org.freedesktop.DBus.Error.InvalidArgs" => {
                Self::InvalidArguments
            }
            "org.bluez.obex.Error.Forbidden" | "org.bluez.obex.Error.NotAuthorized" => {
                Self::Forbidden
            }
            "org.bluez.obex.Error.InProgress" => Self::InProgress,
            "org.bluez.obex.Error.NotFound" | "org.freedesktop.DBus.Error.UnknownObject" => {
                Self::TransferGone
            }
            _ => Self::Failed,
        }
    }
}

impl From<bluer::Error> for BluezFailure {
    fn from(bluez_error: bluer::Error) -> Self {
        match parse_connection_failure(&bluez_error.message) {
            Some(connection_failure) => Self::Connection(connection_failure),
            None => Self::Request(bluez_error.kind.into()),
        }
    }
}

impl From<BluezFailure> for BluetoothError {
    fn from(bluez_failure: BluezFailure) -> Self {
        Self::Bluez(bluez_failure)
    }
}

impl From<bluer::Error> for BluetoothError {
    fn from(bluez_error: bluer::Error) -> Self {
        Self::Bluez(bluez_error.into())
    }
}

impl From<dbus::Error> for BluetoothError {
    fn from(dbus_error: dbus::Error) -> Self {
        Self::ObexService(ObexServiceFailure::from(&dbus_error))
    }
}

impl From<std::io::Error> for BluetoothError {
    fn from(io_error: std::io::Error) -> Self {
        Self::Filesystem(io_error.kind())
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

impl fmt::Display for ConnectionReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PageTimeout | Self::CreateSocket | Self::Timeout => {
                "The device did not answer. Turn it on and keep it within a few meters, then press \
                 Connect again."
            }
            Self::KeyMissing => {
                "The device no longer knows this computer, for example after it was paired with \
                 another one. Put it in pairing mode and press Pair Again."
            }
            Self::Refused | Self::AbortedByRemote => {
                "The device refused the connection. Disconnect it from other phones or computers, \
                 then try again."
            }
            Self::ServiceSearch | Self::GattBrowsing => {
                "The device connected but did not list its services. Wait a few seconds after \
                 turning it on, then try again."
            }
            Self::AdapterNotPowered => {
                "Bluetooth is turned off on this radio. Turn on Powered in Bluetooth Settings."
            }
            Self::Busy | Self::ConcurrentConnectionLimit => {
                "The radio is busy with another connection attempt. Wait a few seconds, then try \
                 again."
            }
            Self::AlreadyConnected => "The device is already connected.",
            Self::Canceled | Self::AbortedByLocal => {
                "The connection attempt was canceled on this computer."
            }
            Self::NotSupported | Self::InvalidArguments => {
                "This radio does not support connecting to this kind of device."
            }
            Self::BadSocket | Self::MemoryAllocation => {
                "BlueZ ran into an internal problem. Restart the Bluetooth service with `systemctl \
                 restart bluetooth`, then try again."
            }
            Self::LinkProtocolError => {
                "The radio link broke down while connecting, often because of interference. Move \
                 closer to the computer and try again."
            }
            Self::Unknown => {
                "BlueZ could not tell why the connection failed. Turn the device off and on again, \
                 then press Connect."
            }
        })
    }
}

impl fmt::Display for BluezRequestFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ConnectionAttemptFailed => {
                "The device did not answer. Turn it on and keep it within a few meters, then try \
                 again."
            }
            Self::AuthenticationFailed | Self::AuthenticationRejected => {
                "Pairing was rejected: the PIN or passkey did not match, or the device left \
                 pairing mode. Put it in pairing mode and press Pair again."
            }
            Self::AuthenticationTimeout => {
                "The device did not finish pairing in time. Keep it in pairing mode and press Pair \
                 again."
            }
            Self::AuthenticationCanceled => "Pairing was canceled.",
            Self::AlreadyConnected => "The device is already connected.",
            Self::AlreadyExists => "The device is already paired.",
            Self::InProgress => {
                "Another attempt is still running. Wait a few seconds, then try again."
            }
            Self::InvalidArguments => "BlueZ rejected the request as invalid.",
            Self::NotReady => {
                "Bluetooth is turned off on this radio. Turn on Powered in Bluetooth Settings."
            }
            Self::DoesNotExist => "BlueZ no longer knows this device. Press Scan to find it again.",
            Self::NotAuthorized => {
                "BlueZ refused the request. Check that your user may use Bluetooth."
            }
            Self::NotSupported | Self::NotAvailable => {
                "This radio or device does not support the request."
            }
            Self::ServiceConnectionLost => {
                "The connection to the Bluetooth service was lost. Check that bluetoothd is \
                 running, then restart Overskride."
            }
            Self::Failed => {
                "BlueZ could not complete the request. Turn the device off and on again, then \
                 retry."
            }
        })
    }
}

impl fmt::Display for BluezFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(connection_failure) => {
                write!(formatter, "{}", connection_failure.reason)
            }
            Self::Request(request_failure) => write!(formatter, "{request_failure}"),
        }
    }
}

impl fmt::Display for ObexServiceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SessionBusUnavailable => {
                "No D-Bus session bus is running, so files cannot be sent or received."
            }
            Self::ServiceMissing => {
                "The file transfer service (obexd) is not running. Install bluez-obex or start \
                 obex.service, then try again."
            }
            Self::NoReply => "The file transfer service did not answer in time. Try again.",
            Self::InvalidArguments => "The file transfer service rejected the request as invalid.",
            Self::Forbidden => "The device refused the file transfer.",
            Self::InProgress => "Another file transfer is still running. Wait for it to finish.",
            Self::TransferGone => "The transfer no longer exists; it finished or was canceled.",
            Self::Failed => {
                "The file transfer failed. Check that the device accepts files, then try again."
            }
        })
    }
}

impl fmt::Display for AudioServerFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ContextUnavailable => "Cannot connect to the PulseAudio server.",
            Self::CardNotFound => {
                "The audio server does not know this device yet. Connect it first."
            }
            Self::ProfileRejected => "The audio server rejected the profile.",
        })
    }
}

impl fmt::Display for BluetoothError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bluez(bluez_failure) => write!(formatter, "{bluez_failure}"),
            Self::ObexService(obex_failure) => write!(formatter, "{obex_failure}"),
            Self::Filesystem(io_kind) => {
                write!(formatter, "Could not store the received file: {io_kind}.")
            }
            Self::AudioServer(audio_failure) => write!(formatter, "{audio_failure}"),
            Self::BackgroundTaskStopped => {
                formatter.write_str("A background Bluetooth task stopped unexpectedly.")
            }
            Self::SystemBusUnavailable => {
                formatter.write_str("Cannot listen for Bluetooth events on the system bus.")
            }
        }
    }
}

impl std::error::Error for BluetoothError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn bluez_failure(kind: bluer::ErrorKind, message: &str) -> BluezFailure {
        BluezFailure::from(bluer::Error {
            kind,
            message: message.to_string(),
        })
    }

    #[test]
    fn switched_off_device_is_a_typed_socket_failure() {
        let connection_error =
            bluez_failure(bluer::ErrorKind::Failed, "br-connection-create-socket");
        assert_eq!(
            connection_error,
            BluezFailure::Connection(BluezConnectionFailure {
                transport: LinkTransport::BasicRate,
                reason: ConnectionReason::CreateSocket,
            })
        );
        assert!(connection_error.to_string().contains("Turn it on"));
    }

    #[test]
    fn low_energy_spellings_map_to_the_same_reasons() {
        assert_eq!(
            bluez_failure(bluer::ErrorKind::Failed, "le-connection-abort-by-remote"),
            BluezFailure::Connection(BluezConnectionFailure {
                transport: LinkTransport::LowEnergy,
                reason: ConnectionReason::AbortedByRemote,
            })
        );
    }

    #[test]
    fn every_bluez_reason_code_is_typed() {
        [
            "br-connection-already-connected",
            "br-connection-page-timeout",
            "br-connection-sdp-search",
            "br-connection-create-socket",
            "br-connection-invalid-argument",
            "br-connection-adapter-not-powered",
            "br-connection-not-supported",
            "br-connection-bad-socket",
            "br-connection-memory-allocation",
            "br-connection-busy",
            "br-connection-concurrent-connection-limit",
            "br-connection-timeout",
            "br-connection-refused",
            "br-connection-aborted-by-remote",
            "br-connection-aborted-by-local",
            "br-connection-lmp-protocol-error",
            "br-connection-canceled",
            "br-connection-key-missing",
            "br-connection-unknown",
            "le-connection-invalid-arguments",
            "le-connection-adapter-not-powered",
            "le-connection-not-supported",
            "le-connection-already-connected",
            "le-connection-bad-socket",
            "le-connection-memory-allocation",
            "le-connection-busy",
            "le-connection-refused",
            "le-connection-create-socket",
            "le-connection-timeout",
            "le-connection-concurrent-connection-limit",
            "le-connection-abort-by-remote",
            "le-connection-abort-by-local",
            "le-connection-link-layer-protocol-error",
            "le-connection-gatt-browsing",
            "le-connection-key-missing",
            "le-connection-unknown",
        ]
        .iter()
        .for_each(|reason_code| {
            assert!(
                parse_connection_failure(reason_code).is_some(),
                "{reason_code} is not typed"
            );
        });
    }

    #[test]
    fn other_bluez_failures_keep_only_the_typed_kind() {
        assert_eq!(
            bluez_failure(
                bluer::ErrorKind::AuthenticationRejected,
                "Authentication Rejected"
            ),
            BluezFailure::Request(BluezRequestFailure::AuthenticationRejected)
        );
    }

    #[test]
    fn missing_obex_daemon_is_typed() {
        let dbus_error =
            dbus::Error::new_custom("org.freedesktop.DBus.Error.ServiceUnknown", "no obexd");
        assert_eq!(
            BluetoothError::from(dbus_error),
            BluetoothError::ObexService(ObexServiceFailure::ServiceMissing)
        );
    }
}
