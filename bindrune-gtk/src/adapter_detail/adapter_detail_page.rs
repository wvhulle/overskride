use bindrune_bluez::{
    bluetooth_adapter::adapter_snapshot_and_discovery::{
        AdapterSnapshot, adapter_changes, set_alias, set_powered,
    },
    bluez_dbus_session::BluezDbusSession,
    bluez_operation_error::BluetoothError,
};
use futures::StreamExt;
use relm4::{adw, adw::prelude::*, gtk, prelude::*};

use crate::{
    adapter_detail::{
        adapter_hardware_group::{AdapterHardwareGroup, AdapterHardwareGroupInput},
        pairing_visibility_group::{
            PairingVisibilityGroup, PairingVisibilityGroupInit, PairingVisibilityGroupInput,
            PairingVisibilityGroupOutput,
        },
    },
    shared_page_widget_templates::{ClampedPage, PageHeader},
};

pub struct AdapterDetailPageInit {
    pub session: BluezDbusSession,
    pub adapter: AdapterSnapshot,
}

pub struct AdapterDetailPage {
    session: BluezDbusSession,
    adapter: AdapterSnapshot,
    alias_changed: bool,
    alias_saving: bool,
    pairing_visibility: Controller<PairingVisibilityGroup>,
    hardware_group: Controller<AdapterHardwareGroup>,
}

#[derive(Debug)]
pub enum AdapterDetailPageInput {
    PoweredToggled(bool),
    VisibleForPairingToggled(bool),
    AliasApplied(String),
    PairingVisibilityReported(PairingVisibilityGroupOutput),
}

#[derive(Debug)]
pub enum AdapterDetailPageOutput {
    Failed(BluetoothError),
    AdapterChanged(AdapterSnapshot),
}

#[derive(Debug)]
pub enum AdapterDetailPageCommand {
    AdapterChanged(AdapterSnapshot),
    ActionFinished(Result<AdapterSnapshot, BluetoothError>),
    AliasSaved(Result<AdapterSnapshot, BluetoothError>),
}

impl AdapterDetailPage {
    fn show_adapter(&mut self, adapter: AdapterSnapshot, sender: &ComponentSender<Self>) {
        self.alias_changed = adapter.alias != self.adapter.alias;
        self.adapter = adapter;
        self.pairing_visibility
            .emit(PairingVisibilityGroupInput::AdapterChanged(
                self.adapter.clone(),
            ));
        self.hardware_group
            .emit(AdapterHardwareGroupInput::AdapterChanged(
                self.adapter.clone(),
            ));
        let _app_gone = sender.output(AdapterDetailPageOutput::AdapterChanged(
            self.adapter.clone(),
        ));
    }
}

#[relm4::component(pub)]
impl Component for AdapterDetailPage {
    type Init = AdapterDetailPageInit;
    type Input = AdapterDetailPageInput;
    type Output = AdapterDetailPageOutput;
    type CommandOutput = AdapterDetailPageCommand;

    view! {
        #[template]
        ClampedPage {
            #[template_child]
            page_content {
                #[template]
                PageHeader {
                    #[template_child]
                    header_icon {
                        #[watch]
                        set_icon_name: Some(model.adapter.hardware.attachment.icon_name()),
                    },
                    #[template_child]
                    header_title {
                        #[watch]
                        set_label: &model.adapter.radio_label,
                    },
                },

                adw::PreferencesGroup {
                    set_title: "Radio",

                    adw::SwitchRow {
                        set_title: "Powered",
                        set_subtitle: "Off disables Bluetooth on this radio",
                        #[watch]
                        #[block_signal(powered_handler)]
                        set_active: model.adapter.powered,
                        connect_active_notify[sender] => move |switch_row| {
                            sender.input(AdapterDetailPageInput::PoweredToggled(switch_row.is_active()));
                        } @powered_handler,
                    },
                },

                #[local_ref]
                pairing_visibility_widget -> adw::PreferencesGroup {},

                adw::PreferencesGroup {
                    set_title: "Identity",

                    adw::EntryRow {
                        set_title: "Name Shown to Other Devices",
                        set_show_apply_button: true,
                        #[track(model.alias_changed)]
                        set_show_apply_button: false,
                        #[track(model.alias_changed)]
                        set_text: &model.adapter.alias,
                        #[track(model.alias_changed)]
                        set_show_apply_button: true,
                        #[watch]
                        set_editable: !model.alias_saving,
                        connect_apply[sender] => move |entry_row| {
                            sender.input(AdapterDetailPageInput::AliasApplied(entry_row.text().to_string()));
                        },

                        add_suffix = &gtk::Spinner {
                            set_tooltip_text: Some("Renaming…"),
                            #[watch]
                            set_spinning: model.alias_saving,
                            #[watch]
                            set_visible: model.alias_saving,
                        },
                    },

                    adw::ActionRow {
                        set_title: "Address",
                        set_subtitle_selectable: true,
                        add_css_class: "property",
                        #[watch]
                        set_subtitle: &model.adapter.address.to_string(),
                    },

                    adw::ActionRow {
                        set_title: "Address Type",
                        add_css_class: "property",
                        #[watch]
                        set_subtitle: &model.adapter.address_type.to_string(),
                    },
                },

                #[local_ref]
                hardware_group_widget -> adw::PreferencesGroup {},
            },
        }
    }

    fn init(
        page_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let pairing_visibility = PairingVisibilityGroup::builder()
            .launch(PairingVisibilityGroupInit {
                session: page_init.session.clone(),
                adapter: page_init.adapter.clone(),
            })
            .forward(
                sender.input_sender(),
                AdapterDetailPageInput::PairingVisibilityReported,
            );
        let hardware_group = AdapterHardwareGroup::builder()
            .launch(page_init.adapter.clone())
            .detach();
        let model = Self {
            session: page_init.session.clone(),
            adapter: page_init.adapter,
            alias_changed: true,
            alias_saving: false,
            pairing_visibility,
            hardware_group,
        };
        let pairing_visibility_widget = model.pairing_visibility.widget();
        let hardware_group_widget = model.hardware_group.widget();
        let widgets = view_output!();

        let session = page_init.session;
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
                                .emit(AdapterDetailPageCommand::AdapterChanged(adapter_snapshot));
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
        let session = self.session.clone();
        let adapter_name = self.adapter.adapter_name.clone();
        match input {
            AdapterDetailPageInput::PoweredToggled(powered) if powered != self.adapter.powered => {
                sender.oneshot_command(async move {
                    AdapterDetailPageCommand::ActionFinished(
                        set_powered(&session, &adapter_name, powered).await,
                    )
                });
            }
            AdapterDetailPageInput::PoweredToggled(_) => {}
            AdapterDetailPageInput::VisibleForPairingToggled(visible) => {
                self.pairing_visibility.emit(
                    PairingVisibilityGroupInput::VisibleForPairingToggled(visible),
                );
            }
            AdapterDetailPageInput::AliasApplied(entered_alias) => {
                let alias = entered_alias.trim().to_owned();
                if alias.is_empty() || alias == self.adapter.alias {
                    self.alias_changed = true;
                    return;
                }
                self.alias_saving = true;
                sender.oneshot_command(async move {
                    AdapterDetailPageCommand::AliasSaved(
                        set_alias(&session, &adapter_name, alias).await,
                    )
                });
            }
            AdapterDetailPageInput::PairingVisibilityReported(
                PairingVisibilityGroupOutput::AdapterChanged(adapter_snapshot),
            ) => self.show_adapter(adapter_snapshot, &sender),
            AdapterDetailPageInput::PairingVisibilityReported(
                PairingVisibilityGroupOutput::Failed(adapter_error),
            ) => {
                let _app_gone = sender.output(AdapterDetailPageOutput::Failed(adapter_error));
            }
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        self.alias_changed = false;
        match command_output {
            AdapterDetailPageCommand::AdapterChanged(adapter_snapshot)
            | AdapterDetailPageCommand::ActionFinished(Ok(adapter_snapshot)) => {
                self.show_adapter(adapter_snapshot, &sender);
            }
            AdapterDetailPageCommand::ActionFinished(Err(adapter_error)) => {
                let _app_gone = sender.output(AdapterDetailPageOutput::Failed(adapter_error));
            }
            AdapterDetailPageCommand::AliasSaved(Ok(adapter_snapshot)) => {
                self.alias_saving = false;
                self.show_adapter(adapter_snapshot, &sender);
            }
            AdapterDetailPageCommand::AliasSaved(Err(adapter_error)) => {
                self.alias_saving = false;
                self.alias_changed = true;
                let _app_gone = sender.output(AdapterDetailPageOutput::Failed(adapter_error));
            }
        }
    }
}
