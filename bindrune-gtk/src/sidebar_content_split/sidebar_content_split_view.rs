use std::collections::HashMap;

use bindrune_bluez::{
    bluetooth_adapter::{
        adapter_snapshot_and_discovery::{AdapterSnapshot, read_adapter},
        adapter_udev_hardware::AdapterHardwareDescription,
    },
    bluez_dbus_session::{BluezDbusSession, DeviceLocation},
    bluez_operation_error::BluetoothError,
    device_pairing::{
        pairing_agent_requests::{AgentPolicy, PairingRequest, pairing_requests},
        pairing_step_failure::ConnectionStepFailure,
    },
    link_quality::disconnect_reason_history::{ConnectionDrop, connection_drops},
    remote_device::{
        remote_device_change_stream::DeviceChange,
        remote_device_commands::{connect_device, disconnect_device, read_device},
        remote_device_snapshot::DeviceSnapshot,
    },
};
use futures::StreamExt;
use relm4::{adw, adw::prelude::*, gtk, prelude::*};

use crate::{
    adapter_detail::adapter_detail_page::{
        AdapterDetailPage, AdapterDetailPageInit, AdapterDetailPageInput, AdapterDetailPageOutput,
    },
    application_preferences_page::ApplicationPreferencesPage,
    background_application_lifecycle::{
        application_menu_actions::{AnswerDialogAction, AppActionGroup, QuitAction},
        gsettings_stored_preferences::{GsettingsStoredPreferences, PreferenceChange},
        startup_page_argument::StartupPage,
        system_tray_menu::{TrayContent, TrayRequest},
    },
    device_detail::{
        device_detail_page::{
            DeviceDetailPage, DeviceDetailPageInit, DeviceDetailPageInput, DeviceDetailPageOutput,
        },
        file_transfer_group::ObexAvailability,
    },
    header_transfer_popover::file_transfer_popover::{
        FileTransferPopover, FileTransferPopoverInit, FileTransferPopoverInput,
        FileTransferPopoverOutput,
    },
    incoming_request_prompts::incoming_request_prompt_queue::{
        IncomingRequestPromptQueue, IncomingRequestPromptQueueInput,
        IncomingRequestPromptQueueOutput, PromptRequest, RequesterDescription,
        answer_from_notification_target,
    },
    sidebar_content_split::{
        device_connection_history::{ChangedConnectionHistory, DeviceConnectionHistories},
        tray_device_menu_sync::TrayDeviceMenuSync,
    },
    sidebar_navigation::{
        sidebar_device_rows::SidebarDeviceRowsOutput,
        sidebar_navigation_page::{
            SidebarNavigationPage, SidebarNavigationPageInit, SidebarNavigationPageInput,
            SidebarNavigationPageOutput,
        },
    },
};

const SETTINGS_TITLE: &str = "Settings";

pub struct SidebarContentSplitViewInit {
    pub session: BluezDbusSession,
    pub obex: ObexAvailability,
    pub preferences: GsettingsStoredPreferences,
    pub radios: Vec<AdapterHardwareDescription>,
    pub adapter: AdapterSnapshot,
    pub startup_page: StartupPage,
}

enum ContentPage {
    AppSettings,
    Adapter,
    Device(Controller<DeviceDetailPage>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AfterAdapterSwitch {
    ShowAdapter,
    OpenDevice(bindrune_bluez::Address),
    StartScan,
}

pub struct SidebarContentSplitView {
    session: BluezDbusSession,
    obex: ObexAvailability,
    preferences: GsettingsStoredPreferences,
    radios: Vec<AdapterHardwareDescription>,
    adapter: AdapterSnapshot,
    device_names: HashMap<bindrune_bluez::Address, String>,
    tray_devices: TrayDeviceMenuSync,
    connection_histories: DeviceConnectionHistories,
    content_page: ContentPage,
    content_title: String,
    split_view: adw::NavigationSplitView,
    sidebar: Controller<SidebarNavigationPage>,
    adapter_page: Controller<AdapterDetailPage>,
    app_settings: Controller<ApplicationPreferencesPage>,
    transfers: Controller<FileTransferPopover>,
    prompt_center: Controller<IncomingRequestPromptQueue>,
    content_slot: adw::Bin,
}

#[derive(Debug)]
pub enum SidebarContentSplitViewInput {
    ScanFailed(BluetoothError),
    AdapterActivated(String),
    ShowAppSettings,
    RefreshDevices,
    PreferenceChanged(PreferenceChange),
    SidebarDeviceRows(SidebarDeviceRowsOutput),
    DeviceDetailPage(DeviceDetailPageOutput),
    AdapterDetailPage(AdapterDetailPageOutput),
    Transfers(FileTransferPopoverOutput),
    IncomingRequestPromptQueue(IncomingRequestPromptQueueOutput),
    Tray(TrayRequest),
}

#[derive(Debug)]
pub enum SidebarContentSplitViewOutput {
    TrayContentChanged(TrayContent),
}

#[derive(Debug)]
pub enum SidebarContentSplitViewCommand {
    PairingRequest(PairingRequest),
    AdapterLoaded {
        outcome: Result<AdapterSnapshot, BluetoothError>,
        after_switch: AfterAdapterSwitch,
    },
    DeviceRead(Result<DeviceSnapshot, BluetoothError>),
    TrayConnectionChanged(Result<(), ConnectionStepFailure>),
    ConnectionDropped(ConnectionDrop),
}

fn launch_adapter_page(
    session: &BluezDbusSession,
    adapter: &AdapterSnapshot,
    sender: &ComponentSender<SidebarContentSplitView>,
) -> Controller<AdapterDetailPage> {
    AdapterDetailPage::builder()
        .launch(AdapterDetailPageInit {
            session: session.clone(),
            adapter: adapter.clone(),
        })
        .forward(
            sender.input_sender(),
            SidebarContentSplitViewInput::AdapterDetailPage,
        )
}

impl SidebarContentSplitView {
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

    fn open_device(&mut self, snapshot: DeviceSnapshot, sender: &ComponentSender<Self>) {
        self.content_title = snapshot.display_name().to_string();
        if self.shown_device_address() == Some(snapshot.address()) {
            self.reveal_content();
            return;
        }
        let device_page = DeviceDetailPage::builder()
            .launch(DeviceDetailPageInit {
                session: self.session.clone(),
                obex: self.obex.clone(),
                adapter_address: self.adapter.address,
                accepts_files: self.preferences.accepts_files_from(snapshot.address()),
                connection_history: self.connection_histories.history_of(snapshot.address()),
                received_files_folder: self.preferences.received_files_folder(),
                snapshot,
            })
            .forward(
                sender.input_sender(),
                SidebarContentSplitViewInput::DeviceDetailPage,
            );
        self.content_slot.set_child(Some(&**device_page.widget()));
        self.content_page = ContentPage::Device(device_page);
        self.reveal_content();
    }

    fn start_scan(&self) {
        self.sidebar.emit(SidebarNavigationPageInput::StartScan);
    }

    fn switch_adapter(
        &self,
        adapter_name: String,
        after_switch: AfterAdapterSwitch,
        sender: &ComponentSender<Self>,
    ) {
        let session = self.session.clone();
        sender.oneshot_command(async move {
            SidebarContentSplitViewCommand::AdapterLoaded {
                outcome: read_adapter(&session, &adapter_name).await,
                after_switch,
            }
        });
    }

    fn install_adapter(&mut self, adapter: AdapterSnapshot, sender: &ComponentSender<Self>) {
        self.sidebar
            .emit(SidebarNavigationPageInput::AdapterInstalled(
                adapter.clone(),
            ));
        self.preferences
            .remember_adapter_name(&adapter.adapter_name);
        self.adapter_page = launch_adapter_page(&self.session, &adapter, sender);
        self.transfers
            .emit(FileTransferPopoverInput::AdapterSwitched(
                adapter.adapter_name.clone(),
            ));
        self.device_names.clear();
        self.tray_devices.forget_all_devices();
        self.adapter = adapter;
        self.publish_tray_content(sender);
        self.show_adapter_page();
    }

    fn handle_device_list_output(
        &mut self,
        device_list_output: SidebarDeviceRowsOutput,
        root: &adw::ToastOverlay,
        sender: &ComponentSender<Self>,
    ) {
        match device_list_output {
            SidebarDeviceRowsOutput::Opened(snapshot) => self.open_device(snapshot, sender),
            SidebarDeviceRowsOutput::Changed(device_change) => {
                self.remember_connection_state(&device_change);
                self.tray_devices.remember_device_change(&device_change);
                self.publish_tray_content(sender);
                self.forward_device_change(device_change);
            }
            SidebarDeviceRowsOutput::ShowUnnamedRequested => {
                self.preferences.set_show_unnamed_devices(true);
            }
            SidebarDeviceRowsOutput::Unavailable(subscription_error) => {
                root.add_toast(adw::Toast::new(&subscription_error.to_string()));
            }
        }
    }

    fn publish_tray_content(&self, sender: &ComponentSender<Self>) {
        let _app_window_gone = sender.output(SidebarContentSplitViewOutput::TrayContentChanged(
            self.tray_devices.tray_content(&self.adapter),
        ));
    }

    fn handle_tray_request(&mut self, tray_request: TrayRequest, sender: &ComponentSender<Self>) {
        match tray_request {
            TrayRequest::SetPowered(powered) => {
                self.adapter_page
                    .emit(AdapterDetailPageInput::PoweredToggled(powered));
            }
            TrayRequest::SetVisibleForPairing(visible_for_pairing) => {
                self.adapter_page
                    .emit(AdapterDetailPageInput::VisibleForPairingToggled(
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
                    SidebarContentSplitViewCommand::TrayConnectionChanged(if connected {
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

    fn show_changed_connection_history(&self, changed_history: Option<ChangedConnectionHistory>) {
        let Some(changed_history) = changed_history else {
            return;
        };
        if self.shown_device_address() != Some(changed_history.address) {
            return;
        }
        if let ContentPage::Device(device_page) = &self.content_page {
            device_page.emit(DeviceDetailPageInput::ConnectionHistoryChanged(
                changed_history.connection_history,
            ));
        }
    }

    fn remember_connection_state(&mut self, device_change: &DeviceChange) {
        let changed_history = self
            .connection_histories
            .record_device_change(device_change);
        self.show_changed_connection_history(changed_history);
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
                    device_page.emit(DeviceDetailPageInput::DeviceChanged(snapshot));
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
        device_page_output: DeviceDetailPageOutput,
        root: &adw::ToastOverlay,
        sender: &ComponentSender<Self>,
    ) {
        match device_page_output {
            DeviceDetailPageOutput::Failed(failure) => {
                root.add_toast(adw::Toast::new(&failure.to_string()));
            }
            DeviceDetailPageOutput::Forgotten { hint, rescan } => {
                if let (false, Some(forgotten_address)) = (rescan, self.shown_device_address()) {
                    self.preferences.revoke_auto_accept(forgotten_address);
                }
                root.add_toast(adw::Toast::new(&hint));
                self.show_adapter_page();
                if rescan {
                    self.start_scan();
                }
            }
            DeviceDetailPageOutput::AutoAcceptChosen {
                address,
                accepts_files,
            } => {
                if accepts_files {
                    self.preferences.allow_auto_accept(address);
                } else {
                    self.preferences.revoke_auto_accept(address);
                }
            }
            DeviceDetailPageOutput::ShowRadio(adapter_name) => {
                if let Some(shown_address) = self.shown_device_address() {
                    self.switch_adapter(
                        adapter_name,
                        AfterAdapterSwitch::OpenDevice(shown_address),
                        sender,
                    );
                }
            }
            DeviceDetailPageOutput::PairViaRadio(adapter_name) => {
                self.switch_adapter(adapter_name, AfterAdapterSwitch::StartScan, sender);
            }
            DeviceDetailPageOutput::TransfersStarted(transfer_handles) => {
                self.transfers
                    .emit(FileTransferPopoverInput::TransfersStarted(transfer_handles));
            }
        }
    }
}

#[relm4::component(pub)]
impl Component for SidebarContentSplitView {
    type Init = SidebarContentSplitViewInit;
    type Input = SidebarContentSplitViewInput;
    type Output = SidebarContentSplitViewOutput;
    type CommandOutput = SidebarContentSplitViewCommand;

    view! {
        adw::ToastOverlay {
            #[local_ref]
            split_view -> adw::NavigationSplitView {
                set_min_sidebar_width: 300.0,
                set_max_sidebar_width: 360.0,

                set_sidebar: Some(model.sidebar.widget()),

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
                "_About Bindrune" => AboutAction,
                "_Quit" => QuitAction,
            },
        }
    }

    fn init(
        view_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let SidebarContentSplitViewInit {
            session,
            obex,
            preferences,
            radios,
            adapter,
            startup_page,
        } = view_init;
        let adapter_page = launch_adapter_page(&session, &adapter, &sender);
        let app_settings = ApplicationPreferencesPage::builder()
            .launch(preferences.clone())
            .detach();
        let prompt_center = IncomingRequestPromptQueue::builder().launch(()).forward(
            sender.input_sender(),
            SidebarContentSplitViewInput::IncomingRequestPromptQueue,
        );
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
        let transfers = FileTransferPopover::builder()
            .launch(FileTransferPopoverInit {
                session: session.clone(),
                obex: obex.clone(),
                preferences: preferences.clone(),
                adapter_name: adapter.adapter_name.clone(),
            })
            .forward(
                sender.input_sender(),
                SidebarContentSplitViewInput::Transfers,
            );
        let content_slot = adw::Bin::new();
        let sidebar = SidebarNavigationPage::builder()
            .launch(SidebarNavigationPageInit {
                session: session.clone(),
                preferences: preferences.clone(),
                radios: radios.clone(),
                adapter: adapter.clone(),
                transfers_button: transfers.widget().clone(),
            })
            .forward(
                sender.input_sender(),
                |sidebar_output| match sidebar_output {
                    SidebarNavigationPageOutput::SettingsActivated => {
                        SidebarContentSplitViewInput::ShowAppSettings
                    }
                    SidebarNavigationPageOutput::AdapterActivated(adapter_name) => {
                        SidebarContentSplitViewInput::AdapterActivated(adapter_name)
                    }
                    SidebarNavigationPageOutput::DeviceListReported(device_list_output) => {
                        SidebarContentSplitViewInput::SidebarDeviceRows(device_list_output)
                    }
                    SidebarNavigationPageOutput::ScanFailed(scan_error) => {
                        SidebarContentSplitViewInput::ScanFailed(scan_error)
                    }
                },
            );
        content_slot.set_child(Some(&**adapter_page.widget()));

        let preference_sender = sender.input_sender().clone();
        preferences.connect_preference_changes(move |preference_change| {
            preference_sender.emit(SidebarContentSplitViewInput::PreferenceChanged(
                preference_change,
            ));
        });

        sender.command(|command_output, shutdown| {
            shutdown
                .register(async move {
                    let Ok(drops) = connection_drops().await else {
                        return;
                    };
                    drops
                        .for_each(|connection_drop| {
                            command_output.emit(SidebarContentSplitViewCommand::ConnectionDropped(
                                connection_drop,
                            ));
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
                            command_output.emit(SidebarContentSplitViewCommand::PairingRequest(
                                pairing_request,
                            ));
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
            tray_devices: TrayDeviceMenuSync::default(),
            connection_histories: DeviceConnectionHistories::default(),
            content_page: ContentPage::Adapter,
            content_title: initial_content_title,
            split_view: adw::NavigationSplitView::new(),
            sidebar,
            adapter_page,
            app_settings,
            transfers,
            prompt_center,
            content_slot: content_slot.clone(),
        };
        let content_slot = &content_slot;
        let split_view = &model.split_view;
        let widgets = view_output!();

        let refresh_sender = sender.input_sender().clone();
        let refresh_action: relm4::actions::RelmAction<RefreshDevicesAction> =
            relm4::actions::RelmAction::new_stateless(move |_refresh_action| {
                refresh_sender.emit(SidebarContentSplitViewInput::RefreshDevices);
            });
        let about_action: relm4::actions::RelmAction<AboutAction> =
            relm4::actions::RelmAction::new_stateless(|_about_action| {
                crate::background_application_lifecycle::application_menu_actions::show_about()
            });
        let mut window_actions =
            relm4::actions::RelmActionGroup::<SidebarContentSplitActionGroup>::new();
        window_actions.add_action(refresh_action);
        window_actions.add_action(about_action);
        window_actions.register_for_widget(&root);
        model.publish_tray_content(&sender);
        if let StartupPage::Device(address) = startup_page {
            let startup_session = model.session.clone();
            let location = DeviceLocation {
                adapter_name: model.adapter.adapter_name.clone(),
                address,
            };
            sender.oneshot_command(async move {
                SidebarContentSplitViewCommand::DeviceRead(
                    read_device(&startup_session, &location).await,
                )
            });
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match input {
            SidebarContentSplitViewInput::ScanFailed(scan_error) => {
                root.add_toast(adw::Toast::new(&scan_error.to_string()));
            }
            SidebarContentSplitViewInput::AdapterActivated(adapter_name) => {
                if adapter_name == self.adapter.adapter_name {
                    self.show_adapter_page();
                } else {
                    self.switch_adapter(adapter_name, AfterAdapterSwitch::ShowAdapter, &sender);
                }
                self.reveal_content();
            }
            SidebarContentSplitViewInput::ShowAppSettings => {
                self.show_app_settings();
                self.reveal_content();
            }
            SidebarContentSplitViewInput::RefreshDevices => {
                self.sidebar
                    .emit(SidebarNavigationPageInput::RefreshDevices);
            }
            SidebarContentSplitViewInput::PreferenceChanged(
                PreferenceChange::AutoAcceptSenders,
            ) => {
                if let ContentPage::Device(device_page) = &self.content_page {
                    let accepts_files = self
                        .preferences
                        .accepts_files_from(device_page.model().shown_address());
                    device_page.emit(DeviceDetailPageInput::AutoAcceptChanged(accepts_files));
                }
            }
            SidebarContentSplitViewInput::PreferenceChanged(
                PreferenceChange::ShowUnnamedDevices(show_unnamed_devices),
            ) => {
                self.sidebar
                    .emit(SidebarNavigationPageInput::ShowUnnamedDevices(
                        show_unnamed_devices,
                    ));
            }
            SidebarContentSplitViewInput::PreferenceChanged(
                PreferenceChange::RunInBackground(_)
                | PreferenceChange::StartAtLogin(_)
                | PreferenceChange::Unrelated,
            ) => {}
            SidebarContentSplitViewInput::SidebarDeviceRows(device_list_output) => {
                self.handle_device_list_output(device_list_output, root, &sender);
            }
            SidebarContentSplitViewInput::DeviceDetailPage(device_page_output) => {
                self.handle_device_page_output(device_page_output, root, &sender);
            }
            SidebarContentSplitViewInput::AdapterDetailPage(AdapterDetailPageOutput::Failed(
                failure,
            )) => {
                root.add_toast(adw::Toast::new(&failure.to_string()));
            }
            SidebarContentSplitViewInput::Transfers(FileTransferPopoverOutput::DecisionNeeded(
                assessment,
            )) => {
                self.prompt_center
                    .emit(IncomingRequestPromptQueueInput::Requested(
                        PromptRequest::Transfer(assessment),
                    ));
            }
            SidebarContentSplitViewInput::IncomingRequestPromptQueue(
                IncomingRequestPromptQueueOutput::TransferDecided {
                    incoming_transfer,
                    transfer_decision,
                },
            ) => {
                self.transfers.emit(FileTransferPopoverInput::Decided {
                    incoming_transfer,
                    transfer_decision,
                });
            }
            SidebarContentSplitViewInput::Transfers(FileTransferPopoverOutput::ReceivingFile {
                file_name,
            }) => {
                root.add_toast(adw::Toast::new(&format!("Receiving {file_name}")));
            }
            SidebarContentSplitViewInput::AdapterDetailPage(
                AdapterDetailPageOutput::AdapterChanged(adapter),
            ) => {
                self.sidebar
                    .emit(SidebarNavigationPageInput::AdapterChanged(adapter.clone()));
                self.adapter = adapter;
                self.publish_tray_content(&sender);
            }
            SidebarContentSplitViewInput::Tray(tray_request) => {
                self.handle_tray_request(tray_request, &sender)
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
            SidebarContentSplitViewCommand::PairingRequest(pairing_request) => {
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
                    .emit(IncomingRequestPromptQueueInput::Requested(
                        PromptRequest::Pairing {
                            pairing_request,
                            requester: requester_description,
                        },
                    ));
            }
            SidebarContentSplitViewCommand::AdapterLoaded {
                outcome,
                after_switch,
            } => match outcome {
                Ok(adapter) => {
                    let adapter_name = adapter.adapter_name.clone();
                    self.install_adapter(adapter, &sender);
                    match after_switch {
                        AfterAdapterSwitch::ShowAdapter => {}
                        AfterAdapterSwitch::StartScan => self.start_scan(),
                        AfterAdapterSwitch::OpenDevice(address) => {
                            let session = self.session.clone();
                            let location = DeviceLocation {
                                adapter_name,
                                address,
                            };
                            sender.oneshot_command(async move {
                                SidebarContentSplitViewCommand::DeviceRead(
                                    read_device(&session, &location).await,
                                )
                            });
                        }
                    }
                }
                Err(adapter_error) => root.add_toast(adw::Toast::new(&adapter_error.to_string())),
            },
            SidebarContentSplitViewCommand::DeviceRead(Ok(snapshot)) => {
                self.open_device(snapshot, &sender)
            }
            SidebarContentSplitViewCommand::DeviceRead(Err(read_error)) => {
                root.add_toast(adw::Toast::new(&read_error.to_string()));
            }
            SidebarContentSplitViewCommand::ConnectionDropped(connection_drop) => {
                if connection_drop.adapter_name == self.adapter.adapter_name {
                    let changed_history = self
                        .connection_histories
                        .record_connection_drop(connection_drop);
                    self.show_changed_connection_history(changed_history);
                }
            }
            SidebarContentSplitViewCommand::TrayConnectionChanged(Ok(())) => {}
            SidebarContentSplitViewCommand::TrayConnectionChanged(Err(connection_failure)) => {
                root.add_toast(adw::Toast::new(&connection_failure.to_string()));
            }
        }
    }
}

relm4::new_action_group!(pub SidebarContentSplitActionGroup, "win");
relm4::new_stateless_action!(pub RefreshDevicesAction, SidebarContentSplitActionGroup, "refresh-devices");
relm4::new_stateless_action!(pub AboutAction, SidebarContentSplitActionGroup, "about");
