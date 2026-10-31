use bindrune_bluez::{
    bluetooth_adapter::{
        adapter_snapshot_and_discovery::AdapterSnapshot,
        adapter_udev_hardware::AdapterHardwareDescription,
    },
    bluez_dbus_session::BluezDbusSession,
    bluez_operation_error::BluetoothError,
};
use relm4::{adw, adw::prelude::*, factory::FactoryVecDeque, gtk, prelude::*};

use crate::{
    background_application_lifecycle::gsettings_stored_preferences::GsettingsStoredPreferences,
    sidebar_navigation::{
        discovery_scan_toggle_row::{
            DiscoveryScanToggleRow, DiscoveryScanToggleRowInit, DiscoveryScanToggleRowInput,
        },
        sidebar_adapter_row::{SidebarAdapterRow, SidebarAdapterRowInit},
        sidebar_device_rows::{
            SidebarDeviceRows, SidebarDeviceRowsInput, SidebarDeviceRowsOutput,
            SidebarDeviceRowsSource,
        },
    },
};

pub struct SidebarNavigationPageInit {
    pub session: BluezDbusSession,
    pub preferences: GsettingsStoredPreferences,
    pub radios: Vec<AdapterHardwareDescription>,
    pub adapter: AdapterSnapshot,
    pub transfers_button: gtk::MenuButton,
}

pub struct SidebarNavigationPage {
    session: BluezDbusSession,
    preferences: GsettingsStoredPreferences,
    adapter: AdapterSnapshot,
    device_list: Controller<SidebarDeviceRows>,
    adapter_rows: FactoryVecDeque<SidebarAdapterRow>,
    scan_toggle_row: Controller<DiscoveryScanToggleRow>,
    device_list_slot: adw::Bin,
}

#[derive(Debug)]
pub enum SidebarNavigationPageInput {
    AdapterInstalled(AdapterSnapshot),
    AdapterChanged(AdapterSnapshot),
    RefreshDevices,
    ShowUnnamedDevices(bool),
    StartScan,
    DeviceListReported(SidebarDeviceRowsOutput),
    AdapterRowActivated(String),
    ScanFailed(BluetoothError),
    SettingsActivated,
}

#[derive(Debug)]
pub enum SidebarNavigationPageOutput {
    SettingsActivated,
    AdapterActivated(String),
    DeviceListReported(SidebarDeviceRowsOutput),
    ScanFailed(BluetoothError),
}

fn launch_device_list(
    session: &BluezDbusSession,
    adapter_name: &str,
    preferences: &GsettingsStoredPreferences,
    sender: &ComponentSender<SidebarNavigationPage>,
) -> Controller<SidebarDeviceRows> {
    SidebarDeviceRows::builder()
        .launch(SidebarDeviceRowsSource {
            session: session.clone(),
            adapter_name: adapter_name.to_string(),
            show_unnamed_devices: preferences.show_unnamed_devices(),
        })
        .forward(
            sender.input_sender(),
            SidebarNavigationPageInput::DeviceListReported,
        )
}

impl SidebarNavigationPage {
    fn relaunch_device_list(&mut self, sender: &ComponentSender<Self>) {
        self.device_list = launch_device_list(
            &self.session,
            &self.adapter.adapter_name,
            &self.preferences,
            sender,
        );
        self.device_list_slot
            .set_child(Some(self.device_list.widget()));
    }

    fn mark_active_adapter(&mut self) {
        let active_adapter_name = self.adapter.adapter_name.clone();
        self.adapter_rows
            .guard()
            .iter_mut()
            .for_each(|adapter_row| {
                adapter_row.active = adapter_row.radio.adapter_name == active_adapter_name;
            });
    }
}

#[relm4::component(pub)]
impl Component for SidebarNavigationPage {
    type Init = SidebarNavigationPageInit;
    type Input = SidebarNavigationPageInput;
    type Output = SidebarNavigationPageOutput;
    type CommandOutput = ();

    view! {
        adw::NavigationPage {
            set_title: "Overskride",

            #[wrap(Some)]
            set_child = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    pack_start: &transfers_button,
                },

                #[wrap(Some)]
                set_content = &gtk::ScrolledWindow {
                    set_vexpand: true,

                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 24,
                        set_margin_all: 12,

                        adw::PreferencesGroup {
                            set_title: "Settings",

                            adw::ActionRow {
                                set_title: "Settings",
                                set_subtitle: "Received files and device list",
                                set_activatable: true,
                                add_prefix = &gtk::Image {
                                    set_icon_name: Some("emblem-system-symbolic"),
                                },
                                add_suffix = &gtk::Image {
                                    set_icon_name: Some("go-next-symbolic"),
                                },
                                connect_activated => SidebarNavigationPageInput::SettingsActivated,
                            },
                        },

                        #[local_ref]
                        adapter_group -> adw::PreferencesGroup {
                            set_title: "Adapters",
                            set_description: Some("Bonds and trust are stored per adapter"),
                        },

                        adw::PreferencesGroup {
                            set_title: "Devices",
                            #[watch]
                            set_description: Some(&format!("Known to {}, plus devices found by scanning.", model.adapter.radio_label)),

                            #[local_ref]
                            scan_toggle_row_widget -> adw::ActionRow {},

                            #[local_ref]
                            device_list_slot -> adw::Bin {},
                        },
                    },
                },
            },
        }
    }

    fn init(
        page_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let SidebarNavigationPageInit {
            session,
            preferences,
            radios,
            adapter,
            transfers_button,
        } = page_init;
        let device_list =
            launch_device_list(&session, &adapter.adapter_name, &preferences, &sender);
        let mut adapter_rows = FactoryVecDeque::builder()
            .launch(adw::PreferencesGroup::default())
            .forward(
                sender.input_sender(),
                SidebarNavigationPageInput::AdapterRowActivated,
            );
        radios.iter().for_each(|radio| {
            adapter_rows.guard().push_back(SidebarAdapterRowInit {
                radio: radio.clone(),
                active: radio.adapter_name == adapter.adapter_name,
            });
        });
        let scan_toggle_row = DiscoveryScanToggleRow::builder()
            .launch(DiscoveryScanToggleRowInit {
                session: session.clone(),
                adapter_name: adapter.adapter_name.clone(),
            })
            .forward(
                sender.input_sender(),
                SidebarNavigationPageInput::ScanFailed,
            );
        let device_list_slot = adw::Bin::new();
        device_list_slot.set_child(Some(device_list.widget()));

        let model = Self {
            session,
            preferences,
            adapter,
            device_list,
            adapter_rows,
            scan_toggle_row,
            device_list_slot: device_list_slot.clone(),
        };
        let device_list_slot = &device_list_slot;
        let adapter_group = model.adapter_rows.widget();
        let scan_toggle_row_widget = model.scan_toggle_row.widget();
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        let page_output = match input {
            SidebarNavigationPageInput::AdapterInstalled(adapter) => {
                self.scan_toggle_row
                    .emit(DiscoveryScanToggleRowInput::AdapterSwitched(
                        adapter.adapter_name.clone(),
                    ));
                self.adapter = adapter;
                self.relaunch_device_list(&sender);
                self.mark_active_adapter();
                return;
            }
            SidebarNavigationPageInput::AdapterChanged(adapter) => {
                let powered_on = adapter.powered && !self.adapter.powered;
                if !adapter.powered {
                    self.scan_toggle_row
                        .emit(DiscoveryScanToggleRowInput::StopScan);
                }
                self.adapter = adapter;
                if powered_on {
                    self.relaunch_device_list(&sender);
                }
                return;
            }
            SidebarNavigationPageInput::RefreshDevices => {
                self.relaunch_device_list(&sender);
                return;
            }
            SidebarNavigationPageInput::ShowUnnamedDevices(show_unnamed_devices) => {
                self.device_list
                    .emit(SidebarDeviceRowsInput::ShowUnnamedDevices(
                        show_unnamed_devices,
                    ));
                return;
            }
            SidebarNavigationPageInput::StartScan => {
                self.scan_toggle_row
                    .emit(DiscoveryScanToggleRowInput::StartScan);
                return;
            }
            SidebarNavigationPageInput::DeviceListReported(device_list_output) => {
                SidebarNavigationPageOutput::DeviceListReported(device_list_output)
            }
            SidebarNavigationPageInput::AdapterRowActivated(adapter_name) => {
                SidebarNavigationPageOutput::AdapterActivated(adapter_name)
            }
            SidebarNavigationPageInput::ScanFailed(scan_error) => {
                SidebarNavigationPageOutput::ScanFailed(scan_error)
            }
            SidebarNavigationPageInput::SettingsActivated => {
                SidebarNavigationPageOutput::SettingsActivated
            }
        };
        let _main_view_gone = sender.output(page_output);
    }
}
