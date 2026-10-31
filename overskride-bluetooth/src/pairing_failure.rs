use std::fmt;

use crate::bluetooth_error::BluezFailure;
use crate::pairing_steps::PairingStep;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairStepFailure {
    Bluez(BluezFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BondingFailure {
    LinkKeyNotStored,
    Bluez(BluezFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationFailure {
    TrustNeedsBond,
    Bluez(BluezFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStepFailure {
    Bluez(BluezFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingFailure {
    Pairing(PairStepFailure),
    Bonding(BondingFailure),
    Authorization(AuthorizationFailure),
    Connection(ConnectionStepFailure),
}

impl PairingFailure {
    pub fn step(&self) -> PairingStep {
        match self {
            Self::Pairing(_) => PairingStep::Pairing,
            Self::Bonding(_) => PairingStep::Bonding,
            Self::Authorization(_) => PairingStep::Authorization,
            Self::Connection(_) => PairingStep::Connection,
        }
    }
}

impl From<BluezFailure> for PairStepFailure {
    fn from(bluez_failure: BluezFailure) -> Self {
        Self::Bluez(bluez_failure)
    }
}

impl From<BluezFailure> for BondingFailure {
    fn from(bluez_failure: BluezFailure) -> Self {
        Self::Bluez(bluez_failure)
    }
}

impl From<BluezFailure> for AuthorizationFailure {
    fn from(bluez_failure: BluezFailure) -> Self {
        Self::Bluez(bluez_failure)
    }
}

impl From<BluezFailure> for ConnectionStepFailure {
    fn from(bluez_failure: BluezFailure) -> Self {
        Self::Bluez(bluez_failure)
    }
}

impl From<bluer::Error> for PairStepFailure {
    fn from(bluez_error: bluer::Error) -> Self {
        Self::Bluez(bluez_error.into())
    }
}

impl From<bluer::Error> for BondingFailure {
    fn from(bluez_error: bluer::Error) -> Self {
        Self::Bluez(bluez_error.into())
    }
}

impl From<bluer::Error> for AuthorizationFailure {
    fn from(bluez_error: bluer::Error) -> Self {
        Self::Bluez(bluez_error.into())
    }
}

impl From<bluer::Error> for ConnectionStepFailure {
    fn from(bluez_error: bluer::Error) -> Self {
        Self::Bluez(bluez_error.into())
    }
}

impl From<PairStepFailure> for PairingFailure {
    fn from(step_failure: PairStepFailure) -> Self {
        Self::Pairing(step_failure)
    }
}

impl From<BondingFailure> for PairingFailure {
    fn from(step_failure: BondingFailure) -> Self {
        Self::Bonding(step_failure)
    }
}

impl From<AuthorizationFailure> for PairingFailure {
    fn from(step_failure: AuthorizationFailure) -> Self {
        Self::Authorization(step_failure)
    }
}

impl From<ConnectionStepFailure> for PairingFailure {
    fn from(step_failure: ConnectionStepFailure) -> Self {
        Self::Connection(step_failure)
    }
}

impl fmt::Display for PairStepFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bluez(bluez_failure) => write!(formatter, "{bluez_failure}"),
        }
    }
}

impl fmt::Display for BondingFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LinkKeyNotStored => formatter.write_str(
                "Pairing finished, but BlueZ did not store a link key. Put the device in pairing mode and press Pair Again.",
            ),
            Self::Bluez(bluez_failure) => write!(formatter, "{bluez_failure}"),
        }
    }
}

impl fmt::Display for AuthorizationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TrustNeedsBond => formatter.write_str(
                "Trust can't be saved: this device is paired but not bonded (no stored link key). Use Pair Again.",
            ),
            Self::Bluez(bluez_failure) => write!(formatter, "{bluez_failure}"),
        }
    }
}

impl fmt::Display for ConnectionStepFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bluez(bluez_failure) => write!(formatter, "{bluez_failure}"),
        }
    }
}

impl fmt::Display for PairingFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pairing(step_failure) => write!(formatter, "{step_failure}"),
            Self::Bonding(step_failure) => write!(formatter, "{step_failure}"),
            Self::Authorization(step_failure) => write!(formatter, "{step_failure}"),
            Self::Connection(step_failure) => write!(formatter, "{step_failure}"),
        }
    }
}

impl std::error::Error for PairingFailure {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth_error::{BluezConnectionFailure, ConnectionReason, LinkTransport};

    #[test]
    fn each_step_failure_reports_its_own_step() {
        assert_eq!(
            PairingFailure::from(BondingFailure::LinkKeyNotStored).step(),
            PairingStep::Bonding
        );
        assert_eq!(
            PairingFailure::from(AuthorizationFailure::TrustNeedsBond).step(),
            PairingStep::Authorization
        );
    }

    #[test]
    fn switched_off_device_fails_the_connection_step_with_a_typed_reason() {
        let connection_failure = ConnectionStepFailure::from(bluer::Error {
            kind: bluer::ErrorKind::Failed,
            message: "br-connection-create-socket".to_string(),
        });
        assert_eq!(
            connection_failure,
            ConnectionStepFailure::Bluez(BluezFailure::Connection(BluezConnectionFailure {
                transport: LinkTransport::BasicRate,
                reason: ConnectionReason::CreateSocket,
            }))
        );
        assert_eq!(
            PairingFailure::from(connection_failure).step(),
            PairingStep::Connection
        );
    }
}
