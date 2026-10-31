use std::time::{Duration, Instant};

use bindrune_bluez::{
    bluetooth_adapter::adapter_snapshot_and_discovery::{
        AdapterSnapshot, set_discoverable_timeout, set_visible_for_pairing,
    },
    bluez_dbus_session::BluetoothSession,
    bluez_operation_error::BluetoothError,
};
use futures::StreamExt;
use relm4::{adw, adw::prelude::*, gtk, prelude::*};
use uom::{
    fmt::DisplayStyle,
    si::{
        f32::Time,
        time::{minute, second},
    },
};

use crate::shared_page_widget_templates::protocol_info_button;

const COUNTDOWN_TICK: Duration = Duration::from_secs(1);
const OFFERED_VISIBILITY_TIMEOUTS: [Duration; 4] = [
    Duration::ZERO,
    Duration::from_mins(3),
    Duration::from_mins(5),
    Duration::from_mins(15),
];
const VISIBILITY_PROTOCOL_TERM: &str = "Inquiry scan + Pairable";
const VISIBILITY_PROTOCOL_DETAILS: &str =
    "Discoverable makes this radio answer the inquiry packets other devices send while they scan, \
     so this computer appears in their lists under the name below. Pairable lets BlueZ accept the \
     pairing requests they then send; the pairing agent of this app asks you to confirm the code. \
     Turning visibility off keeps Pairable on, so bonded devices can still pair again. This is \
     only needed when the other device starts the pairing; to pair a headset or controller, put \
     it in pairing mode and use Scan instead.";

pub struct PairingVisibilityGroupInit {
    pub session: BluetoothSession,
    pub adapter: AdapterSnapshot,
}

pub struct PairingVisibilityGroup {
    session: BluetoothSession,
    adapter: AdapterSnapshot,
    visibility_timeout_choices: Vec<Duration>,
    visibility_choices_changed: bool,
    visible_since: Option<Instant>,
}

#[derive(Debug)]
pub enum PairingVisibilityGroupInput {
    AdapterChanged(AdapterSnapshot),
    VisibleForPairingToggled(bool),
    VisibilityDurationChosen(u32),
}

#[derive(Debug)]
pub enum PairingVisibilityGroupOutput {
    Failed(BluetoothError),
    AdapterChanged(AdapterSnapshot),
}

#[derive(Debug)]
pub enum PairingVisibilityGroupCommand {
    ActionFinished(Result<AdapterSnapshot, BluetoothError>),
    VisibilityTick,
}

fn visible_for_pairing(adapter: &AdapterSnapshot) -> bool {
    adapter.discoverable && adapter.pairable
}

fn visibility_timeout_choices(current_timeout: Duration) -> Vec<Duration> {
    let mut timeout_choices = OFFERED_VISIBILITY_TIMEOUTS.to_vec();
    if !timeout_choices.contains(&current_timeout) {
        timeout_choices.push(current_timeout);
        timeout_choices.sort_unstable();
    }
    timeout_choices
}

fn visibility_choice_label(visibility_timeout: Duration) -> String {
    if visibility_timeout.is_zero() {
        return "Until turned off".to_string();
    }
    Time::new::<second>(visibility_timeout.as_secs_f32())
        .into_format_args(minute, DisplayStyle::Description)
        .to_string()
}

fn visibility_choice_list(timeout_choices: &[Duration]) -> gtk::StringList {
    let choice_labels: Vec<String> = timeout_choices
        .iter()
        .map(|visibility_timeout| visibility_choice_label(*visibility_timeout))
        .collect();
    let label_slices: Vec<&str> = choice_labels.iter().map(String::as_str).collect();
    gtk::StringList::new(&label_slices)
}

fn format_whole_seconds(remaining: Duration) -> String {
    humantime::format_duration(Duration::from_secs(remaining.as_secs())).to_string()
}

fn describe_pairing_visibility(adapter: &AdapterSnapshot, remaining: Option<Duration>) -> String {
    if !adapter.discoverable {
        return "Hidden; already paired devices can still connect".to_string();
    }
    if !adapter.pairable {
        return "Visible, but pairing requests are rejected".to_string();
    }
    let alias = &adapter.alias;
    match (adapter.discoverable_timeout, remaining) {
        (visibility_timeout, _) if visibility_timeout.is_zero() => {
            format!("Visible as “{alias}” until turned off")
        }
        (_, Some(remaining)) => format!(
            "Visible as “{alias}” for {} more",
            format_whole_seconds(remaining)
        ),
        (visibility_timeout, None) => format!(
            "Visible as “{alias}”, hides within {}",
            visibility_choice_label(visibility_timeout).to_lowercase()
        ),
    }
}

impl PairingVisibilityGroup {
    fn show_adapter(&mut self, adapter: AdapterSnapshot, sender: &ComponentSender<Self>) {
        let became_discoverable = adapter.discoverable && !self.adapter.discoverable;
        let timeout_changed = adapter.discoverable_timeout != self.adapter.discoverable_timeout;
        let timeout_choices = visibility_timeout_choices(adapter.discoverable_timeout);
        self.visibility_choices_changed = timeout_choices != self.visibility_timeout_choices;
        self.visibility_timeout_choices = timeout_choices;
        self.adapter = adapter;
        if !self.adapter.discoverable {
            self.visible_since = None;
        } else if became_discoverable || timeout_changed {
            self.start_visibility_countdown(sender);
        }
    }

    fn start_visibility_countdown(&mut self, sender: &ComponentSender<Self>) {
        self.visible_since = Some(Instant::now());
        let countdown_ticks = self.adapter.discoverable_timeout.as_secs();
        sender.command(move |command_output, shutdown| {
            shutdown
                .register(
                    futures::stream::iter(0..countdown_ticks)
                        .then(|_elapsed_seconds| tokio::time::sleep(COUNTDOWN_TICK))
                        .for_each(move |()| {
                            command_output.emit(PairingVisibilityGroupCommand::VisibilityTick);
                            futures::future::ready(())
                        }),
                )
                .drop_on_shutdown()
        });
    }

    fn remaining_visibility(&self) -> Option<Duration> {
        let visibility_timeout = self.adapter.discoverable_timeout;
        let visible_since = self.visible_since?;
        (!visibility_timeout.is_zero())
            .then(|| visibility_timeout.saturating_sub(visible_since.elapsed()))
    }

    fn selected_visibility_choice(&self) -> u32 {
        self.visibility_timeout_choices
            .iter()
            .position(|timeout_choice| *timeout_choice == self.adapter.discoverable_timeout)
            .and_then(|choice_position| u32::try_from(choice_position).ok())
            .unwrap_or_default()
    }
}

#[relm4::component(pub)]
impl Component for PairingVisibilityGroup {
    type Init = PairingVisibilityGroupInit;
    type Input = PairingVisibilityGroupInput;
    type Output = PairingVisibilityGroupOutput;
    type CommandOutput = PairingVisibilityGroupCommand;

    view! {
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
                    sender.input(PairingVisibilityGroupInput::VisibleForPairingToggled(switch_row.is_active()));
                } @visibility_handler,
            },

            adw::ComboRow {
                set_title: "Stay Visible",
                #[track(model.visibility_choices_changed)]
                #[block_signal(duration_handler)]
                set_model: Some(&visibility_choice_list(&model.visibility_timeout_choices)),
                #[watch]
                #[block_signal(duration_handler)]
                set_selected: model.selected_visibility_choice(),
                connect_selected_notify[sender] => move |combo_row| {
                    sender.input(PairingVisibilityGroupInput::VisibilityDurationChosen(combo_row.selected()));
                } @duration_handler,
            },
        }
    }

    fn init(
        group_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {
            session: group_init.session,
            visibility_timeout_choices: visibility_timeout_choices(
                group_init.adapter.discoverable_timeout,
            ),
            visibility_choices_changed: true,
            visible_since: None,
            adapter: group_init.adapter,
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        self.visibility_choices_changed = false;
        let session = self.session.clone();
        let adapter_name = self.adapter.adapter_name.clone();
        match input {
            PairingVisibilityGroupInput::AdapterChanged(adapter_snapshot) => {
                self.show_adapter(adapter_snapshot, &sender);
            }
            PairingVisibilityGroupInput::VisibleForPairingToggled(visible)
                if visible != visible_for_pairing(&self.adapter) =>
            {
                sender.oneshot_command(async move {
                    PairingVisibilityGroupCommand::ActionFinished(
                        set_visible_for_pairing(&session, &adapter_name, visible).await,
                    )
                });
            }
            PairingVisibilityGroupInput::VisibilityDurationChosen(choice_position) => {
                let Some(chosen_timeout) = self
                    .visibility_timeout_choices
                    .get(choice_position as usize)
                    .copied()
                else {
                    return;
                };
                if chosen_timeout == self.adapter.discoverable_timeout {
                    return;
                }
                sender.oneshot_command(async move {
                    PairingVisibilityGroupCommand::ActionFinished(
                        set_discoverable_timeout(&session, &adapter_name, chosen_timeout).await,
                    )
                });
            }
            PairingVisibilityGroupInput::VisibleForPairingToggled(_) => {}
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        self.visibility_choices_changed = false;
        match command_output {
            PairingVisibilityGroupCommand::ActionFinished(Ok(adapter_snapshot)) => {
                self.show_adapter(adapter_snapshot.clone(), &sender);
                let _page_gone = sender.output(PairingVisibilityGroupOutput::AdapterChanged(
                    adapter_snapshot,
                ));
            }
            PairingVisibilityGroupCommand::ActionFinished(Err(adapter_error)) => {
                let _page_gone = sender.output(PairingVisibilityGroupOutput::Failed(adapter_error));
            }
            PairingVisibilityGroupCommand::VisibilityTick => {}
        }
    }
}
#[cfg(test)]
mod tests {
    use bindrune_bluez::bluetooth_adapter::adapter_udev_hardware::RadioHardware;

    use super::*;

    fn visible_adapter() -> AdapterSnapshot {
        AdapterSnapshot {
            adapter_name: "hci0".to_string(),
            radio_label: "Test Radio".to_string(),
            hardware: RadioHardware::default(),
            address: bindrune_bluez::Address::any(),
            address_type: bindrune_bluez::AddressType::BrEdr,
            alias: "Ryzen".to_string(),
            powered: true,
            discoverable: true,
            pairable: true,
            discovering: false,
            discoverable_timeout: Duration::from_mins(3),
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
        adapter.discoverable_timeout = Duration::ZERO;
        assert_eq!(
            describe_pairing_visibility(&adapter, Some(Duration::from_secs(5))),
            "Visible as “Ryzen” until turned off"
        );
    }

    #[test]
    fn known_start_shows_countdown() {
        assert_eq!(
            describe_pairing_visibility(&visible_adapter(), Some(Duration::from_secs(125))),
            "Visible as “Ryzen” for 2m 5s more"
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
        let minutes = |minute_values: &[u64]| -> Vec<Duration> {
            minute_values
                .iter()
                .map(|minute_value| Duration::from_mins(*minute_value))
                .collect()
        };
        assert_eq!(
            visibility_timeout_choices(Duration::from_mins(10)),
            minutes(&[0, 3, 5, 10, 15])
        );
        assert_eq!(
            visibility_timeout_choices(Duration::from_mins(5)),
            minutes(&[0, 3, 5, 15])
        );
    }
}
