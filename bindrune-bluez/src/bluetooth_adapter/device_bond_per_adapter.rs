use crate::{
    bluetooth_adapter::adapter_udev_hardware::AdapterHardwareDescription,
    bluez_dbus_session::{BluezDbusSession, DeviceLocation},
    bluez_operation_error::BluetoothError,
};

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
pub struct DeviceBondPerAdapter {
    pub radio: AdapterHardwareDescription,
    pub adapter_address: bluer::Address,
    pub shown: bool,
    pub membership: RadioMembership,
}

impl DeviceBondPerAdapter {
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
    session: &BluezDbusSession,
    radio: AdapterHardwareDescription,
    shown_location: &DeviceLocation,
) -> Result<DeviceBondPerAdapter, BluetoothError> {
    let adapter = session.adapter(&radio.adapter_name)?;
    Ok(DeviceBondPerAdapter {
        adapter_address: adapter.address().await?,
        shown: radio.adapter_name == shown_location.adapter_name,
        membership: read_radio_membership(&adapter, shown_location.address).await?,
        radio,
    })
}

pub async fn read_radio_bonds(
    session: &BluezDbusSession,
    shown_location: &DeviceLocation,
) -> Result<Vec<DeviceBondPerAdapter>, BluetoothError> {
    let radio_bond_reads = session
        .radios()
        .await?
        .into_iter()
        .map(|radio| read_radio_bond(session, radio, shown_location));
    futures::future::try_join_all(radio_bond_reads).await
}
