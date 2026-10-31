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
use relm4::prelude::*;

use crate::widgets::page_templates::{ClampedPage, PageHeader};

const MAXIMUM_TIMEOUT_MINUTES: u32 = 60;

pub struct AdapterPageInit {
    pub session: BluetoothSession,
    pub adapter: AdapterSnapshot,
}

pub struct AdapterPage {
    session: BluetoothSession,
    adapter: AdapterSnapshot,
    alias_changed: bool,
}

#[derive(Debug)]
pub enum AdapterPageInput {
    SetPowered(bool),
    SetDiscoverable(bool),
    SetDiscoverableTimeout(u32),
    SetAlias(String),
}

#[derive(Debug)]
pub enum AdapterPageOutput {
    Failed(BluetoothError),
    AdapterChanged(AdapterSnapshot),
}

#[derive(Debug)]
pub enum AdapterPageCommand {
    AdapterChanged(AdapterSnapshot),
    ActionFinished(Result<AdapterSnapshot, BluetoothError>),
}

fn whole_minutes(spin_value: f64) -> u32 {
    (0..=MAXIMUM_TIMEOUT_MINUTES)
        .find(|candidate_minutes| f64::from(*candidate_minutes) >= spin_value.round())
        .unwrap_or(MAXIMUM_TIMEOUT_MINUTES)
}

impl AdapterPage {
    fn show_adapter(&mut self, adapter: AdapterSnapshot, sender: &ComponentSender<Self>) {
        self.alias_changed = adapter.alias != self.adapter.alias;
        self.adapter = adapter;
        let _app_gone = sender.output(AdapterPageOutput::AdapterChanged(self.adapter.clone()));
    }
}

#[relm4::component(pub)]
impl Component for AdapterPage {
    type Init = AdapterPageInit;
    type Input = AdapterPageInput;
    type Output = AdapterPageOutput;
    type CommandOutput = AdapterPageCommand;

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
                        #[watch]
                        set_label: &model.adapter.radio_label,
                    },
                },

                adw::PreferencesGroup {
                    set_title: "Status",

                    adw::SwitchRow {
                        set_title: "Powered",
                        #[watch]
                        #[block_signal(powered_handler)]
                        set_active: model.adapter.powered,
                        connect_active_notify[sender] => move |switch_row| {
                            sender.input(AdapterPageInput::SetPowered(switch_row.is_active()));
                        } @powered_handler,
                    },

                    adw::SwitchRow {
                        set_title: "Discoverable",
                        set_subtitle: "Visible to other devices",
                        #[watch]
                        #[block_signal(discoverable_handler)]
                        set_active: model.adapter.discoverable,
                        connect_active_notify[sender] => move |switch_row| {
                            sender.input(AdapterPageInput::SetDiscoverable(switch_row.is_active()));
                        } @discoverable_handler,
                    },

                    adw::SpinRow {
                        set_title: "Discoverable Timeout",
                        set_subtitle: "In minutes, 0 means forever",
                        set_adjustment: Some(&gtk::Adjustment::new(0.0, 0.0, f64::from(MAXIMUM_TIMEOUT_MINUTES), 1.0, 5.0, 0.0)),
                        #[watch]
                        #[block_signal(timeout_handler)]
                        set_value: f64::from(model.adapter.discoverable_timeout_minutes),
                        connect_value_notify[sender] => move |spin_row| {
                            sender.input(AdapterPageInput::SetDiscoverableTimeout(whole_minutes(spin_row.value())));
                        } @timeout_handler,
                    },
                },

                adw::PreferencesGroup {
                    set_title: "Identity",

                    adw::EntryRow {
                        set_title: "Name Shown to Other Devices",
                        set_show_apply_button: true,
                        #[track(model.alias_changed)]
                        set_text: &model.adapter.alias,
                        connect_apply[sender] => move |entry_row| {
                            sender.input(AdapterPageInput::SetAlias(entry_row.text().to_string()));
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
                        set_title: "Kernel Name",
                        set_subtitle_selectable: true,
                        add_css_class: "property",
                        #[watch]
                        set_subtitle: &model.adapter.adapter_name,
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
        let model = Self {
            session: page_init.session.clone(),
            adapter: page_init.adapter,
            alias_changed: true,
        };
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
                                .emit(AdapterPageCommand::AdapterChanged(adapter_snapshot));
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
            AdapterPageInput::SetPowered(powered) if powered != self.adapter.powered => {
                sender.oneshot_command(async move {
                    AdapterPageCommand::ActionFinished(
                        set_powered(&session, &adapter_name, powered).await,
                    )
                });
            }
            AdapterPageInput::SetDiscoverable(discoverable)
                if discoverable != self.adapter.discoverable =>
            {
                sender.oneshot_command(async move {
                    AdapterPageCommand::ActionFinished(
                        set_discoverable(&session, &adapter_name, discoverable).await,
                    )
                });
            }
            AdapterPageInput::SetDiscoverableTimeout(timeout_minutes)
                if timeout_minutes != self.adapter.discoverable_timeout_minutes =>
            {
                sender.oneshot_command(async move {
                    AdapterPageCommand::ActionFinished(
                        set_discoverable_timeout(&session, &adapter_name, timeout_minutes).await,
                    )
                });
            }
            AdapterPageInput::SetAlias(alias) => {
                sender.oneshot_command(async move {
                    AdapterPageCommand::ActionFinished(
                        set_alias(&session, &adapter_name, alias).await,
                    )
                });
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
        match command_output {
            AdapterPageCommand::AdapterChanged(adapter_snapshot)
            | AdapterPageCommand::ActionFinished(Ok(adapter_snapshot)) => {
                self.show_adapter(adapter_snapshot, &sender);
            }
            AdapterPageCommand::ActionFinished(Err(adapter_error)) => {
                let _app_gone = sender.output(AdapterPageOutput::Failed(adapter_error));
            }
        }
    }
}
