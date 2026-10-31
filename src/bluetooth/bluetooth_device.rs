use std::cell::{Cell, RefCell};
use std::str::FromStr;

use gtk::glib;
use gtk::glib::Properties;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use crate::pairing_stages::{read_pairing_stages, PairingStages, PAIRING_STEP_DEFINITIONS};

pub const UNKNOWN_DEVICE_NAME: &str = "Unknown Device";

#[derive(Clone, Debug)]
pub struct DeviceSnapshot {
    pub address: bluer::Address,
    pub adapter_name: String,
    pub adapter_address: bluer::Address,
    pub alias: String,
    pub rssi: i32,
    pub connected: bool,
    pub pairing_stages: PairingStages,
}

impl DeviceSnapshot {
    pub async fn read_from_device(device: &bluer::Device, adapter: &bluer::Adapter) -> bluer::Result<Self> {
        Ok(Self {
            address: device.address(),
            adapter_name: adapter.name().to_string(),
            adapter_address: adapter.address().await?,
            alias: device.alias().await?,
            rssi: device.rssi().await?.map(i32::from).unwrap_or(0),
            connected: device.is_connected().await?,
            pairing_stages: read_pairing_stages(device).await?,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct PairingStatusSummary {
    icon_name: String,
    css_classes: Vec<String>,
    tooltip: String,
    visible: bool,
}

impl PairingStatusSummary {
    fn from_pairing_stages(stages: &PairingStages) -> Self {
        let input_device_missing = stages.connected && stages.is_input_device && stages.kernel_input_devices.is_none();
        let (icon_name, style_class, status_summary) = if stages.blocked {
            ("action-unavailable-symbolic", "error", "Blocked\nEvery connection is rejected")
        } else if stages.paired && !stages.bonded {
            ("dialog-warning-symbolic", "warning", "Bond missing\nTrust can't be saved")
        } else if input_device_missing {
            ("dialog-warning-symbolic", "warning", "Connected, but no input device appeared")
        } else if stages.connected {
            ("bluetooth-active-symbolic", "success", "Connected")
        } else if stages.bonded && stages.trusted {
            ("bluetooth-disconnected-symbolic", "dim-label", "Ready\nReconnects when turned on")
        } else {
            ("changes-prevent-symbolic", "dim-label", "Bonded\nEach connection needs approval")
        };

        let step_overview = PAIRING_STEP_DEFINITIONS
            .iter()
            .filter(|definition| stages.is_step_applicable(definition.step))
            .map(|definition| {
                let step_state = if stages.is_step_done(definition.step) { "done" } else { "not yet" };
                format!("{}: {}", definition.title, step_state)
            })
            .collect::<Vec<String>>()
            .join("\n");

        Self {
            icon_name: icon_name.to_string(),
            css_classes: vec![style_class.to_string()],
            tooltip: format!("{}\n\n{}", status_summary, step_overview),
            visible: stages.paired,
        }
    }
}

fn alias_looks_like_address(alias: &str) -> bool {
    bluer::Address::from_str(&alias.replace('-', ":")).is_ok()
}

fn rssi_icon_name_for_strength(rssi: i32) -> &'static str {
    match rssi {
        0 => "rssi-none-symbolic",
        strength if -strength <= 60 => "rssi-high-symbolic",
        strength if -strength <= 70 => "rssi-medium-symbolic",
        strength if -strength <= 80 => "rssi-low-symbolic",
        strength if -strength <= 90 => "rssi-dead-symbolic",
        strength if -strength <= 100 => "rssi-none-symbolic",
        _ => "rssi-not-found-symbolic",
    }
}

mod imp {
    use super::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::BluetoothDevice)]
    pub struct BluetoothDevice {
        pub bluer_address: Cell<bluer::Address>,
        pub adapter_address: Cell<bluer::Address>,

        #[property(get, set)]
        adapter_name: RefCell<String>,

        #[property(get, set = Self::set_alias_and_display_name)]
        #[property(name = "display-name", get = Self::display_name, type = String)]
        alias: RefCell<String>,

        #[property(get, set = Self::set_rssi_and_icon_name)]
        #[property(name = "rssi-icon-name", get = Self::rssi_icon_name, type = String)]
        rssi: Cell<i32>,

        #[property(get, set)]
        connected: Cell<bool>,

        #[property(get, set)]
        trusted: Cell<bool>,

        #[property(name = "pairing-status-icon-name", get, type = String, member = icon_name)]
        #[property(name = "pairing-status-css-classes", get, type = Vec<String>, member = css_classes)]
        #[property(name = "pairing-status-tooltip", get, type = String, member = tooltip)]
        #[property(name = "pairing-status-visible", get, type = bool, member = visible)]
        pub pairing_status: RefCell<PairingStatusSummary>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BluetoothDevice {
        const NAME: &'static str = "BluetoothDevice";
        type Type = super::BluetoothDevice;
    }

    #[glib::derived_properties]
    impl ObjectImpl for BluetoothDevice {}

    impl BluetoothDevice {
        fn set_alias_and_display_name(&self, alias: String) {
            self.alias.replace(alias);
            self.obj().notify_display_name();
        }

        fn display_name(&self) -> String {
            let alias = self.alias.borrow();
            if alias_looks_like_address(&alias) {
                UNKNOWN_DEVICE_NAME.to_string()
            } else {
                alias.clone()
            }
        }

        fn set_rssi_and_icon_name(&self, rssi: i32) {
            self.rssi.set(rssi);
            self.obj().notify_rssi_icon_name();
        }

        fn rssi_icon_name(&self) -> String {
            rssi_icon_name_for_strength(self.rssi.get()).to_string()
        }
    }
}

glib::wrapper! {
    pub struct BluetoothDevice(ObjectSubclass<imp::BluetoothDevice>);
}

impl BluetoothDevice {
    pub fn from_snapshot(device_snapshot: &DeviceSnapshot) -> Self {
        let bluetooth_device: Self = glib::Object::new();
        bluetooth_device.imp().bluer_address.set(device_snapshot.address);
        bluetooth_device.apply_snapshot(device_snapshot);
        bluetooth_device
    }

    pub fn apply_snapshot(&self, device_snapshot: &DeviceSnapshot) {
        self.imp().adapter_address.set(device_snapshot.adapter_address);
        self.set_adapter_name(device_snapshot.adapter_name.clone());
        self.set_alias(device_snapshot.alias.clone());
        self.set_rssi(device_snapshot.rssi);
        self.set_connected(device_snapshot.connected);
        self.apply_pairing_stages(&device_snapshot.pairing_stages);
    }

    pub fn apply_pairing_stages(&self, stages: &PairingStages) {
        self.set_trusted(stages.trusted);
        self.imp().pairing_status.replace(PairingStatusSummary::from_pairing_stages(stages));
        self.notify_pairing_status_icon_name();
        self.notify_pairing_status_css_classes();
        self.notify_pairing_status_tooltip();
        self.notify_pairing_status_visible();
    }

    pub fn bluer_address(&self) -> bluer::Address {
        self.imp().bluer_address.get()
    }

    pub fn bluer_adapter_address(&self) -> bluer::Address {
        self.imp().adapter_address.get()
    }

    pub fn is_unknown(&self) -> bool {
        alias_looks_like_address(&self.alias())
    }
}
