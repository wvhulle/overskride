use crate::bluetooth_error::BluetoothError;
use crate::radios::RadioDescription;
use crate::session::{BluetoothSession, DeviceLocation};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RadioDeviceState {
    pub bonded: bool,
    pub trusted: bool,
    pub connected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioMembership {
    Unknown,
    Known(RadioDeviceState),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioBond {
    pub radio: RadioDescription,
    pub adapter_address: bluer::Address,
    pub shown: bool,
    pub membership: RadioMembership,
}

impl RadioBond {
    pub fn is_bonded(&self) -> bool {
        matches!(
            self.membership,
            RadioMembership::Known(RadioDeviceState { bonded: true, .. })
        )
    }
}

async fn read_radio_membership(
    adapter: &bluer::Adapter,
    address: bluer::Address,
) -> Result<RadioMembership, BluetoothError> {
    if !adapter.device_addresses().await?.contains(&address) {
        return Ok(RadioMembership::Unknown);
    }
    let radio_device = adapter.device(address)?;
    let paired = radio_device.is_paired().await?;
    Ok(RadioMembership::Known(RadioDeviceState {
        bonded: radio_device.is_bonded().await?.unwrap_or(paired),
        trusted: radio_device.is_trusted().await?,
        connected: radio_device.is_connected().await?,
    }))
}

async fn read_radio_bond(
    session: &BluetoothSession,
    radio: RadioDescription,
    shown_location: &DeviceLocation,
) -> Result<RadioBond, BluetoothError> {
    let adapter = session.adapter(&radio.adapter_name)?;
    Ok(RadioBond {
        adapter_address: adapter.address().await?,
        shown: radio.adapter_name == shown_location.adapter_name,
        membership: read_radio_membership(&adapter, shown_location.address).await?,
        radio,
    })
}

pub async fn read_radio_bonds(
    session: &BluetoothSession,
    shown_location: &DeviceLocation,
) -> Result<Vec<RadioBond>, BluetoothError> {
    let radio_bond_reads = session
        .radios()
        .await?
        .into_iter()
        .map(|radio| read_radio_bond(session, radio, shown_location));
    futures::future::try_join_all(radio_bond_reads).await
}
