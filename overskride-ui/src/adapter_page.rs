use std::time::{Duration, Instant};

use futures::StreamExt;
use overskride_bluetooth::adapter_state::{
    AdapterSnapshot, adapter_changes, set_alias, set_discoverable_timeout, set_powered,
    set_visible_for_pairing,
};
use overskride_bluetooth::bluetooth_error::BluetoothError;
use overskride_bluetooth::session::BluetoothSession;
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::prelude::*;

use crate::widgets::page_templates::{ClampedPage, PageHeader};
use crate::widgets::protocol_info::protocol_info_button;

const SECONDS_PER_MINUTE: u64 = 60;
const COUNTDOWN_TICK: Duration = Duration::from_secs(1);
const OFFERED_VISIBILITY_MINUTES: [u32; 4] = [0, 3, 5, 15];
const VISIBILITY_PROTOCOL_TERM: &str = "Inquiry scan + Pairable";
const VISIBILITY_PROTOCOL_DETAILS: &str = "Discoverable makes this radio answer the inquiry packets other devices send while they scan, so this computer appears in their lists under the name below. Pairable lets BlueZ accept the pairing requests they then send; the pairing agent of this app asks you to confirm the code. Turning visibility off keeps Pairable on, so bonded devices can still pair again. This is only needed when the other device starts the pairing; to pair a headset or controller, put it in pairing mode and use Scan instead.";

pub struct AdapterPageInit {
    pub session: BluetoothSession,
    pub adapter: AdapterSnapshot,
}

pub struct AdapterPage {
    session: BluetoothSession,
    adapter: AdapterSnapshot,
    alias_changed: bool,
    visibility_minute_choices: Vec<u32>,
    visibility_choices_changed: bool,
    visible_since: Option<Instant>,
}

#[derive(Debug)]
pub enum AdapterPageInput {
    PoweredToggled(bool),
    VisibleForPairingToggled(bool),
    VisibilityDurationChosen(u32),
    AliasApplied(String),
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
    VisibilityTick,
}

fn visible_for_pairing(adapter: &AdapterSnapshot) -> bool {
    adapter.discoverable && adapter.pairable
}

fn visibility_minute_choices(current_minutes: u32) -> Vec<u32> {
    let mut minute_choices = OFFERED_VISIBILITY_MINUTES.to_vec();
    if !minute_choices.contains(&current_minutes) {
        minute_choices.push(current_minutes);
        minute_choices.sort_unstable();
    }
    minute_choices
}

fn visibility_choice_label(minutes: u32) -> String {
    match minutes {
        0 => "Until Turned Off".to_string(),
        1 => "1 Minute".to_string(),
        many_minutes => format!("{many_minutes} Minutes"),
    }
}

fn visibility_choice_list(minute_choices: &[u32]) -> gtk::StringList {
    let choice_labels: Vec<String> = minute_choices
        .iter()
        .map(|minutes| visibility_choice_label(*minutes))
        .collect();
    let label_slices: Vec<&str> = choice_labels.iter().map(String::as_str).collect();
    gtk::StringList::new(&label_slices)
}

fn format_minutes_seconds(remaining: Duration) -> String {
    let remaining_seconds = remaining.as_secs();
    format!(
        "{}:{:02}",
        remaining_seconds / SECONDS_PER_MINUTE,
        remaining_seconds % SECONDS_PER_MINUTE
    )
}

fn describe_pairing_visibility(adapter: &AdapterSnapshot, remaining: Option<Duration>) -> String {
    if !adapter.discoverable {
        return "Hidden; already paired devices can still connect".to_string();
    }
    if !adapter.pairable {
        return "Visible, but pairing requests are rejected".to_string();
    }
    let alias = &adapter.alias;
    match (adapter.discoverable_timeout_minutes, remaining) {
        (0, _) => format!("Visible as “{alias}” until turned off"),
        (_, Some(remaining)) => format!(
            "Visible as “{alias}” for {} more",
            format_minutes_seconds(remaining)
        ),
        (timeout_minutes, None) => format!(
            "Visible as “{alias}”, hides within {}",
            visibility_choice_label(timeout_minutes).to_lowercase()
        ),
    }
}

impl AdapterPage {
    fn show_adapter(&mut self, adapter: AdapterSnapshot, sender: &ComponentSender<Self>) {
        self.alias_changed = adapter.alias != self.adapter.alias;
        let became_discoverable = adapter.discoverable && !self.adapter.discoverable;
        let timeout_changed =
            adapter.discoverable_timeout_minutes != self.adapter.discoverable_timeout_minutes;
        let minute_choices = visibility_minute_choices(adapter.discoverable_timeout_minutes);
        self.visibility_choices_changed = minute_choices != self.visibility_minute_choices;
        self.visibility_minute_choices = minute_choices;
        self.adapter = adapter;
        if !self.adapter.discoverable {
            self.visible_since = None;
        } else if became_discoverable || timeout_changed {
            self.start_visibility_countdown(sender);
        }
        let _app_gone = sender.output(AdapterPageOutput::AdapterChanged(self.adapter.clone()));
    }

    fn start_visibility_countdown(&mut self, sender: &ComponentSender<Self>) {
        self.visible_since = Some(Instant::now());
        let countdown_ticks =
            u64::from(self.adapter.discoverable_timeout_minutes) * SECONDS_PER_MINUTE;
        sender.command(move |command_output, shutdown| {
            shutdown
                .register(
                    futures::stream::iter(0..countdown_ticks)
                        .then(|_elapsed_seconds| tokio::time::sleep(COUNTDOWN_TICK))
                        .for_each(move |()| {
                            command_output.emit(AdapterPageCommand::VisibilityTick);
                            futures::future::ready(())
                        }),
                )
                .drop_on_shutdown()
        });
    }

    fn remaining_visibility(&self) -> Option<Duration> {
        let timeout_minutes = self.adapter.discoverable_timeout_minutes;
        let visible_since = self.visible_since?;
        (timeout_minutes > 0).then(|| {
            Duration::from_secs(u64::from(timeout_minutes) * SECONDS_PER_MINUTE)
                .saturating_sub(visible_since.elapsed())
        })
    }

    fn selected_visibility_choice(&self) -> u32 {
        self.visibility_minute_choices
            .iter()
            .position(|minutes| *minutes == self.adapter.discoverable_timeout_minutes)
            .and_then(|choice_position| u32::try_from(choice_position).ok())
            .unwrap_or_default()
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
                            sender.input(AdapterPageInput::PoweredToggled(switch_row.is_active()));
                        } @powered_handler,
                    },
                },

                adw::PreferencesGroup {
                    set_title: "Pairing From Other Devices",
                    set_description: Some("For phones and computers that start the pairing themselves. To pair a headset or controller, use Scan in the sidebar instead."),
                    #[watch]
                    set_sensitive: model.adapter.powered,

                    adw::SwitchRow {
                        set_title: "Visible for Pairing",
                        set_subtitle_lines: 0,
                        add_suffix: &protocol_info_button(VISIBILITY_PROTOCOL_TERM, VISIBILITY_PROTOCOL_DETAILS),
                        #[watch]
                        set_subtitle: &describe_pairing_visibility(&model.adapter, model.remaining_visibility()),
                        #[watch]
                        #[block_signal(visibility_handler)]
                        set_active: visible_for_pairing(&model.adapter),
                        connect_active_notify[sender] => move |switch_row| {
                            sender.input(AdapterPageInput::VisibleForPairingToggled(switch_row.is_active()));
                        } @visibility_handler,
                    },

                    adw::ComboRow {
                        set_title: "Stay Visible",
                        #[track(model.visibility_choices_changed)]
                        #[block_signal(duration_handler)]
                        set_model: Some(&visibility_choice_list(&model.visibility_minute_choices)),
                        #[watch]
                        #[block_signal(duration_handler)]
                        set_selected: model.selected_visibility_choice(),
                        connect_selected_notify[sender] => move |combo_row| {
                            sender.input(AdapterPageInput::VisibilityDurationChosen(combo_row.selected()));
                        } @duration_handler,
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
                            sender.input(AdapterPageInput::AliasApplied(entry_row.text().to_string()));
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

                adw::PreferencesGroup {
                    set_title: "Hardware",

                    adw::ActionRow {
                        set_title: "Connection",
                        add_css_class: "property",
                        #[watch]
                        set_subtitle: model.adapter.hardware.attachment.describe_attachment(),
                    },

                    adw::ActionRow {
                        set_title: "Vendor",
                        set_subtitle_selectable: true,
                        add_css_class: "property",
                        #[watch]
                        set_visible: model.adapter.hardware.vendor_name.is_some(),
                        #[watch]
                        set_subtitle: model.adapter.hardware.vendor_name.as_deref().unwrap_or_default(),
                    },

                    adw::ActionRow {
                        set_title: "Model",
                        set_subtitle_selectable: true,
                        add_css_class: "property",
                        #[watch]
                        set_visible: model.adapter.hardware.model_name.is_some(),
                        #[watch]
                        set_subtitle: model.adapter.hardware.model_name.as_deref().unwrap_or_default(),
                    },

                    adw::ActionRow {
                        set_title: "Kernel Driver",
                        set_subtitle_selectable: true,
                        add_css_class: "property",
                        #[watch]
                        set_visible: model.adapter.hardware.kernel_driver.is_some(),
                        #[watch]
                        set_subtitle: model.adapter.hardware.kernel_driver.as_deref().unwrap_or_default(),
                    },

                    adw::ActionRow {
                        set_title: "Hardware ID",
                        set_subtitle_selectable: true,
                        add_css_class: "property",
                        #[watch]
                        set_visible: model.adapter.hardware.hardware_id.is_some(),
                        #[watch]
                        set_subtitle: model.adapter.hardware.hardware_id.as_deref().unwrap_or_default(),
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
            visibility_minute_choices: visibility_minute_choices(
                page_init.adapter.discoverable_timeout_minutes,
            ),
            visibility_choices_changed: true,
            visible_since: None,
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
        self.visibility_choices_changed = false;
        let session = self.session.clone();
        let adapter_name = self.adapter.adapter_name.clone();
        match input {
            AdapterPageInput::PoweredToggled(powered) if powered != self.adapter.powered => {
                sender.oneshot_command(async move {
                    AdapterPageCommand::ActionFinished(
                        set_powered(&session, &adapter_name, powered).await,
                    )
                });
            }
            AdapterPageInput::VisibleForPairingToggled(visible)
                if visible != visible_for_pairing(&self.adapter) =>
            {
                sender.oneshot_command(async move {
                    AdapterPageCommand::ActionFinished(
                        set_visible_for_pairing(&session, &adapter_name, visible).await,
                    )
                });
            }
            AdapterPageInput::VisibilityDurationChosen(choice_position) => {
                let Some(timeout_minutes) = self
                    .visibility_minute_choices
                    .get(choice_position as usize)
                    .copied()
                else {
                    return;
                };
                if timeout_minutes == self.adapter.discoverable_timeout_minutes {
                    return;
                }
                sender.oneshot_command(async move {
                    AdapterPageCommand::ActionFinished(
                        set_discoverable_timeout(&session, &adapter_name, timeout_minutes).await,
                    )
                });
            }
            AdapterPageInput::AliasApplied(alias) => {
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
        self.visibility_choices_changed = false;
        match command_output {
            AdapterPageCommand::AdapterChanged(adapter_snapshot)
            | AdapterPageCommand::ActionFinished(Ok(adapter_snapshot)) => {
                self.show_adapter(adapter_snapshot, &sender);
            }
            AdapterPageCommand::ActionFinished(Err(adapter_error)) => {
                let _app_gone = sender.output(AdapterPageOutput::Failed(adapter_error));
            }
            AdapterPageCommand::VisibilityTick => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use overskride_bluetooth::radios::RadioHardware;

    fn visible_adapter() -> AdapterSnapshot {
        AdapterSnapshot {
            adapter_name: "hci0".to_string(),
            radio_label: "Test Radio".to_string(),
            hardware: RadioHardware::default(),
            address: overskride_bluetooth::Address::any(),
            address_type: overskride_bluetooth::AddressType::BrEdr,
            alias: "Ryzen".to_string(),
            powered: true,
            discoverable: true,
            pairable: true,
            discovering: false,
            discoverable_timeout_minutes: 3,
        }
    }

    #[test]
    fn hidden_adapter_says_paired_devices_still_connect() {
        let mut adapter = visible_adapter();
        adapter.discoverable = false;
        assert_eq!(
            describe_pairing_visibility(&adapter, None),
            "Hidden; already paired devices can still connect"
        );
    }

    #[test]
    fn visible_without_pairable_warns_about_rejection() {
        let mut adapter = visible_adapter();
        adapter.pairable = false;
        assert_eq!(
            describe_pairing_visibility(&adapter, None),
            "Visible, but pairing requests are rejected"
        );
    }

    #[test]
    fn zero_timeout_stays_visible_until_turned_off() {
        let mut adapter = visible_adapter();
        adapter.discoverable_timeout_minutes = 0;
        assert_eq!(
            describe_pairing_visibility(&adapter, Some(Duration::from_secs(5))),
            "Visible as “Ryzen” until turned off"
        );
    }

    #[test]
    fn known_start_shows_countdown() {
        assert_eq!(
            describe_pairing_visibility(&visible_adapter(), Some(Duration::from_secs(125))),
            "Visible as “Ryzen” for 2:05 more"
        );
    }

    #[test]
    fn unknown_start_shows_upper_bound() {
        assert_eq!(
            describe_pairing_visibility(&visible_adapter(), None),
            "Visible as “Ryzen”, hides within 3 minutes"
        );
    }

    #[test]
    fn unusual_timeout_becomes_extra_choice() {
        assert_eq!(visibility_minute_choices(10), vec![0, 3, 5, 10, 15]);
        assert_eq!(visibility_minute_choices(5), vec![0, 3, 5, 15]);
    }
}
