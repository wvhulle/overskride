use std::fmt;

#[derive(Debug)]
pub enum BluetoothError {
    Bluez(bluer::Error),
    Dbus(dbus::Error),
    Filesystem(std::io::Error),
    AudioServer(AudioServerFailure),
    MissingLinkKey,
    TrustNeedsBond,
    BackgroundTaskStopped,
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
        Self::Bluez(bluez_error)
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
