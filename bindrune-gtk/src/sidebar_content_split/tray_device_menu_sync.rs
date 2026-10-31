use std::collections::BTreeMap;

use bindrune_bluez::{
    bluetooth_adapter::adapter_snapshot_and_discovery::AdapterSnapshot,
    remote_device::remote_device_change_stream::DeviceChange,
};

use crate::background_application_lifecycle::system_tray_menu::{TrayContent, TrayDevice};

#[derive(Default)]
pub struct TrayDeviceMenuSync {
    paired_tray_devices: BTreeMap<bindrune_bluez::Address, TrayDevice>,
}

impl TrayDeviceMenuSync {
    pub fn forget_all_devices(&mut self) {
        self.paired_tray_devices.clear();
    }

    pub fn remember_device_change(&mut self, device_change: &DeviceChange) {
        match device_change {
            DeviceChange::Appeared(snapshot) | DeviceChange::Updated(snapshot)
                if snapshot.paired =>
            {
                self.paired_tray_devices.insert(
                    snapshot.address(),
                    TrayDevice {
                        address: snapshot.address(),
                        display_name: snapshot.display_name().to_string(),
                        connected: snapshot.connected,
                    },
                );
            }
            DeviceChange::Appeared(snapshot) | DeviceChange::Updated(snapshot) => {
                self.paired_tray_devices.remove(&snapshot.address());
            }
            DeviceChange::Disappeared(address) => {
                self.paired_tray_devices.remove(address);
            }
        }
    }

    pub fn tray_content(&self, adapter: &AdapterSnapshot) -> TrayContent {
        let mut paired_devices: Vec<TrayDevice> =
            self.paired_tray_devices.values().cloned().collect();
        paired_devices.sort_by(|first_device, second_device| {
            second_device
                .connected
                .cmp(&first_device.connected)
                .then_with(|| first_device.display_name.cmp(&second_device.display_name))
        });
        TrayContent {
            powered: adapter.powered,
            visible_for_pairing: adapter.discoverable,
            paired_devices,
        }
    }
}
