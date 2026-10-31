use std::path::PathBuf;

use futures::StreamExt;
use overskride_bluetooth::adapter_state::{
    AdapterSnapshot, adapter_changes, set_alias, set_discoverable, set_discoverable_timeout,
    set_powered,
};
use overskride_bluetooth::bluetooth_error::BluetoothError;
use overskride_bluetooth::session::BluetoothSession;
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::gio;
use relm4::prelude::*;

use crate::app_preferences::AppPreferences;
use crate::widgets::page_templates::{ClampedPage, PageHeader};

pub struct AdapterSettingsInit {
    pub session: BluetoothSession,
    pub adapter: AdapterSnapshot,
    pub preferences: AppPreferences,
}

pub struct AdapterSettings {
    session: BluetoothSession,
    adapter: AdapterSnapshot,
    preferences: AppPreferences,
    received_files_folder: PathBuf,
    alias_changed: bool,
    folder_changed: bool,
}

#[derive(Debug)]
pub enum AdapterSettingsInput {
    SetPowered(bool),
    SetDiscoverable(bool),
    SetDiscoverableTimeout(u32),
    SetAlias(String),
    ChooseReceivedFilesFolder,
    ReceivedFilesFolderChosen(PathBuf),
}

#[derive(Debug)]
pub enum AdapterSettingsOutput {
    Failed(BluetoothError),
    AdapterChanged(AdapterSnapshot),
}

#[derive(Debug)]
pub enum AdapterSettingsCommand {
    AdapterChanged(AdapterSnapshot),
    ActionFinished(Result<AdapterSnapshot, BluetoothError>),
}

impl AdapterSettings {
    fn show_adapter(&mut self, adapter: AdapterSnapshot, sender: &ComponentSender<Self>) {
        self.alias_changed = adapter.alias != self.adapter.alias;
        self.adapter = adapter;
        let _app_gone = sender.output(AdapterSettingsOutput::AdapterChanged(self.adapter.clone()));
    }
}

const MAXIMUM_TIMEOUT_MINUTES: u32 = 60;

fn whole_minutes(spin_value: f64) -> u32 {
    (0..=MAXIMUM_TIMEOUT_MINUTES)
        .find(|candidate_minutes| f64::from(*candidate_minutes) >= spin_value.round())
        .unwrap_or(MAXIMUM_TIMEOUT_MINUTES)
}

#[relm4::component(pub)]
impl Component for AdapterSettings {
    type Init = AdapterSettingsInit;
    type Input = AdapterSettingsInput;
    type Output = AdapterSettingsOutput;
    type CommandOutput = AdapterSettingsCommand;

    view! {
        #[template]
        ClampedPage {
            #[template_child]
            page_content {
                #[template]
                PageHeader {
                    #[template_child]
                    header_icon {
                        set_icon_name: Some("bluetooth-symbolic"),
                    },
                    #[template_child]
                    header_title {
                        set_label: "Bluetooth Settings",
                    },
                },

                    adw::PreferencesGroup {
                        set_title: "Bluetooth Adapter Status",
                        #[watch]
                        set_description: Some(&model.adapter.radio_label),

                        adw::SwitchRow {
                            set_title: "Powered",
                            #[watch]
                            #[block_signal(powered_handler)]
                            set_active: model.adapter.powered,
                            connect_active_notify[sender] => move |switch_row| {
                                sender.input(AdapterSettingsInput::SetPowered(switch_row.is_active()));
                            } @powered_handler,
                        },

                        adw::SwitchRow {
                            set_title: "Discoverable",
                            set_subtitle: "Visible to other devices",
                            #[watch]
                            #[block_signal(discoverable_handler)]
                            set_active: model.adapter.discoverable,
                            connect_active_notify[sender] => move |switch_row| {
                                sender.input(AdapterSettingsInput::SetDiscoverable(switch_row.is_active()));
                            } @discoverable_handler,
                        },
                    },

                    adw::PreferencesGroup {
                        set_title: "Adapter Properties",

                        adw::SpinRow {
                            set_title: "Discoverable Timeout",
                            set_subtitle: "In minutes, 0 means forever",
                            set_adjustment: Some(&gtk::Adjustment::new(0.0, 0.0, f64::from(MAXIMUM_TIMEOUT_MINUTES), 1.0, 5.0, 0.0)),
                            #[watch]
                            #[block_signal(timeout_handler)]
                            set_value: f64::from(model.adapter.discoverable_timeout_minutes),
                            connect_value_notify[sender] => move |spin_row| {
                                sender.input(AdapterSettingsInput::SetDiscoverableTimeout(whole_minutes(spin_row.value())));
                            } @timeout_handler,
                        },

                        adw::EntryRow {
                            set_title: "Adapter Name",
                            set_show_apply_button: true,
                            #[track(model.alias_changed)]
                            set_text: &model.adapter.alias,
                            connect_apply[sender] => move |entry_row| {
                                sender.input(AdapterSettingsInput::SetAlias(entry_row.text().to_string()));
                            },
                        },
                    },

                    adw::PreferencesGroup {
                        set_title: "System Settings",

                        #[name = "auto_accept_row"]
                        adw::SwitchRow {
                            set_title: "Auto Accept",
                            set_subtitle: "Accept files from trusted devices without asking",
                        },

                        #[name = "keep_accepting_row"]
                        adw::SwitchRow {
                            set_title: "Keep Accepting",
                            set_subtitle: "After accepting one file, accept further files from the same device",
                        },

                        adw::ActionRow {
                            set_title: "Received Files Location",
                            set_subtitle_selectable: true,
                            #[track(model.folder_changed)]
                            set_subtitle: &model.received_files_folder.to_string_lossy(),

                            add_suffix = &gtk::Button {
                                set_icon_name: "folder-symbolic",
                                set_valign: gtk::Align::Center,
                                add_css_class: "flat",
                                connect_clicked => AdapterSettingsInput::ChooseReceivedFilesFolder,
                            },
                        },

                        #[name = "hide_unknown_row"]
                        adw::SwitchRow {
                            set_title: "Hide Unknown Devices",
                            set_subtitle: "Devices without a name stay out of the device list",
                        },
                    },
            },
        }
    }

    fn init(
        settings_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {
            session: settings_init.session.clone(),
            received_files_folder: settings_init.preferences.received_files_folder(),
            adapter: settings_init.adapter,
            preferences: settings_init.preferences,
            alias_changed: true,
            folder_changed: true,
        };
        let widgets = view_output!();

        let settings = model.preferences.settings();
        settings
            .bind(
                "auto-accept-from-trusted",
                &widgets.auto_accept_row,
                "active",
            )
            .build();
        settings
            .bind("first-auto-accept", &widgets.keep_accepting_row, "active")
            .build();
        settings
            .bind("hide-unknown-devices", &widgets.hide_unknown_row, "active")
            .build();

        let session = settings_init.session;
        let adapter_name = model.adapter.adapter_name.clone();
        sender.command(move |command_output, shutdown| {
            shutdown
                .register(async move {
                    let Ok(adapter_snapshots) = adapter_changes(&session, &adapter_name).await
                    else {
                        return;
                    };
                    adapter_snapshots
                        .for_each(|adapter_snapshot| {
                            command_output
                                .emit(AdapterSettingsCommand::AdapterChanged(adapter_snapshot));
                            futures::future::ready(())
                        })
                        .await;
                })
                .drop_on_shutdown()
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        self.alias_changed = false;
        self.folder_changed = false;
        let session = self.session.clone();
        let adapter_name = self.adapter.adapter_name.clone();
        match input {
            AdapterSettingsInput::SetPowered(powered) if powered != self.adapter.powered => {
                sender.oneshot_command(async move {
                    AdapterSettingsCommand::ActionFinished(
                        set_powered(&session, &adapter_name, powered).await,
                    )
                });
            }
            AdapterSettingsInput::SetDiscoverable(discoverable)
                if discoverable != self.adapter.discoverable =>
            {
                sender.oneshot_command(async move {
                    AdapterSettingsCommand::ActionFinished(
                        set_discoverable(&session, &adapter_name, discoverable).await,
                    )
                });
            }
            AdapterSettingsInput::SetDiscoverableTimeout(timeout_minutes)
                if timeout_minutes != self.adapter.discoverable_timeout_minutes =>
            {
                sender.oneshot_command(async move {
                    AdapterSettingsCommand::ActionFinished(
                        set_discoverable_timeout(&session, &adapter_name, timeout_minutes).await,
                    )
                });
            }
            AdapterSettingsInput::SetAlias(alias) => {
                sender.oneshot_command(async move {
                    AdapterSettingsCommand::ActionFinished(
                        set_alias(&session, &adapter_name, alias).await,
                    )
                });
            }
            AdapterSettingsInput::ChooseReceivedFilesFolder => {
                let folder_dialog = gtk::FileDialog::builder()
                    .title("Choose Where Received Files Go")
                    .modal(true)
                    .initial_folder(&gio::File::for_path(&self.received_files_folder))
                    .build();
                let input_sender = sender.input_sender().clone();
                relm4::spawn_local(async move {
                    let parent_window = relm4::main_application().active_window();
                    let Ok(chosen_folder) = folder_dialog
                        .select_folder_future(parent_window.as_ref())
                        .await
                    else {
                        return;
                    };
                    if let Some(folder_path) = chosen_folder.path() {
                        input_sender
                            .emit(AdapterSettingsInput::ReceivedFilesFolderChosen(folder_path));
                    }
                });
            }
            AdapterSettingsInput::ReceivedFilesFolderChosen(folder_path) => {
                self.preferences.set_received_files_folder(&folder_path);
                self.received_files_folder = folder_path;
                self.folder_changed = true;
            }
            _ => {}
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        self.alias_changed = false;
        self.folder_changed = false;
        match command_output {
            AdapterSettingsCommand::AdapterChanged(adapter_snapshot)
            | AdapterSettingsCommand::ActionFinished(Ok(adapter_snapshot)) => {
                self.show_adapter(adapter_snapshot, &sender);
            }
            AdapterSettingsCommand::ActionFinished(Err(adapter_error)) => {
                let _app_gone = sender.output(AdapterSettingsOutput::Failed(adapter_error));
            }
        }
    }
}
