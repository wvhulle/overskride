use bluer::{AdapterEvent, AdapterProperty};
use futures::{Stream, StreamExt};

use crate::bluetooth_error::BluetoothError;
use crate::radios::{RadioHardware, describe_radio};
use crate::session::BluetoothSession;

const SECONDS_PER_MINUTE: u32 = 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterSnapshot {
    pub adapter_name: String,
    pub radio_label: String,
    pub hardware: RadioHardware,
    pub address: bluer::Address,
    pub address_type: bluer::AddressType,
    pub alias: String,
    pub powered: bool,
    pub discoverable: bool,
    pub pairable: bool,
    pub discovering: bool,
    pub discoverable_timeout_minutes: u32,
}

impl AdapterSnapshot {
    async fn read(adapter: &bluer::Adapter) -> Result<Self, BluetoothError> {
        let radio_description = describe_radio(adapter.name());
        Ok(Self {
            adapter_name: radio_description.adapter_name,
            radio_label: radio_description.radio_label,
            hardware: radio_description.hardware,
            address: adapter.address().await?,
            address_type: adapter.address_type().await?,
            alias: adapter.alias().await?,
            powered: adapter.is_powered().await?,
            discoverable: adapter.is_discoverable().await?,
            pairable: adapter.is_pairable().await?,
            discovering: adapter.is_discovering().await?,
            discoverable_timeout_minutes: adapter.discoverable_timeout().await?
                / SECONDS_PER_MINUTE,
        })
    }

    fn apply_property(&mut self, changed_property: AdapterProperty) -> bool {
        match changed_property {
            AdapterProperty::Powered(powered) => self.powered = powered,
            AdapterProperty::Discoverable(discoverable) => self.discoverable = discoverable,
            AdapterProperty::Pairable(pairable) => self.pairable = pairable,
            AdapterProperty::Discovering(discovering) => self.discovering = discovering,
            AdapterProperty::Alias(alias) => self.alias = alias,
            AdapterProperty::DiscoverableTimeout(timeout_seconds) => {
                self.discoverable_timeout_minutes = timeout_seconds / SECONDS_PER_MINUTE;
            }
            _ => return false,
        }
        true
    }
}

pub async fn read_adapter(
    session: &BluetoothSession,
    adapter_name: &str,
) -> Result<AdapterSnapshot, BluetoothError> {
    AdapterSnapshot::read(&session.adapter(adapter_name)?).await
}

pub async fn adapter_changes(
    session: &BluetoothSession,
    adapter_name: &str,
) -> Result<impl Stream<Item = AdapterSnapshot> + Send + 'static, BluetoothError> {
    let adapter = session.adapter(adapter_name)?;
    let initial_snapshot = AdapterSnapshot::read(&adapter).await?;
    let adapter_events = adapter.events().await?;
    let changed_snapshots = adapter_events.scan(
        initial_snapshot.clone(),
        |current_snapshot, adapter_event| {
            let changed_snapshot = match adapter_event {
                AdapterEvent::PropertyChanged(changed_property) => current_snapshot
                    .apply_property(changed_property)
                    .then(|| current_snapshot.clone()),
                _ => None,
            };
            futures::future::ready(Some(changed_snapshot))
        },
    );
    Ok(
        futures::stream::once(futures::future::ready(Some(initial_snapshot)))
            .chain(changed_snapshots)
            .filter_map(futures::future::ready),
    )
}

pub async fn set_powered(
    session: &BluetoothSession,
    adapter_name: &str,
    powered: bool,
) -> Result<AdapterSnapshot, BluetoothError> {
    let adapter = session.adapter(adapter_name)?;
    adapter.set_powered(powered).await?;
    AdapterSnapshot::read(&adapter).await
}

pub async fn set_discoverable(
    session: &BluetoothSession,
    adapter_name: &str,
    discoverable: bool,
) -> Result<AdapterSnapshot, BluetoothError> {
    let adapter = session.adapter(adapter_name)?;
    adapter.set_discoverable(discoverable).await?;
    AdapterSnapshot::read(&adapter).await
}

pub async fn set_pairable(
    session: &BluetoothSession,
    adapter_name: &str,
    pairable: bool,
) -> Result<AdapterSnapshot, BluetoothError> {
    let adapter = session.adapter(adapter_name)?;
    adapter.set_pairable(pairable).await?;
    AdapterSnapshot::read(&adapter).await
}

pub async fn set_discoverable_timeout(
    session: &BluetoothSession,
    adapter_name: &str,
    timeout_minutes: u32,
) -> Result<AdapterSnapshot, BluetoothError> {
    let adapter = session.adapter(adapter_name)?;
    adapter
        .set_discoverable_timeout(timeout_minutes * SECONDS_PER_MINUTE)
        .await?;
    AdapterSnapshot::read(&adapter).await
}

pub async fn set_alias(
    session: &BluetoothSession,
    adapter_name: &str,
    alias: String,
) -> Result<AdapterSnapshot, BluetoothError> {
    let adapter = session.adapter(adapter_name)?;
    adapter.set_alias(alias).await?;
    AdapterSnapshot::read(&adapter).await
}
