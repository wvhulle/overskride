use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

use bindrune_bluez::{
    bluetooth_adapter::{
        adapter_snapshot_and_discovery::{
            AdapterSnapshot, DiscoverySession, read_adapter, start_discovery,
        },
        adapter_udev_hardware::RadioDescription,
    },
    bluez_dbus_session::{BluetoothSession, DeviceLocation},
    bluez_operation_error::BluetoothError,
    device_pairing::{
        pairing_agent_requests::{AgentPolicy, PairingRequest, pairing_requests},
        pairing_step_failure::ConnectionStepFailure,
    },
    link_quality::disconnect_reason_history::{
        ConnectionDrop, ConnectionHistory, connection_drops,
    },
    remote_device::{
        remote_device_change_stream::DeviceChange,
        remote_device_commands::{connect_device, disconnect_device, read_device},
        remote_device_snapshot::DeviceSnapshot,
    },
};
use futures::StreamExt;
use relm4::{adw, adw::prelude::*, factory::FactoryVecDeque, gtk, prelude::*};

use crate::{
    adapter_detail::adapter_detail_page::{
        AdapterPage, AdapterPageInit, AdapterPageInput, AdapterPageOutput,
    },
    application_preferences_page::AppSettingsPage,
    background_application_lifecycle::{
        application_menu_actions::{AnswerDialogAction, AppActionGroup, QuitAction},
        gsettings_stored_preferences::{AppPreferences, PreferenceChange},
        system_tray_menu::{TrayContent, TrayDevice, TrayRequest},
    },
    device_detail::{
        device_detail_page::{DevicePage, DevicePageInit, DevicePageInput, DevicePageOutput},
        file_transfer_group::ObexAvailability,
    },
    header_transfer_popover::file_transfer_popover::{
        TransfersInit, TransfersInput, TransfersOutput, TransfersPopover,
    },
    incoming_request_prompts::incoming_request_prompt_queue::{
        PromptCenter, PromptCenterInput, PromptCenterOutput, PromptRequest, RequesterDescription,
        answer_from_notification_target,
    },
    sidebar_device_list::{
        sidebar_adapter_row::{AdapterRow, AdapterRowInit},
        sidebar_device_rows::{DeviceList, DeviceListInput, DeviceListOutput, DeviceListSource},
    },
};

const SCAN_DURATION_SECONDS: u32 = 60;
const SCAN_TICK: Duration = Duration::from_secs(1);
const SETTINGS_TITLE: &str = "Settings";

pub struct MainViewInit {
    pub session: BluetoothSession,
    pub obex: ObexAvailability,
    pub preferences: AppPreferences,
    pub radios: Vec<RadioDescription>,
    pub adapter: AdapterSnapshot,
}

enum ContentPage {
    AppSettings,
    Adapter,
    Device(Controller<DevicePage>),
}

enum ScanState {
    Idle,
    Scanning {
        generation: u64,
        remaining_seconds: u32,
        _discovery: DiscoverySession,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AfterAdapterSwitch {
    ShowAdapter,
    OpenDevice(bindrune_bluez::Address),
    StartScan,
}

pub struct MainView {
    session: BluetoothSession,
    obex: ObexAvailability,
    preferences: AppPreferences,
    radios: Vec<RadioDescription>,
    adapter: AdapterSnapshot,
    device_names: HashMap<bindrune_bluez::Address, String>,
    paired_tray_devices: BTreeMap<bindrune_bluez::Address, TrayDevice>,
    connection_histories: BTreeMap<bindrune_bluez::Address, ConnectionHistory>,
    scan_state: ScanState,
    scan_generation: u64,
    content_page: ContentPage,
    content_title: String,
    split_view: adw::NavigationSplitView,
    device_list: Controller<DeviceList>,
    adapter_page: Controller<AdapterPage>,
    app_settings: Controller<AppSettingsPage>,
    adapter_rows: FactoryVecDeque<AdapterRow>,
    transfers: Controller<TransfersPopover>,
    prompt_center: Controller<PromptCenter>,
    device_list_slot: adw::Bin,
    content_slot: adw::Bin,
}

#[derive(Debug)]
pub enum MainViewInput {
    ScanToggled(bool),
    AdapterActivated(String),
    ShowAppSettings,
    RefreshDevices,
    PreferenceChanged(PreferenceChange),
    DeviceList(DeviceListOutput),
    DevicePage(DevicePageOutput),
    AdapterPage(AdapterPageOutput),
    Transfers(TransfersOutput),
    PromptCenter(PromptCenterOutput),
    Tray(TrayRequest),
}

#[derive(Debug)]
pub enum MainViewOutput {
    TrayContentChanged(TrayContent),
}

#[derive(Debug)]
pub enum MainViewCommand {
    PairingRequest(PairingRequest),
    ScanStarted {
        generation: u64,
        outcome: Result<DiscoverySession, BluetoothError>,
    },
    ScanTick(u64),
    AdapterLoaded {
        outcome: Result<AdapterSnapshot, BluetoothError>,
        after_switch: AfterAdapterSwitch,
    },
    DeviceRead(Result<DeviceSnapshot, BluetoothError>),
    TrayConnectionChanged(Result<(), ConnectionStepFailure>),
    ConnectionDropped(ConnectionDrop),
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
            show_unnamed_devices: preferences.show_unnamed_devices(),
        })
        .forward(sender.input_sender(), MainViewInput::DeviceList)
}

fn launch_adapter_page(
    session: &BluetoothSession,
    adapter: &AdapterSnapshot,
    sender: &ComponentSender<MainView>,
) -> Controller<AdapterPage> {
    AdapterPage::builder()
        .launch(AdapterPageInit {
            session: session.clone(),
            adapter: adapter.clone(),
        })
        .forward(sender.input_sender(), MainViewInput::AdapterPage)
}

impl MainView {
    fn is_scanning(&self) -> bool {
        matches!(self.scan_state, ScanState::Scanning { .. })
    }

    fn scan_row_title(&self) -> &'static str {
        if self.is_scanning() {
            "Looking for Nearby Devices"
        } else {
            "Scan for Nearby Devices"
        }
    }

    fn scan_row_subtitle(&self) -> String {
        match self.scan_state {
            ScanState::Scanning {
                remaining_seconds, ..
            } => format!("{remaining_seconds} s left"),
            ScanState::Idle => "Finds devices that are in pairing mode".to_string(),
        }
    }

    fn scan_button_label(&self) -> &'static str {
        if self.is_scanning() { "Stop" } else { "Scan" }
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

    fn reveal_content(&self) {
        self.split_view.set_show_content(true);
    }

    pub fn content_navigation(&self) -> &adw::NavigationSplitView {
        &self.split_view
    }

    fn show_app_settings(&mut self) {
        self.content_page = ContentPage::AppSettings;
        self.content_slot
            .set_child(Some(&**self.app_settings.widget()));
        self.content_title = SETTINGS_TITLE.to_string();
    }

    fn show_adapter_page(&mut self) {
        self.content_page = ContentPage::Adapter;
        self.content_slot
            .set_child(Some(&**self.adapter_page.widget()));
        self.content_title = self.adapter.radio_label.clone();
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

    fn open_device(&mut self, snapshot: DeviceSnapshot, sender: &ComponentSender<Self>) {
        self.content_title = snapshot.display_name().to_string();
        if self.shown_device_address() == Some(snapshot.address()) {
            self.reveal_content();
            return;
        }
        let device_page = DevicePage::builder()
            .launch(DevicePageInit {
                session: self.session.clone(),
                obex: self.obex.clone(),
                adapter_address: self.adapter.address,
                accepts_files: self.preferences.accepts_files_from(snapshot.address()),
                connection_history: self.connection_history_of(snapshot.address()),
                received_files_folder: self.preferences.received_files_folder(),
                snapshot,
            })
            .forward(sender.input_sender(), MainViewInput::DevicePage);
        self.content_slot.set_child(Some(&**device_page.widget()));
        self.content_page = ContentPage::Device(device_page);
        self.reveal_content();
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
        self.adapter_page = launch_adapter_page(&self.session, &adapter, sender);
        self.device_list_slot
            .set_child(Some(self.device_list.widget()));
        self.transfers.emit(TransfersInput::AdapterSwitched(
            adapter.adapter_name.clone(),
        ));
        self.device_names.clear();
        self.paired_tray_devices.clear();
        self.adapter = adapter;
        self.publish_tray_content(sender);
        self.mark_active_adapter();
        self.show_adapter_page();
    }

    fn handle_device_list_output(
        &mut self,
        device_list_output: DeviceListOutput,
        root: &adw::ToastOverlay,
        sender: &ComponentSender<Self>,
    ) {
        match device_list_output {
            DeviceListOutput::Opened(snapshot) => self.open_device(snapshot, sender),
            DeviceListOutput::Changed(device_change) => {
                self.remember_connection_state(&device_change);
                self.remember_tray_device(&device_change);
                self.publish_tray_content(sender);
                self.forward_device_change(device_change);
            }
            DeviceListOutput::ShowUnnamedRequested => {
                self.preferences.set_show_unnamed_devices(true);
            }
            DeviceListOutput::Unavailable(subscription_error) => {
                root.add_toast(adw::Toast::new(&subscription_error.to_string()));
            }
        }
    }

    fn remember_tray_device(&mut self, device_change: &DeviceChange) {
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

    fn publish_tray_content(&self, sender: &ComponentSender<Self>) {
        let mut paired_devices: Vec<TrayDevice> =
            self.paired_tray_devices.values().cloned().collect();
        paired_devices.sort_by(|first_device, second_device| {
            second_device
                .connected
                .cmp(&first_device.connected)
                .then_with(|| first_device.display_name.cmp(&second_device.display_name))
        });
        let _app_window_gone = sender.output(MainViewOutput::TrayContentChanged(TrayContent {
            powered: self.adapter.powered,
            visible_for_pairing: self.adapter.discoverable,
            paired_devices,
        }));
    }

    fn handle_tray_request(&mut self, tray_request: TrayRequest, sender: &ComponentSender<Self>) {
        match tray_request {
            TrayRequest::SetPowered(powered) => {
                self.adapter_page
                    .emit(AdapterPageInput::PoweredToggled(powered));
            }
            TrayRequest::SetVisibleForPairing(visible_for_pairing) => {
                self.adapter_page
                    .emit(AdapterPageInput::VisibleForPairingToggled(
                        visible_for_pairing,
                    ));
            }
            TrayRequest::SetDeviceConnected { address, connected } => {
                let session = self.session.clone();
                let location = DeviceLocation {
                    adapter_name: self.adapter.adapter_name.clone(),
                    address,
                };
                sender.oneshot_command(async move {
                    MainViewCommand::TrayConnectionChanged(if connected {
                        connect_device(&session, &location).await
                    } else {
                        disconnect_device(&session, &location).await
                    })
                });
            }
            TrayRequest::ShowSettings => {
                self.show_app_settings();
                self.reveal_content();
            }
            TrayRequest::OpenWindow | TrayRequest::Quit => {}
        }
    }

    fn shown_device_address(&self) -> Option<bindrune_bluez::Address> {
        match &self.content_page {
            ContentPage::Device(device_page) => Some(device_page.model().shown_address()),
            ContentPage::AppSettings | ContentPage::Adapter => None,
        }
    }

    fn connection_history_of(&self, address: bindrune_bluez::Address) -> ConnectionHistory {
        self.connection_histories
            .get(&address)
            .cloned()
            .unwrap_or_default()
    }

    fn update_connection_history(
        &mut self,
        address: bindrune_bluez::Address,
        change_history: impl FnOnce(&mut ConnectionHistory),
    ) {
        let connection_history = self.connection_histories.entry(address).or_default();
        let previous_history = connection_history.clone();
        change_history(connection_history);
        if *connection_history == previous_history || self.shown_device_address() != Some(address) {
            return;
        }
        if let ContentPage::Device(device_page) = &self.content_page {
            device_page.emit(DevicePageInput::ConnectionHistoryChanged(
                self.connection_history_of(address),
            ));
        }
    }

    fn remember_connection_state(&mut self, device_change: &DeviceChange) {
        match device_change {
            DeviceChange::Appeared(snapshot) | DeviceChange::Updated(snapshot) => {
                let connected = snapshot.connected;
                self.update_connection_history(snapshot.address(), |connection_history| {
                    if connected {
                        connection_history.record_connected(Instant::now());
                    } else {
                        connection_history.record_disconnected();
                    }
                });
            }
            DeviceChange::Disappeared(address) => {
                self.connection_histories.remove(address);
            }
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
                    self.show_adapter_page();
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
                if let (false, Some(forgotten_address)) = (rescan, self.shown_device_address()) {
                    self.preferences.revoke_auto_accept(forgotten_address);
                }
                root.add_toast(adw::Toast::new(&hint));
                self.show_adapter_page();
                if rescan {
                    self.start_scan(sender);
                }
            }
            DevicePageOutput::AutoAcceptChosen {
                address,
                accepts_files,
            } => {
                if accepts_files {
                    self.preferences.allow_auto_accept(address);
                } else {
                    self.preferences.revoke_auto_accept(address);
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
    type Output = MainViewOutput;
    type CommandOutput = MainViewCommand;

    view! {
        adw::ToastOverlay {
            #[local_ref]
            split_view -> adw::NavigationSplitView {
                set_min_sidebar_width: 300.0,
                set_max_sidebar_width: 360.0,

                #[wrap(Some)]
                set_sidebar = &adw::NavigationPage {
                    set_title: "Overskride",

                    #[wrap(Some)]
                    set_child = &adw::ToolbarView {
                        add_top_bar = &adw::HeaderBar {
                            pack_start: model.transfers.widget(),
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
                                        connect_activated => MainViewInput::ShowAppSettings,
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

                                    adw::ActionRow {
                                        #[watch]
                                        set_title: model.scan_row_title(),
                                        #[watch]
                                        set_subtitle: &model.scan_row_subtitle(),

                                        add_suffix = &gtk::ToggleButton {
                                            set_valign: gtk::Align::Center,
                                            #[watch]
                                            set_tooltip_text: Some(if model.is_scanning() {
                                                "Stop looking for nearby devices"
                                            } else {
                                                "Look for nearby devices for one minute"
                                            }),
                                            #[watch]
                                            #[block_signal(scan_handler)]
                                            set_active: model.is_scanning(),
                                            connect_toggled[sender] => move |scan_button| {
                                                sender.input(MainViewInput::ScanToggled(scan_button.is_active()));
                                            } @scan_handler,

                                            gtk::Box {
                                                set_spacing: 6,

                                                adw::Spinner {
                                                    #[watch]
                                                    set_visible: model.is_scanning(),
                                                },

                                                gtk::Label {
                                                    #[watch]
                                                    set_label: model.scan_button_label(),
                                                },
                                            },
                                        },
                                    },

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
                "_Quit" => QuitAction,
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
        let adapter_page = launch_adapter_page(&session, &adapter, &sender);
        let app_settings = AppSettingsPage::builder()
            .launch(preferences.clone())
            .detach();
        let mut adapter_rows = FactoryVecDeque::builder()
            .launch(adw::PreferencesGroup::default())
            .forward(sender.input_sender(), MainViewInput::AdapterActivated);
        radios.iter().for_each(|radio| {
            adapter_rows.guard().push_back(AdapterRowInit {
                radio: radio.clone(),
                active: radio.adapter_name == adapter.adapter_name,
            });
        });
        let prompt_center = PromptCenter::builder()
            .launch(())
            .forward(sender.input_sender(), MainViewInput::PromptCenter);
        let prompt_center_sender = prompt_center.sender().clone();
        let answer_dialog_action: relm4::actions::RelmAction<AnswerDialogAction> =
            relm4::actions::RelmAction::new_with_target_value(
                move |_answer_dialog_action, notification_target: (u64, String)| {
                    answer_from_notification_target(&prompt_center_sender, notification_target);
                },
            );
        let mut prompt_actions = relm4::actions::RelmActionGroup::<AppActionGroup>::new();
        prompt_actions.add_action(answer_dialog_action);
        prompt_actions.register_for_main_application();
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
        content_slot.set_child(Some(&**adapter_page.widget()));

        let preference_sender = sender.input_sender().clone();
        preferences.connect_preference_changes(move |preference_change| {
            preference_sender.emit(MainViewInput::PreferenceChanged(preference_change));
        });

        sender.command(|command_output, shutdown| {
            shutdown
                .register(async move {
                    let Ok(drops) = connection_drops().await else {
                        return;
                    };
                    drops
                        .for_each(|connection_drop| {
                            command_output
                                .emit(MainViewCommand::ConnectionDropped(connection_drop));
                            futures::future::ready(())
                        })
                        .await;
                })
                .drop_on_shutdown()
        });

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

        let initial_content_title = adapter.radio_label.clone();
        let model = Self {
            session,
            obex,
            preferences,
            radios,
            adapter,
            device_names: HashMap::new(),
            paired_tray_devices: BTreeMap::new(),
            connection_histories: BTreeMap::new(),
            scan_state: ScanState::Idle,
            scan_generation: 0,
            content_page: ContentPage::Adapter,
            content_title: initial_content_title,
            split_view: adw::NavigationSplitView::new(),
            device_list,
            adapter_page,
            app_settings,
            adapter_rows,
            transfers,
            prompt_center,
            device_list_slot: device_list_slot.clone(),
            content_slot: content_slot.clone(),
        };
        let device_list_slot = &device_list_slot;
        let content_slot = &content_slot;
        let split_view = &model.split_view;
        let adapter_group = model.adapter_rows.widget();
        let widgets = view_output!();

        let refresh_sender = sender.input_sender().clone();
        let refresh_action: relm4::actions::RelmAction<RefreshDevicesAction> =
            relm4::actions::RelmAction::new_stateless(move |_refresh_action| {
                refresh_sender.emit(MainViewInput::RefreshDevices);
            });
        let about_action: relm4::actions::RelmAction<AboutAction> =
            relm4::actions::RelmAction::new_stateless(|_about_action| {
                crate::background_application_lifecycle::application_menu_actions::show_about()
            });
        let mut window_actions = relm4::actions::RelmActionGroup::<MainViewActionGroup>::new();
        window_actions.add_action(refresh_action);
        window_actions.add_action(about_action);
        window_actions.register_for_widget(&root);
        model.publish_tray_content(&sender);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match input {
            MainViewInput::ScanToggled(true) => self.start_scan(&sender),
            MainViewInput::ScanToggled(false) => self.scan_state = ScanState::Idle,
            MainViewInput::AdapterActivated(adapter_name) => {
                if adapter_name == self.adapter.adapter_name {
                    self.show_adapter_page();
                } else {
                    self.switch_adapter(adapter_name, AfterAdapterSwitch::ShowAdapter, &sender);
                }
                self.reveal_content();
            }
            MainViewInput::ShowAppSettings => {
                self.show_app_settings();
                self.reveal_content();
            }
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
            MainViewInput::PreferenceChanged(PreferenceChange::AutoAcceptSenders) => {
                if let ContentPage::Device(device_page) = &self.content_page {
                    let accepts_files = self
                        .preferences
                        .accepts_files_from(device_page.model().shown_address());
                    device_page.emit(DevicePageInput::AutoAcceptChanged(accepts_files));
                }
            }
            MainViewInput::PreferenceChanged(PreferenceChange::ShowUnnamedDevices(
                show_unnamed_devices,
            )) => {
                self.device_list
                    .emit(DeviceListInput::ShowUnnamedDevices(show_unnamed_devices));
            }
            MainViewInput::PreferenceChanged(
                PreferenceChange::RunInBackground(_)
                | PreferenceChange::StartAtLogin(_)
                | PreferenceChange::Unrelated,
            ) => {}
            MainViewInput::DeviceList(device_list_output) => {
                self.handle_device_list_output(device_list_output, root, &sender);
            }
            MainViewInput::DevicePage(device_page_output) => {
                self.handle_device_page_output(device_page_output, root, &sender);
            }
            MainViewInput::AdapterPage(AdapterPageOutput::Failed(failure)) => {
                root.add_toast(adw::Toast::new(&failure.to_string()));
            }
            MainViewInput::Transfers(TransfersOutput::DecisionNeeded(assessment)) => {
                self.prompt_center
                    .emit(PromptCenterInput::Requested(PromptRequest::Transfer(
                        assessment,
                    )));
            }
            MainViewInput::PromptCenter(PromptCenterOutput::TransferDecided {
                incoming_transfer,
                transfer_decision,
            }) => {
                self.transfers.emit(TransfersInput::Decided {
                    incoming_transfer,
                    transfer_decision,
                });
            }
            MainViewInput::Transfers(TransfersOutput::ReceivingFile { file_name }) => {
                root.add_toast(adw::Toast::new(&format!("Receiving {file_name}")));
            }
            MainViewInput::AdapterPage(AdapterPageOutput::AdapterChanged(adapter)) => {
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
                self.publish_tray_content(&sender);
            }
            MainViewInput::Tray(tray_request) => self.handle_tray_request(tray_request, &sender),
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
                self.prompt_center
                    .emit(PromptCenterInput::Requested(PromptRequest::Pairing {
                        pairing_request,
                        requester: requester_description,
                    }));
            }
            MainViewCommand::ScanStarted {
                generation,
                outcome,
            } => match outcome {
                Ok(discovery) if generation == self.scan_generation => {
                    self.scan_state = ScanState::Scanning {
                        generation,
                        remaining_seconds: SCAN_DURATION_SECONDS,
                        _discovery: discovery,
                    };
                    sender.command(move |command_output, shutdown| {
                        shutdown
                            .register(
                                futures::stream::iter(0..SCAN_DURATION_SECONDS)
                                    .then(|_elapsed_seconds| tokio::time::sleep(SCAN_TICK))
                                    .for_each(move |()| {
                                        command_output.emit(MainViewCommand::ScanTick(generation));
                                        futures::future::ready(())
                                    }),
                            )
                            .drop_on_shutdown()
                    });
                }
                Ok(_stale_discovery) => {}
                Err(scan_error) => root.add_toast(adw::Toast::new(&scan_error.to_string())),
            },
            MainViewCommand::ScanTick(ticked_generation) => {
                let ScanState::Scanning {
                    generation,
                    remaining_seconds,
                    ..
                } = &mut self.scan_state
                else {
                    return;
                };
                if *generation != ticked_generation {
                    return;
                }
                *remaining_seconds = remaining_seconds.saturating_sub(1);
                if *remaining_seconds == 0 {
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
                        AfterAdapterSwitch::ShowAdapter => {}
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
            MainViewCommand::ConnectionDropped(connection_drop) => {
                if connection_drop.adapter_name == self.adapter.adapter_name {
                    self.update_connection_history(connection_drop.address, |connection_history| {
                        connection_history.record_drop(connection_drop);
                    });
                }
            }
            MainViewCommand::TrayConnectionChanged(Ok(())) => {}
            MainViewCommand::TrayConnectionChanged(Err(connection_failure)) => {
                root.add_toast(adw::Toast::new(&connection_failure.to_string()));
            }
        }
    }
}

relm4::new_action_group!(pub MainViewActionGroup, "win");
relm4::new_stateless_action!(pub RefreshDevicesAction, MainViewActionGroup, "refresh-devices");
relm4::new_stateless_action!(pub AboutAction, MainViewActionGroup, "about");
