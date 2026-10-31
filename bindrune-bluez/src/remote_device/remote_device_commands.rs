use futures::{Stream, StreamExt, future, stream};

use crate::{
    bluez_dbus_session::{BluezDbusSession, DeviceLocation},
    bluez_operation_error::BluetoothError,
    device_pairing::{
        pairing_step_failure::{
            AuthorizationFailure, BondingFailure, ConnectionStepFailure, PairStepFailure,
            PairingFailure,
        },
        pairing_step_progress::{ConnectionPolicy, PairingProgress, PairingStep},
    },
    remote_device::remote_device_snapshot::DeviceSnapshot,
};

const PAIRING_SEQUENCE: [PairingStep; 4] = [
    PairingStep::Pairing,
    PairingStep::Bonding,
    PairingStep::Authorization,
    PairingStep::Connection,
];

pub async fn read_device(
    session: &BluezDbusSession,
    location: &DeviceLocation,
) -> Result<DeviceSnapshot, BluetoothError> {
    DeviceSnapshot::read(&session.device(location)?, location.clone()).await
}

pub async fn connect_device(
    session: &BluezDbusSession,
    location: &DeviceLocation,
) -> Result<(), ConnectionStepFailure> {
    Ok(session.device(location)?.connect().await?)
}

pub async fn disconnect_device(
    session: &BluezDbusSession,
    location: &DeviceLocation,
) -> Result<(), ConnectionStepFailure> {
    Ok(session.device(location)?.disconnect().await?)
}

pub async fn rename_device(
    session: &BluezDbusSession,
    location: &DeviceLocation,
    alias: &str,
) -> Result<(), BluetoothError> {
    let device = session.device(location)?;
    Ok(device.set_alias(alias.trim().to_string()).await?)
}

pub async fn forget_device(
    session: &BluezDbusSession,
    location: &DeviceLocation,
) -> Result<(), BluetoothError> {
    Ok(session
        .adapter(&location.adapter_name)?
        .remove_device(location.address)
        .await?)
}

pub async fn set_connection_policy(
    session: &BluezDbusSession,
    location: &DeviceLocation,
    connection_policy: ConnectionPolicy,
) -> Result<(), AuthorizationFailure> {
    let device = session.device(location)?;
    match connection_policy {
        ConnectionPolicy::Blocked => device.set_blocked(true).await?,
        ConnectionPolicy::AskEachTime => {
            device.set_blocked(false).await?;
            device.set_trusted(false).await?;
        }
        ConnectionPolicy::Trusted => {
            let paired = device.is_paired().await?;
            if !device.is_bonded().await?.unwrap_or(paired) {
                return Err(AuthorizationFailure::TrustNeedsBond);
            }
            device.set_blocked(false).await?;
            device.set_trusted(true).await?;
        }
    }
    Ok(())
}

async fn pair_with_device(device: &bluer::Device) -> Result<(), PairStepFailure> {
    Ok(device.pair().await?)
}

async fn confirm_bond(device: &bluer::Device) -> Result<(), BondingFailure> {
    let paired = device.is_paired().await?;
    if !device.is_bonded().await?.unwrap_or(paired) {
        return Err(BondingFailure::LinkKeyNotStored);
    }
    Ok(())
}

async fn trust_device(device: &bluer::Device) -> Result<(), AuthorizationFailure> {
    Ok(device.set_trusted(true).await?)
}

async fn connect_after_pairing(device: &bluer::Device) -> Result<(), ConnectionStepFailure> {
    if device.connect().await.is_err() {
        device.connect().await?;
    }
    Ok(())
}

async fn run_pairing_step(device: &bluer::Device, step: PairingStep) -> Result<(), PairingFailure> {
    match step {
        PairingStep::Pairing => pair_with_device(device).await?,
        PairingStep::Bonding => confirm_bond(device).await?,
        PairingStep::Authorization => trust_device(device).await?,
        PairingStep::Connection => connect_after_pairing(device).await?,
        PairingStep::Signal | PairingStep::InputDevice => {}
    }
    Ok(())
}

fn pairing_step_progress(
    device: bluer::Device,
    step: PairingStep,
) -> impl Stream<Item = Option<PairingProgress>> {
    let step_outcome = async move {
        run_pairing_step(&device, step)
            .await
            .err()
            .map(PairingProgress::Failed)
    };
    stream::once(future::ready(Some(PairingProgress::Running(step))))
        .chain(stream::once(step_outcome))
}

pub fn pair_trust_and_connect(
    session: &BluezDbusSession,
    location: &DeviceLocation,
) -> Result<impl Stream<Item = PairingProgress> + Send + 'static, PairStepFailure> {
    let device = session.device(location)?;
    let progress_events = stream::iter(PAIRING_SEQUENCE)
        .flat_map(move |step| pairing_step_progress(device.clone(), step))
        .chain(stream::once(future::ready(Some(PairingProgress::Finished))))
        .filter_map(future::ready);
    Ok(progress_events.scan(false, |pairing_failed, progress| {
        if *pairing_failed {
            return future::ready(None);
        }
        *pairing_failed = matches!(progress, PairingProgress::Failed(_));
        future::ready(Some(progress))
    }))
}
