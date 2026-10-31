use ksni::menu::{CheckmarkItem, StandardItem};

use crate::app_preferences::APPLICATION_ID;
use crate::app_window::AppWindowInput;

const MOST_TRAY_DEVICES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayDevice {
    pub address: overskride_bluetooth::Address,
    pub display_name: String,
    pub connected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrayContent {
    pub powered: bool,
    pub visible_for_pairing: bool,
    pub paired_devices: Vec<TrayDevice>,
}

#[derive(Debug)]
pub enum TrayRequest {
    OpenWindow,
    Quit,
    SetPowered(bool),
    SetVisibleForPairing(bool),
    SetDeviceConnected {
        address: overskride_bluetooth::Address,
        connected: bool,
    },
    ShowSettings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayScope {
    WindowOnly,
    Bluetooth,
}

pub struct OverskrideTray {
    pub window_sender: relm4::Sender<AppWindowInput>,
    pub scope: TrayScope,
    pub content: TrayContent,
}

impl OverskrideTray {
    fn request(&self, tray_request: TrayRequest) {
        self.window_sender.emit(AppWindowInput::Tray(tray_request));
    }

    fn bluetooth_items(&self) -> Vec<ksni::MenuItem<Self>> {
        let powered = self.content.powered;
        let mut bluetooth_items = vec![
            CheckmarkItem {
                label: "Bluetooth".to_string(),
                checked: powered,
                activate: Box::new(move |tray: &mut Self| {
                    tray.request(TrayRequest::SetPowered(!powered));
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Visible for Pairing".to_string(),
                checked: self.content.visible_for_pairing,
                enabled: powered,
                activate: Box::new(|tray: &mut Self| {
                    let visible_for_pairing = !tray.content.visible_for_pairing;
                    tray.request(TrayRequest::SetVisibleForPairing(visible_for_pairing));
                }),
                ..Default::default()
            }
            .into(),
        ];
        if powered && !self.content.paired_devices.is_empty() {
            bluetooth_items.push(ksni::MenuItem::Separator);
            bluetooth_items.push(
                StandardItem {
                    label: "Paired Devices".to_string(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
            bluetooth_items.extend(
                self.content
                    .paired_devices
                    .iter()
                    .take(MOST_TRAY_DEVICES)
                    .map(device_item),
            );
        }
        bluetooth_items
    }
}

fn device_item(tray_device: &TrayDevice) -> ksni::MenuItem<OverskrideTray> {
    let address = tray_device.address;
    let connected = tray_device.connected;
    CheckmarkItem {
        label: tray_device.display_name.clone(),
        checked: connected,
        activate: Box::new(move |tray: &mut OverskrideTray| {
            tray.request(TrayRequest::SetDeviceConnected {
                address,
                connected: !connected,
            });
        }),
        ..Default::default()
    }
    .into()
}

impl ksni::Tray for OverskrideTray {
    fn id(&self) -> String {
        APPLICATION_ID.to_string()
    }

    fn icon_name(&self) -> String {
        APPLICATION_ID.to_string()
    }

    fn title(&self) -> String {
        "Overskride".to_string()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let connected_count = self
            .content
            .paired_devices
            .iter()
            .filter(|tray_device| tray_device.connected)
            .count();
        let description = match (self.content.powered, connected_count) {
            (false, _) => "Bluetooth is off".to_string(),
            (true, 0) => "No devices connected".to_string(),
            (true, 1) => "1 device connected".to_string(),
            (true, connected_count) => format!("{connected_count} devices connected"),
        };
        ksni::ToolTip {
            title: "Overskride".to_string(),
            description,
            ..Default::default()
        }
    }

    fn activate(&mut self, _horizontal_position: i32, _vertical_position: i32) {
        self.request(TrayRequest::OpenWindow);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let mut menu_items = vec![
            StandardItem {
                label: "Open Overskride".to_string(),
                activate: Box::new(|tray: &mut Self| tray.request(TrayRequest::OpenWindow)),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
        ];
        if self.scope == TrayScope::Bluetooth {
            menu_items.extend(self.bluetooth_items());
            menu_items.push(ksni::MenuItem::Separator);
            menu_items.push(
                StandardItem {
                    label: "Settings".to_string(),
                    icon_name: "emblem-system-symbolic".to_string(),
                    activate: Box::new(|tray: &mut Self| tray.request(TrayRequest::ShowSettings)),
                    ..Default::default()
                }
                .into(),
            );
        }
        menu_items.push(
            StandardItem {
                label: "Quit".to_string(),
                icon_name: "application-exit".to_string(),
                activate: Box::new(|tray: &mut Self| tray.request(TrayRequest::Quit)),
                ..Default::default()
            }
            .into(),
        );
        menu_items
    }
}
