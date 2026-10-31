use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt;
use overskride_bluetooth::adapter_state::{AdapterSnapshot, read_adapter};
use overskride_bluetooth::bluetooth_error::BluetoothError;
use overskride_bluetooth::device_actions::read_device;
use overskride_bluetooth::device_changes::DeviceChange;
use overskride_bluetooth::device_snapshot::DeviceSnapshot;
use overskride_bluetooth::discovery::{DiscoverySession, start_discovery};
use overskride_bluetooth::pairing_agent::{AgentPolicy, PairingRequest, pairing_requests};
use overskride_bluetooth::radios::RadioDescription;
use overskride_bluetooth::session::{BluetoothSession, DeviceLocation};
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::prelude::*;

use crate::adapter_settings::{AdapterSettings, AdapterSettingsInit, AdapterSettingsOutput};
use crate::app_preferences::AppPreferences;
use crate::device_list::{DeviceList, DeviceListInput, DeviceListOutput, DeviceListSource};
use crate::device_page::{
    DevicePage, DevicePageInit, DevicePageInput, DevicePageOutput, ObexAvailability,
};
use crate::pairing_prompts::{RequesterDescription, present_pairing_request};
use crate::transfers_popover::{TransfersInit, TransfersInput, TransfersOutput, TransfersPopover};

const SCAN_DURATION: Duration = Duration::from_secs(60);
const SETTINGS_TITLE: &str = "Settings";

pub struct MainViewInit {
    pub session: BluetoothSession,
    pub obex: ObexAvailability,
    pub preferences: AppPreferences,
    pub radios: Vec<RadioDescription>,
    pub adapter: AdapterSnapshot,
}

enum ContentPage {
    BluetoothSettings,
    Device(Controller<DevicePage>),
}

enum ScanState {
    Idle,
    Scanning {
        generation: u64,
        _discovery: DiscoverySession,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AfterAdapterSwitch {
    ShowSettings,
    OpenDevice(overskride_bluetooth::Address),
    StartScan,
}

pub struct MainView {
    session: BluetoothSession,
    obex: ObexAvailability,
    preferences: AppPreferences,
    radios: Vec<RadioDescription>,
    adapter: AdapterSnapshot,
    device_names: HashMap<overskride_bluetooth::Address, String>,
    scan_state: ScanState,
    scan_generation: u64,
    content_page: ContentPage,
    content_title: String,
    show_content: bool,
    device_list: Controller<DeviceList>,
    adapter_settings: Controller<AdapterSettings>,
    transfers: Controller<TransfersPopover>,
    device_list_slot: adw::Bin,
    content_slot: adw::Bin,
}

#[derive(Debug)]
pub enum MainViewInput {
    ScanToggled(bool),
    RadioSelected(u32),
    ShowSettings,
    BackToSidebar,
    RefreshDevices,
    HideUnknownDevices(bool),
    DeviceList(DeviceListOutput),
    DevicePage(DevicePageOutput),
    AdapterSettings(AdapterSettingsOutput),
    Transfers(TransfersOutput),
}

#[derive(Debug)]
pub enum MainViewCommand {
    PairingRequest(PairingRequest),
    ScanStarted {
        generation: u64,
        outcome: Result<DiscoverySession, BluetoothError>,
    },
    ScanExpired(u64),
    AdapterLoaded {
        outcome: Result<AdapterSnapshot, BluetoothError>,
        after_switch: AfterAdapterSwitch,
    },
    DeviceRead(Result<DeviceSnapshot, BluetoothError>),
}

fn radio_choice_list(radios: &[RadioDescription]) -> gtk::StringList {
    let radio_labels: Vec<&str> = radios
        .iter()
        .map(|radio| radio.radio_label.as_str())
        .collect();
    gtk::StringList::new(&radio_labels)
}

fn launch_device_list(
    session: &BluetoothSession,
    adapter_name: &str,
    preferences: &AppPreferences,
    sender: &ComponentSender<MainView>,
) -> Controller<DeviceList> {
    DeviceList::builder()
        .launch(DeviceListSource {
            session: session.clone(),
            adapter_name: adapter_name.to_string(),
            hide_unknown_devices: preferences.hide_unknown_devices(),
        })
        .forward(sender.input_sender(), MainViewInput::DeviceList)
}

fn launch_adapter_settings(
    session: &BluetoothSession,
    adapter: &AdapterSnapshot,
    preferences: &AppPreferences,
    sender: &ComponentSender<MainView>,
) -> Controller<AdapterSettings> {
    AdapterSettings::builder()
        .launch(AdapterSettingsInit {
            session: session.clone(),
            adapter: adapter.clone(),
            preferences: preferences.clone(),
        })
        .forward(sender.input_sender(), MainViewInput::AdapterSettings)
}

impl MainView {
    fn is_scanning(&self) -> bool {
        matches!(self.scan_state, ScanState::Scanning { .. })
    }

    fn selected_radio_position(&self) -> u32 {
        self.radios
            .iter()
            .position(|radio| radio.adapter_name == self.adapter.adapter_name)
            .and_then(|radio_position| u32::try_from(radio_position).ok())
            .unwrap_or_default()
    }

    fn radio_label(&self, adapter_name: &str) -> String {
        self.radios
            .iter()
            .find(|radio| radio.adapter_name == adapter_name)
            .map_or_else(
                || adapter_name.to_string(),
                |radio| radio.radio_label.clone(),
            )
    }

    fn show_settings(&mut self) {
        self.content_page = ContentPage::BluetoothSettings;
        self.content_slot
            .set_child(Some(&**self.adapter_settings.widget()));
        self.content_title = SETTINGS_TITLE.to_string();
    }

    fn open_device(&mut self, snapshot: DeviceSnapshot, sender: &ComponentSender<Self>) {
        self.content_title = snapshot.display_name().to_string();
        let device_page = DevicePage::builder()
            .launch(DevicePageInit {
                session: self.session.clone(),
                obex: self.obex.clone(),
                adapter_address: self.adapter.address,
                snapshot,
            })
            .forward(sender.input_sender(), MainViewInput::DevicePage);
        self.content_slot.set_child(Some(&**device_page.widget()));
        self.content_page = ContentPage::Device(device_page);
        self.show_content = true;
    }

    fn start_scan(&mut self, sender: &ComponentSender<Self>) {
        if self.is_scanning() {
            return;
        }
        self.scan_generation += 1;
        let generation = self.scan_generation;
        let session = self.session.clone();
        let adapter_name = self.adapter.adapter_name.clone();
        sender.oneshot_command(async move {
            MainViewCommand::ScanStarted {
                generation,
                outcome: start_discovery(&session, &adapter_name).await,
            }
        });
    }

    fn switch_adapter(
        &self,
        adapter_name: String,
        after_switch: AfterAdapterSwitch,
        sender: &ComponentSender<Self>,
    ) {
        let session = self.session.clone();
        sender.oneshot_command(async move {
            MainViewCommand::AdapterLoaded {
                outcome: read_adapter(&session, &adapter_name).await,
                after_switch,
            }
        });
    }

    fn install_adapter(&mut self, adapter: AdapterSnapshot, sender: &ComponentSender<Self>) {
        self.scan_state = ScanState::Idle;
        self.preferences
            .remember_adapter_name(&adapter.adapter_name);
        self.device_list = launch_device_list(
            &self.session,
            &adapter.adapter_name,
            &self.preferences,
            sender,
        );
        self.adapter_settings =
            launch_adapter_settings(&self.session, &adapter, &self.preferences, sender);
        self.device_list_slot
            .set_child(Some(self.device_list.widget()));
        self.transfers.emit(TransfersInput::AdapterSwitched(
            adapter.adapter_name.clone(),
        ));
        self.device_names.clear();
        self.adapter = adapter;
        self.show_settings();
    }

    fn handle_device_list_output(
        &mut self,
        device_list_output: DeviceListOutput,
        root: &adw::ToastOverlay,
        sender: &ComponentSender<Self>,
    ) {
        match device_list_output {
            DeviceListOutput::Opened(snapshot) => self.open_device(snapshot, sender),
            DeviceListOutput::Changed(device_change) => self.forward_device_change(device_change),
            DeviceListOutput::Unavailable(subscription_error) => {
                root.add_toast(adw::Toast::new(&subscription_error.to_string()));
            }
        }
    }

    fn shown_device_address(&self) -> Option<overskride_bluetooth::Address> {
        match &self.content_page {
            ContentPage::Device(device_page) => Some(device_page.model().shown_address()),
            ContentPage::BluetoothSettings => None,
        }
    }

    fn forward_device_change(&mut self, device_change: DeviceChange) {
        let shown_address = self.shown_device_address();
        match device_change {
            DeviceChange::Appeared(snapshot) | DeviceChange::Updated(snapshot) => {
                self.device_names
                    .insert(snapshot.address(), snapshot.display_name().to_string());
                if shown_address != Some(snapshot.address()) {
                    return;
                }
                self.content_title = snapshot.display_name().to_string();
                if let ContentPage::Device(device_page) = &self.content_page {
                    device_page.emit(DevicePageInput::DeviceChanged(snapshot));
                }
            }
            DeviceChange::Disappeared(address) => {
                self.device_names.remove(&address);
                if shown_address == Some(address) {
                    self.show_settings();
                }
            }
        }
    }

    fn handle_device_page_output(
        &mut self,
        device_page_output: DevicePageOutput,
        root: &adw::ToastOverlay,
        sender: &ComponentSender<Self>,
    ) {
        match device_page_output {
            DevicePageOutput::Failed(failure) => {
                root.add_toast(adw::Toast::new(&failure.to_string()));
            }
            DevicePageOutput::Forgotten { hint, rescan } => {
                root.add_toast(adw::Toast::new(&hint));
                self.show_settings();
                if rescan {
                    self.start_scan(sender);
                }
            }
            DevicePageOutput::ShowRadio(adapter_name) => {
                if let Some(shown_address) = self.shown_device_address() {
                    self.switch_adapter(
                        adapter_name,
                        AfterAdapterSwitch::OpenDevice(shown_address),
                        sender,
                    );
                }
            }
            DevicePageOutput::PairViaRadio(adapter_name) => {
                self.switch_adapter(adapter_name, AfterAdapterSwitch::StartScan, sender);
            }
            DevicePageOutput::TransfersStarted(transfer_handles) => {
                self.transfers
                    .emit(TransfersInput::TransfersStarted(transfer_handles));
            }
        }
    }
}

#[relm4::component(pub)]
impl Component for MainView {
    type Init = MainViewInit;
    type Input = MainViewInput;
    type Output = ();
    type CommandOutput = MainViewCommand;

    view! {
        adw::ToastOverlay {
            #[name = "split_view"]
            adw::NavigationSplitView {
                set_min_sidebar_width: 300.0,
                set_max_sidebar_width: 360.0,
                #[watch]
                set_show_content: model.show_content,

                #[wrap(Some)]
                set_sidebar = &adw::NavigationPage {
                    set_title: "Overskride",

                    #[wrap(Some)]
                    set_child = &adw::ToolbarView {
                        add_top_bar = &adw::HeaderBar {
                            pack_start: model.transfers.widget(),

                            pack_end = &gtk::ToggleButton {
                                set_tooltip_text: Some("Search for new devices for 60 seconds"),
                                add_css_class: "flat",
                                #[watch]
                                #[block_signal(scan_handler)]
                                set_active: model.is_scanning(),
                                connect_toggled[sender] => move |scan_button| {
                                    sender.input(MainViewInput::ScanToggled(scan_button.is_active()));
                                } @scan_handler,

                                gtk::Box {
                                    set_spacing: 6,
                                    gtk::Spinner {
                                        #[watch]
                                        set_spinning: model.is_scanning(),
                                    },
                                    gtk::Label {
                                        set_label: "Scan",
                                    },
                                },
                            },
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
                                        set_title: "Bluetooth Settings",
                                        set_activatable: true,
                                        add_suffix = &gtk::Image {
                                            set_icon_name: Some("go-next-symbolic"),
                                        },
                                        connect_activated => MainViewInput::ShowSettings,
                                    },
                                },

                                adw::PreferencesGroup {
                                    set_title: "Radio",
                                    set_description: Some("Bonds and trust are stored per radio"),
                                    #[watch]
                                    set_visible: model.radios.len() > 1,

                                    adw::ComboRow {
                                        set_title: "Active Radio",
                                        set_model: Some(&radio_choice_list(&model.radios)),
                                        #[watch]
                                        #[block_signal(radio_handler)]
                                        set_selected: model.selected_radio_position(),
                                        connect_selected_notify[sender] => move |combo_row| {
                                            sender.input(MainViewInput::RadioSelected(combo_row.selected()));
                                        } @radio_handler,
                                    },
                                },

                                adw::PreferencesGroup {
                                    set_title: "Devices",
                                    set_description: Some("Known to this radio. Press Scan to find new ones."),

                                    #[local_ref]
                                    device_list_slot -> adw::Bin {},
                                },
                            },
                        },
                    },
                },

                #[wrap(Some)]
                set_content = &adw::NavigationPage {
                    #[watch]
                    set_title: &model.content_title,
                    connect_hidden => MainViewInput::BackToSidebar,

                    #[wrap(Some)]
                    set_child = &adw::ToolbarView {
                        add_top_bar = &adw::HeaderBar {
                            pack_end = &gtk::MenuButton {
                                set_icon_name: "open-menu-symbolic",
                                set_tooltip_text: Some("Main Menu"),
                                set_menu_model: Some(&main_menu),
                            },
                        },

                        #[local_ref]
                        content_slot -> adw::Bin {},
                    },
                },
            },
        }
    }

    menu! {
        main_menu: {
            section! {
                "_Refresh Devices" => RefreshDevicesAction,
            },
            section! {
                "_About Overskride" => AboutAction,
            },
        }
    }

    fn init(
        view_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let MainViewInit {
            session,
            obex,
            preferences,
            radios,
            adapter,
        } = view_init;
        let device_list =
            launch_device_list(&session, &adapter.adapter_name, &preferences, &sender);
        let adapter_settings = launch_adapter_settings(&session, &adapter, &preferences, &sender);
        let transfers = TransfersPopover::builder()
            .launch(TransfersInit {
                session: session.clone(),
                obex: obex.clone(),
                preferences: preferences.clone(),
                adapter_name: adapter.adapter_name.clone(),
            })
            .forward(sender.input_sender(), MainViewInput::Transfers);
        let device_list_slot = adw::Bin::new();
        let content_slot = adw::Bin::new();
        device_list_slot.set_child(Some(device_list.widget()));
        content_slot.set_child(Some(&**adapter_settings.widget()));

        let hide_unknown_sender = sender.input_sender().clone();
        preferences.settings().connect_changed(
            Some("hide-unknown-devices"),
            move |changed_settings, setting_key| {
                hide_unknown_sender.emit(MainViewInput::HideUnknownDevices(
                    changed_settings.boolean(setting_key),
                ));
            },
        );

        let agent_session = session.clone();
        sender.command(move |command_output, shutdown| {
            shutdown
                .register(async move {
                    let agent_policy = AgentPolicy {
                        trust_after_pairing: true,
                    };
                    let Ok(requests) = pairing_requests(&agent_session, agent_policy).await else {
                        return;
                    };
                    requests
                        .for_each(|pairing_request| {
                            command_output.emit(MainViewCommand::PairingRequest(pairing_request));
                            futures::future::ready(())
                        })
                        .await;
                })
                .drop_on_shutdown()
        });

        let model = Self {
            session,
            obex,
            preferences,
            radios,
            adapter,
            device_names: HashMap::new(),
            scan_state: ScanState::Idle,
            scan_generation: 0,
            content_page: ContentPage::BluetoothSettings,
            content_title: SETTINGS_TITLE.to_string(),
            show_content: false,
            device_list,
            adapter_settings,
            transfers,
            device_list_slot: device_list_slot.clone(),
            content_slot: content_slot.clone(),
        };
        let device_list_slot = &device_list_slot;
        let content_slot = &content_slot;
        let widgets = view_output!();

        let refresh_sender = sender.input_sender().clone();
        let refresh_action: relm4::actions::RelmAction<RefreshDevicesAction> =
            relm4::actions::RelmAction::new_stateless(move |_refresh_action| {
                refresh_sender.emit(MainViewInput::RefreshDevices);
            });
        let about_action: relm4::actions::RelmAction<AboutAction> =
            relm4::actions::RelmAction::new_stateless(|_about_action| crate::about::show_about());
        let mut window_actions = relm4::actions::RelmActionGroup::<MainViewActionGroup>::new();
        window_actions.add_action(refresh_action);
        window_actions.add_action(about_action);
        window_actions.register_for_widget(&root);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match input {
            MainViewInput::ScanToggled(true) => self.start_scan(&sender),
            MainViewInput::ScanToggled(false) => self.scan_state = ScanState::Idle,
            MainViewInput::RadioSelected(radio_position) => {
                let Some(radio) = usize::try_from(radio_position)
                    .ok()
                    .and_then(|position| self.radios.get(position))
                else {
                    return;
                };
                if radio.adapter_name != self.adapter.adapter_name {
                    self.switch_adapter(
                        radio.adapter_name.clone(),
                        AfterAdapterSwitch::ShowSettings,
                        &sender,
                    );
                }
            }
            MainViewInput::ShowSettings => {
                self.show_settings();
                self.show_content = true;
            }
            MainViewInput::BackToSidebar => self.show_content = false,
            MainViewInput::RefreshDevices => {
                self.device_list = launch_device_list(
                    &self.session,
                    &self.adapter.adapter_name,
                    &self.preferences,
                    &sender,
                );
                self.device_list_slot
                    .set_child(Some(self.device_list.widget()));
            }
            MainViewInput::HideUnknownDevices(hide_unknown_devices) => {
                self.device_list
                    .emit(DeviceListInput::HideUnknownDevices(hide_unknown_devices));
            }
            MainViewInput::DeviceList(device_list_output) => {
                self.handle_device_list_output(device_list_output, root, &sender);
            }
            MainViewInput::DevicePage(device_page_output) => {
                self.handle_device_page_output(device_page_output, root, &sender);
            }
            MainViewInput::AdapterSettings(AdapterSettingsOutput::Failed(failure)) => {
                root.add_toast(adw::Toast::new(&failure.to_string()));
            }
            MainViewInput::Transfers(TransfersOutput::ReceivingFile { file_name }) => {
                root.add_toast(adw::Toast::new(&format!("Receiving {file_name}")));
            }
            MainViewInput::AdapterSettings(AdapterSettingsOutput::AdapterChanged(adapter)) => {
                if adapter.powered && !self.adapter.powered {
                    self.device_list = launch_device_list(
                        &self.session,
                        &adapter.adapter_name,
                        &self.preferences,
                        &sender,
                    );
                    self.device_list_slot
                        .set_child(Some(self.device_list.widget()));
                }
                if !adapter.powered {
                    self.scan_state = ScanState::Idle;
                }
                self.adapter = adapter;
            }
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        match command_output {
            MainViewCommand::PairingRequest(pairing_request) => {
                let requester = pairing_request.requester();
                let requester_description = RequesterDescription {
                    device_name: self
                        .device_names
                        .get(&requester.address)
                        .cloned()
                        .unwrap_or_else(|| requester.address.to_string()),
                    radio_label: self.radio_label(&requester.adapter_name),
                };
                present_pairing_request(pairing_request, &requester_description);
            }
            MainViewCommand::ScanStarted {
                generation,
                outcome,
            } => match outcome {
                Ok(discovery) if generation == self.scan_generation => {
                    self.scan_state = ScanState::Scanning {
                        generation,
                        _discovery: discovery,
                    };
                    sender.oneshot_command(async move {
                        tokio::time::sleep(SCAN_DURATION).await;
                        MainViewCommand::ScanExpired(generation)
                    });
                }
                Ok(_stale_discovery) => {}
                Err(scan_error) => root.add_toast(adw::Toast::new(&scan_error.to_string())),
            },
            MainViewCommand::ScanExpired(expired_generation) => {
                if matches!(self.scan_state, ScanState::Scanning { generation, .. } if generation == expired_generation)
                {
                    self.scan_state = ScanState::Idle;
                }
            }
            MainViewCommand::AdapterLoaded {
                outcome,
                after_switch,
            } => match outcome {
                Ok(adapter) => {
                    let adapter_name = adapter.adapter_name.clone();
                    self.install_adapter(adapter, &sender);
                    match after_switch {
                        AfterAdapterSwitch::ShowSettings => {}
                        AfterAdapterSwitch::StartScan => self.start_scan(&sender),
                        AfterAdapterSwitch::OpenDevice(address) => {
                            let session = self.session.clone();
                            let location = DeviceLocation {
                                adapter_name,
                                address,
                            };
                            sender.oneshot_command(async move {
                                MainViewCommand::DeviceRead(read_device(&session, &location).await)
                            });
                        }
                    }
                }
                Err(adapter_error) => root.add_toast(adw::Toast::new(&adapter_error.to_string())),
            },
            MainViewCommand::DeviceRead(Ok(snapshot)) => self.open_device(snapshot, &sender),
            MainViewCommand::DeviceRead(Err(read_error)) => {
                root.add_toast(adw::Toast::new(&read_error.to_string()));
            }
        }
    }
}

relm4::new_action_group!(pub MainViewActionGroup, "win");
relm4::new_stateless_action!(pub RefreshDevicesAction, MainViewActionGroup, "refresh-devices");
relm4::new_stateless_action!(pub AboutAction, MainViewActionGroup, "about");
