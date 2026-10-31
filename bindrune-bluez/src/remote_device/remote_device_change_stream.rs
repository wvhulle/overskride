use std::{collections::HashMap, time::Duration};

use bluer::{AdapterEvent, DeviceEvent};
use futures::{
    Stream, StreamExt,
    stream::{FuturesUnordered, SelectAll},
};

use crate::{
    bluez_dbus_session::{BluezDbusSession, DeviceLocation},
    bluez_operation_error::BluetoothError,
    remote_device::remote_device_snapshot::{DeviceSnapshot, PropertyRelevance},
};

const KERNEL_INPUT_SETTLE_DELAY: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
pub enum DeviceChange {
    Appeared(DeviceSnapshot),
    Updated(DeviceSnapshot),
    Disappeared(bluer::Address),
}

struct AddressedDeviceEvent {
    address: bluer::Address,
    device_event: DeviceEvent,
}

enum WatcherWakeup {
    Adapter(AdapterEvent),
    Device(AddressedDeviceEvent),
    KernelInputSettled(bluer::Address),
    Finished,
}

struct SubscribedDevice<DeviceEvents> {
    snapshot: DeviceSnapshot,
    device_events: DeviceEvents,
}

enum AdapterReaction<DeviceEvents> {
    Subscribe(SubscribedDevice<DeviceEvents>),
    Report(DeviceChange),
    Nothing,
}

enum DeviceEventOutcome {
    Unchanged,
    Changed(DeviceSnapshot),
    ConnectedAwaitingKernelInput(DeviceSnapshot),
}

struct DeviceSnapshots {
    adapter: bluer::Adapter,
    snapshots: HashMap<bluer::Address, DeviceSnapshot>,
}

async fn kernel_input_settled(address: bluer::Address) -> bluer::Address {
    tokio::time::sleep(KERNEL_INPUT_SETTLE_DELAY).await;
    address
}

impl DeviceSnapshots {
    async fn add_device(
        &mut self,
        address: bluer::Address,
    ) -> Result<
        SubscribedDevice<impl Stream<Item = AddressedDeviceEvent> + Unpin + use<>>,
        BluetoothError,
    > {
        let device = self.adapter.device(address)?;
        let location = DeviceLocation {
            adapter_name: self.adapter.name().to_string(),
            address,
        };
        let snapshot = DeviceSnapshot::read(&device, location).await?;
        let device_events = device
            .events()
            .await?
            .map(move |device_event| AddressedDeviceEvent {
                address,
                device_event,
            });
        self.snapshots.insert(address, snapshot.clone());
        Ok(SubscribedDevice {
            snapshot,
            device_events,
        })
    }

    async fn react_to_adapter_event(
        &mut self,
        adapter_event: AdapterEvent,
    ) -> AdapterReaction<impl Stream<Item = AddressedDeviceEvent> + Unpin + use<>> {
        match adapter_event {
            AdapterEvent::DeviceAdded(address) if !self.snapshots.contains_key(&address) => {
                match self.add_device(address).await {
                    Ok(subscribed_device) => AdapterReaction::Subscribe(subscribed_device),
                    Err(_unreadable_device) => AdapterReaction::Nothing,
                }
            }
            AdapterEvent::DeviceRemoved(address) if self.snapshots.remove(&address).is_some() => {
                AdapterReaction::Report(DeviceChange::Disappeared(address))
            }
            _ => AdapterReaction::Nothing,
        }
    }

    fn apply_device_event(&mut self, addressed_event: AddressedDeviceEvent) -> DeviceEventOutcome {
        let DeviceEvent::PropertyChanged(changed_property) = addressed_event.device_event;
        let Some(snapshot) = self.snapshots.get_mut(&addressed_event.address) else {
            return DeviceEventOutcome::Unchanged;
        };
        match snapshot.apply_property(changed_property) {
            PropertyRelevance::Ignored => DeviceEventOutcome::Unchanged,
            PropertyRelevance::Displayed => DeviceEventOutcome::Changed(snapshot.clone()),
            PropertyRelevance::ConnectionChanged => {
                snapshot.refresh_kernel_input();
                if snapshot.connected {
                    DeviceEventOutcome::ConnectedAwaitingKernelInput(snapshot.clone())
                } else {
                    DeviceEventOutcome::Changed(snapshot.clone())
                }
            }
        }
    }

    fn settle_kernel_input(&mut self, address: bluer::Address) -> Option<DeviceChange> {
        let snapshot = self.snapshots.get_mut(&address)?;
        let previous_binding = snapshot.kernel_input.clone();
        snapshot.refresh_kernel_input();
        (snapshot.kernel_input != previous_binding).then(|| DeviceChange::Updated(snapshot.clone()))
    }
}

pub async fn device_changes(
    session: &BluezDbusSession,
    adapter_name: &str,
) -> Result<impl Stream<Item = DeviceChange> + Send + 'static, BluetoothError> {
    let adapter = session.adapter(adapter_name)?;
    let known_device_events = futures::stream::iter(
        adapter
            .device_addresses()
            .await?
            .into_iter()
            .map(AdapterEvent::DeviceAdded),
    );
    let adapter_events = known_device_events.chain(adapter.events().await?);

    Ok(async_stream::stream! {
        let mut device_snapshots = DeviceSnapshots { adapter, snapshots: HashMap::new() };
        let mut device_events = SelectAll::new();
        let mut kernel_refresh_timers = FuturesUnordered::new();
        let mut adapter_events = std::pin::pin!(adapter_events);
        loop {
            let wakeup = tokio::select! {
                Some(adapter_event) = adapter_events.next() => WatcherWakeup::Adapter(adapter_event),
                Some(addressed_event) = device_events.next() => WatcherWakeup::Device(addressed_event),
                Some(settled_address) = kernel_refresh_timers.next() => WatcherWakeup::KernelInputSettled(settled_address),
                else => WatcherWakeup::Finished,
            };
            let device_change = match wakeup {
                WatcherWakeup::Adapter(adapter_event) => match device_snapshots.react_to_adapter_event(adapter_event).await {
                    AdapterReaction::Subscribe(subscribed_device) => {
                        device_events.push(subscribed_device.device_events);
                        Some(DeviceChange::Appeared(subscribed_device.snapshot))
                    }
                    AdapterReaction::Report(device_change) => Some(device_change),
                    AdapterReaction::Nothing => None,
                },
                WatcherWakeup::Device(addressed_event) => match device_snapshots.apply_device_event(addressed_event) {
                    DeviceEventOutcome::Unchanged => None,
                    DeviceEventOutcome::Changed(snapshot) => Some(DeviceChange::Updated(snapshot)),
                    DeviceEventOutcome::ConnectedAwaitingKernelInput(snapshot) => {
                        kernel_refresh_timers.push(kernel_input_settled(snapshot.address()));
                        Some(DeviceChange::Updated(snapshot))
                    }
                },
                WatcherWakeup::KernelInputSettled(settled_address) => device_snapshots.settle_kernel_input(settled_address),
                WatcherWakeup::Finished => break,
            };
            if let Some(device_change) = device_change {
                yield device_change;
            }
        }
    })
}
