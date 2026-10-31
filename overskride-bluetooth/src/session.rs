use crate::bluetooth_error::{BluetoothError, BluezFailure};
use crate::radios::{RadioDescription, describe_radio};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceLocation {
    pub adapter_name: String,
    pub address: bluer::Address,
}

#[derive(Clone)]
pub struct BluetoothSession {
    bluez_session: bluer::Session,
}

impl std::fmt::Debug for BluetoothSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BluetoothSession")
    }
}

impl BluetoothSession {
    pub async fn open() -> Result<Self, BluetoothError> {
        Ok(Self {
            bluez_session: bluer::Session::new().await?,
        })
    }

    pub async fn radios(&self) -> Result<Vec<RadioDescription>, BluetoothError> {
        let mut adapter_names = self.bluez_session.adapter_names().await?;
        adapter_names.sort();
        Ok(adapter_names
            .into_iter()
            .map(|adapter_name| describe_radio(&adapter_name))
            .collect())
    }

    pub async fn default_adapter_name(&self) -> Result<String, BluetoothError> {
        Ok(self
            .bluez_session
            .default_adapter()
            .await?
            .name()
            .to_string())
    }

    pub(crate) fn bluez_session(&self) -> &bluer::Session {
        &self.bluez_session
    }

    pub(crate) fn adapter(&self, adapter_name: &str) -> Result<bluer::Adapter, BluezFailure> {
        Ok(self.bluez_session.adapter(adapter_name)?)
    }

    pub(crate) fn device(
        &self,
        device_location: &DeviceLocation,
    ) -> Result<bluer::Device, BluezFailure> {
        Ok(self
            .adapter(&device_location.adapter_name)?
            .device(device_location.address)?)
    }
}
