use futures::{Stream, StreamExt, future, stream};

use crate::bluetooth_error::BluetoothError;
use crate::device_snapshot::DeviceSnapshot;
use crate::pairing_steps::{ConnectionPolicy, PairingProgress, PairingStep};
use crate::session::{BluetoothSession, DeviceLocation};

const PAIRING_SEQUENCE: [PairingStep; 4] = [
    PairingStep::Pairing,
    PairingStep::Bonding,
    PairingStep::Authorization,
    PairingStep::Connection,
];

pub async fn read_device(
    session: &BluetoothSession,
    location: &DeviceLocation,
) -> Result<DeviceSnapshot, BluetoothError> {
    DeviceSnapshot::read(&session.device(location)?, location.clone()).await
}

pub async fn connect_device(
    session: &BluetoothSession,
    location: &DeviceLocation,
) -> Result<(), BluetoothError> {
    Ok(session.device(location)?.connect().await?)
}

pub async fn disconnect_device(
    session: &BluetoothSession,
    location: &DeviceLocation,
) -> Result<(), BluetoothError> {
    Ok(session.device(location)?.disconnect().await?)
}

pub async fn rename_device(
    session: &BluetoothSession,
    location: &DeviceLocation,
    alias: &str,
) -> Result<String, BluetoothError> {
    let device = session.device(location)?;
    device.set_alias(alias.trim().to_string()).await?;
    Ok(device.alias().await?)
}

pub async fn forget_device(
    session: &BluetoothSession,
    location: &DeviceLocation,
) -> Result<(), BluetoothError> {
    Ok(session
        .adapter(&location.adapter_name)?
        .remove_device(location.address)
        .await?)
}

pub async fn set_connection_policy(
    session: &BluetoothSession,
    location: &DeviceLocation,
    connection_policy: ConnectionPolicy,
) -> Result<(), BluetoothError> {
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
                return Err(BluetoothError::TrustNeedsBond);
            }
            device.set_blocked(false).await?;
            device.set_trusted(true).await?;
        }
    }
    Ok(())
}

async fn run_pairing_step(device: &bluer::Device, step: PairingStep) -> Result<(), BluetoothError> {
    match step {
        PairingStep::Pairing => device.pair().await?,
        PairingStep::Bonding => {
            let paired = device.is_paired().await?;
            if !device.is_bonded().await?.unwrap_or(paired) {
                return Err(BluetoothError::MissingLinkKey);
            }
        }
        PairingStep::Authorization => device.set_trusted(true).await?,
        PairingStep::Connection => {
            if device.connect().await.is_err() {
                device.connect().await?;
            }
        }
        PairingStep::InputDevice => {}
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
            .map(|step_error| PairingProgress::Failed(step, step_error))
    };
    stream::once(future::ready(Some(PairingProgress::Running(step))))
        .chain(stream::once(step_outcome))
}

pub fn pair_trust_and_connect(
    session: &BluetoothSession,
    location: &DeviceLocation,
) -> Result<impl Stream<Item = PairingProgress> + Send + 'static, BluetoothError> {
    let device = session.device(location)?;
    let progress_events = stream::iter(PAIRING_SEQUENCE)
        .flat_map(move |step| pairing_step_progress(device.clone(), step))
        .chain(stream::once(future::ready(Some(PairingProgress::Finished))))
        .filter_map(future::ready);
    Ok(progress_events.scan(false, |pairing_failed, progress| {
        if *pairing_failed {
            return future::ready(None);
        }
        *pairing_failed = matches!(progress, PairingProgress::Failed(..));
        future::ready(Some(progress))
    }))
}
