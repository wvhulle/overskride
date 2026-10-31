use overskride_bluetooth::device_snapshot::{
    BatteryReading, DeviceClass, DeviceSnapshot, DistanceEstimate, SignalStrength,
};
use overskride_bluetooth::kernel_input_devices::{KernelInputBinding, KernelInputDevices};
use overskride_bluetooth::pairing_failure::PairingFailure;
use overskride_bluetooth::pairing_steps::{
    ConnectionPolicy, PAIRING_STEP_DEFINITIONS, PairingStep, applicable_steps, connection_policy,
    first_missing_step, is_step_applicable, is_step_done, step_definition,
};
use overskride_bluetooth::radio_bonds::{RadioBond, RadioMembership};
use uom::fmt::DisplayStyle;
use uom::si::length::meter;

use crate::device_status::{SignalQuality, input_device_missing};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivePairing {
    Idle,
    Running(PairingStep),
    Failed(PairingFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepNodeState {
    Done,
    Next,
    Running,
    Warning,
    Failed,
    Pending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepButtonAction {
    Pair,
    PairAgain,
    Connect,
    Disconnect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineAction {
    Step(StepButtonAction),
    ForgetDevice,
    SetConnectionPolicy(ConnectionPolicy),
    ShowRadio(String),
    PairViaRadio(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepButton {
    pub action: StepButtonAction,
    pub label: &'static str,
    pub sensitive: bool,
    pub suggested: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepButtonSlot {
    Hidden,
    Shown(StepButton),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicySelector {
    Absent,
    Present {
        selected: ConnectionPolicy,
        sensitive: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertyHint {
    Absent,
    Shown(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertyRow {
    Inline {
        key: String,
        value: String,
        hint: PropertyHint,
    },
    InlineWithIcon {
        key: String,
        value: String,
        hint: PropertyHint,
        icon_name: String,
    },
    Stacked {
        key: String,
        value: String,
        hint: PropertyHint,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepPresentation {
    pub step: PairingStep,
    pub title: &'static str,
    pub protocol_term: &'static str,
    pub protocol_details: &'static str,
    pub visible: bool,
    pub sensitive: bool,
    pub node_state: StepNodeState,
    pub status_text: String,
    pub button: StepButtonSlot,
    pub policy_selector: PolicySelector,
    pub property_rows: Vec<PropertyRow>,
    pub needs_attention: bool,
    pub offers_forget: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RadioControl {
    Shown,
    Show(String),
    PairVia(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioRowPresentation {
    pub radio_label: String,
    pub state_text: &'static str,
    pub may_be_stale: bool,
    pub control: RadioControl,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioSectionPresentation {
    pub visible: bool,
    pub shown_radio_label: String,
    pub also_bonded_via: String,
    pub rows: Vec<RadioRowPresentation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelinePresentation {
    pub summary: String,
    pub completed_steps: usize,
    pub applicable_step_count: usize,
    pub steps: Vec<StepPresentation>,
    pub radios: RadioSectionPresentation,
}

pub struct TimelineInputs<'inputs> {
    pub snapshot: &'inputs DeviceSnapshot,
    pub radio_bonds: &'inputs [RadioBond],
    pub active_pairing: &'inputs ActivePairing,
}

fn inline_property(key: &str, value: impl Into<String>) -> PropertyRow {
    PropertyRow::Inline {
        key: key.to_string(),
        value: value.into(),
        hint: PropertyHint::Absent,
    }
}

fn hinted_property(key: &str, value: impl Into<String>, hint_text: &str) -> PropertyRow {
    PropertyRow::Inline {
        key: key.to_string(),
        value: value.into(),
        hint: PropertyHint::Shown(hint_text.to_string()),
    }
}

fn stacked_property(key: &str, value: impl Into<String>) -> PropertyRow {
    PropertyRow::Stacked {
        key: key.to_string(),
        value: value.into(),
        hint: PropertyHint::Absent,
    }
}

fn capitalize_first_letter(text: &str) -> String {
    let mut characters = text.chars();
    characters
        .next()
        .map_or_else(String::new, |first_character| {
            first_character
                .to_uppercase()
                .chain(characters.flat_map(char::to_lowercase))
                .collect()
        })
}

impl TimelineInputs<'_> {
    fn is_running(&self, step: PairingStep) -> bool {
        *self.active_pairing == ActivePairing::Running(step)
    }

    fn is_busy(&self) -> bool {
        matches!(self.active_pairing, ActivePairing::Running(_))
    }

    fn shown_radio(&self) -> Option<&RadioBond> {
        self.radio_bonds.iter().find(|radio_bond| radio_bond.shown)
    }

    fn shown_radio_label(&self) -> String {
        self.shown_radio()
            .map(|radio_bond| radio_bond.radio.radio_label.clone())
            .unwrap_or_default()
    }

    fn summary(&self) -> String {
        let snapshot = self.snapshot;
        match self.active_pairing {
            ActivePairing::Running(running_step) => {
                return format!(
                    "Working on {}…",
                    step_definition(*running_step).title.to_lowercase()
                );
            }
            ActivePairing::Failed(pairing_failure) => {
                return format!("{} failed.", step_definition(pairing_failure.step()).title);
            }
            ActivePairing::Idle => {}
        }
        let summary_sentence = if !snapshot.paired {
            "Not paired yet."
        } else if !snapshot.bonded {
            "Paired, but no link key was stored, so trust can't be saved."
        } else if snapshot.blocked {
            "Blocked. Every connection is rejected."
        } else if !snapshot.trusted {
            "Bonded. Each connection needs approval."
        } else if !snapshot.connected && in_range(snapshot) {
            "Bonded and trusted. In range, but not connected."
        } else if !snapshot.connected {
            "Bonded and trusted. Reconnects when turned on."
        } else if input_device_missing(snapshot) {
            "Connected, but no input device appeared."
        } else {
            "Connected and ready."
        };
        summary_sentence.to_string()
    }

    fn missing_prerequisite(&self, step: PairingStep) -> Option<PairingStep> {
        let prerequisite_step = match step {
            PairingStep::Pairing | PairingStep::Signal => return None,
            PairingStep::Bonding | PairingStep::Connection => PairingStep::Pairing,
            PairingStep::Authorization => PairingStep::Bonding,
            PairingStep::InputDevice => PairingStep::Connection,
        };
        let step_is_active = self.is_running(step)
            || matches!(self.active_pairing, ActivePairing::Failed(pairing_failure) if pairing_failure.step() == step);
        let blocked_by_prerequisite = !is_step_done(self.snapshot, prerequisite_step)
            && !is_step_done(self.snapshot, step)
            && !step_is_active;
        blocked_by_prerequisite.then_some(prerequisite_step)
    }

    fn default_node_state(
        &self,
        step: PairingStep,
        first_missing: Option<PairingStep>,
    ) -> StepNodeState {
        if is_step_done(self.snapshot, step) {
            StepNodeState::Done
        } else if first_missing == Some(step) {
            StepNodeState::Next
        } else {
            StepNodeState::Pending
        }
    }

    fn button(
        &self,
        action: StepButtonAction,
        label: &'static str,
        sensitive: bool,
        suggested: bool,
    ) -> StepButtonSlot {
        StepButtonSlot::Shown(StepButton {
            action,
            label,
            sensitive: sensitive && !self.is_busy(),
            suggested,
        })
    }

    fn pairing_step(
        &self,
        default_state: StepNodeState,
        first_missing: Option<PairingStep>,
    ) -> StepParts {
        let snapshot = self.snapshot;
        let status_text = if snapshot.paired {
            "Link key generated"
        } else if snapshot.is_gamepad() {
            "Hold PS + Create until the light flashes, then press Pair"
        } else {
            "Put the device in pairing mode, then press Pair"
        };
        let address_hint = format!(
            "{} address",
            capitalize_first_letter(&snapshot.address_type)
        );
        let method_row = if snapshot.legacy_pairing {
            hinted_property("Method", "Legacy PIN pairing", "Older, weaker method")
        } else {
            inline_property("Method", "Secure Simple Pairing")
        };
        let class_rows = match snapshot.device_class {
            DeviceClass::Reported(device_class) => vec![inline_property(
                "Device class",
                format!("0x{device_class:06x}"),
            )],
            DeviceClass::Unknown => Vec::new(),
        };
        StepParts {
            node_state: default_state,
            status_text: status_text.to_string(),
            button: if snapshot.paired {
                StepButtonSlot::Hidden
            } else {
                self.button(
                    StepButtonAction::Pair,
                    "Pair",
                    true,
                    first_missing == Some(PairingStep::Pairing),
                )
            },
            property_rows: [
                method_row,
                hinted_property("Address", snapshot.address().to_string(), &address_hint),
            ]
            .into_iter()
            .chain(class_rows)
            .collect(),
        }
    }

    fn bonding_step(&self, default_state: StepNodeState) -> StepParts {
        let snapshot = self.snapshot;
        let bond_missing = snapshot.paired && !snapshot.bonded;
        let status_text = if snapshot.bonded {
            "Link key stored, survives power-off and reboot"
        } else if bond_missing {
            "No link key stored, pair again with the device in pairing mode"
        } else {
            "Happens together with pairing"
        };
        let storage_rows = self
            .shown_radio()
            .filter(|_shown_radio| snapshot.bonded)
            .map(|shown_radio| {
                stacked_property(
                    "Stored in",
                    format!(
                        "/var/lib/bluetooth/{}/{}/info",
                        shown_radio.adapter_address,
                        snapshot.address()
                    ),
                )
            });
        StepParts {
            node_state: if bond_missing {
                StepNodeState::Warning
            } else {
                default_state
            },
            status_text: status_text.to_string(),
            button: if snapshot.paired {
                self.button(
                    StepButtonAction::PairAgain,
                    "Pair Again",
                    true,
                    bond_missing,
                )
            } else {
                StepButtonSlot::Hidden
            },
            property_rows: storage_rows.into_iter().collect(),
        }
    }

    fn authorization_step(&self, default_state: StepNodeState) -> StepParts {
        let snapshot = self.snapshot;
        let policy = connection_policy(snapshot);
        let status_text = match (policy, snapshot.bonded) {
            (ConnectionPolicy::Blocked, _) => "Every connection is rejected",
            (ConnectionPolicy::Trusted, true) => "Connects without asking",
            (ConnectionPolicy::Trusted, false) => {
                "Connects without asking until it disconnects (no bond)"
            }
            (ConnectionPolicy::AskEachTime, true) => "The pairing agent approves each connection",
            (ConnectionPolicy::AskEachTime, false) => "Needs a bond before trust can be saved",
        };
        let saved_row = if snapshot.bonded {
            inline_property("Saved", "Yes")
        } else {
            hinted_property(
                "Saved",
                "No",
                "Only kept while connected, because there is no bond",
            )
        };
        StepParts {
            node_state: if snapshot.blocked {
                StepNodeState::Warning
            } else {
                default_state
            },
            status_text: status_text.to_string(),
            button: StepButtonSlot::Hidden,
            property_rows: vec![saved_row],
        }
    }

    fn battery_rows(&self) -> Vec<PropertyRow> {
        let snapshot = self.snapshot;
        if !snapshot.connected {
            return Vec::new();
        }
        let BatteryReading::Percentage(battery_percent) = snapshot.battery() else {
            return vec![inline_property("Battery", "Not reported")];
        };
        let battery_source = match snapshot.reported_battery {
            BatteryReading::Percentage(_) => "Reported by BlueZ",
            BatteryReading::Unknown => "Reported by the input driver",
        };
        let rounded_battery_level = (u16::from(battery_percent.min(100)) + 5) / 10 * 10;
        vec![PropertyRow::InlineWithIcon {
            key: "Battery".to_string(),
            value: format!("{battery_percent}%"),
            hint: PropertyHint::Shown(battery_source.to_string()),
            icon_name: format!("battery-level-{rounded_battery_level}-symbolic"),
        }]
    }

    fn signal_step(&self, default_state: StepNodeState) -> StepParts {
        let snapshot = self.snapshot;
        let (status_text, property_rows) = match snapshot.signal_strength {
            SignalStrength::Measured(rssi) => {
                let signal_quality = SignalQuality::from_rssi(rssi);
                (
                    format!("{} signal, in range", signal_quality.quality_label()),
                    vec![
                        PropertyRow::InlineWithIcon {
                            key: "Signal strength".to_string(),
                            value: format!("{rssi} dBm"),
                            hint: PropertyHint::Shown("Measured while scanning".to_string()),
                            icon_name: signal_quality.quality_icon_name().to_string(),
                        },
                        distance_row(snapshot.distance_estimate()),
                    ],
                )
            }
            SignalStrength::Unknown if snapshot.connected => {
                ("Connected, strength not reported".to_string(), Vec::new())
            }
            SignalStrength::Unknown => (
                "Not measured: press Scan to check whether it is nearby".to_string(),
                Vec::new(),
            ),
        };
        StepParts {
            node_state: if default_state == StepNodeState::Next && self.is_busy() {
                StepNodeState::Pending
            } else {
                default_state
            },
            status_text,
            button: StepButtonSlot::Hidden,
            property_rows,
        }
    }

    fn connection_step(&self, default_state: StepNodeState) -> StepParts {
        let snapshot = self.snapshot;
        let waiting_for_device = !snapshot.connected && snapshot.bonded && snapshot.trusted;
        let ready_to_connect = waiting_for_device && in_range(snapshot);
        let status_text = if snapshot.connected {
            "Connected"
        } else if ready_to_connect {
            "In range: press Connect if it doesn't connect by itself"
        } else if waiting_for_device {
            "Waiting: turn the device on or bring it closer"
        } else {
            "Not connected"
        };
        let button = if snapshot.connected {
            self.button(
                StepButtonAction::Disconnect,
                "Disconnect",
                snapshot.paired,
                false,
            )
        } else {
            self.button(
                StepButtonAction::Connect,
                "Connect",
                snapshot.paired,
                ready_to_connect,
            )
        };
        StepParts {
            node_state: if ready_to_connect {
                StepNodeState::Next
            } else {
                default_state
            },
            status_text: status_text.to_string(),
            button,
            property_rows: self.battery_rows(),
        }
    }

    fn input_device_step(&self, default_state: StepNodeState) -> StepParts {
        let snapshot = self.snapshot;
        let (status_text, property_rows) = match &snapshot.kernel_input {
            KernelInputBinding::Bound(kernel_input_devices) => {
                let input_count = kernel_input_devices.input_nodes.len();
                let plural_suffix = if input_count == 1 { "" } else { "s" };
                (
                    format!("{input_count} input device{plural_suffix}"),
                    kernel_input_rows(kernel_input_devices),
                )
            }
            KernelInputBinding::Absent if snapshot.connected => (
                "No input device".to_string(),
                vec![hinted_property(
                    "Input devices",
                    "None",
                    "Check the kernel log (dmesg)",
                )],
            ),
            KernelInputBinding::Absent => (
                "Not connected".to_string(),
                vec![hinted_property(
                    "Input devices",
                    "None yet",
                    "Created automatically once connected",
                )],
            ),
        };
        StepParts {
            node_state: if input_device_missing(snapshot) {
                StepNodeState::Warning
            } else {
                default_state
            },
            status_text,
            button: StepButtonSlot::Hidden,
            property_rows,
        }
    }

    fn step_parts(&self, step: PairingStep, first_missing: Option<PairingStep>) -> StepParts {
        let default_state = self.default_node_state(step, first_missing);
        let mut step_parts = match step {
            PairingStep::Pairing => self.pairing_step(default_state, first_missing),
            PairingStep::Bonding => self.bonding_step(default_state),
            PairingStep::Authorization => self.authorization_step(default_state),
            PairingStep::Signal => self.signal_step(default_state),
            PairingStep::Connection => self.connection_step(default_state),
            PairingStep::InputDevice => self.input_device_step(default_state),
        };
        match self.active_pairing {
            ActivePairing::Running(running_step) if *running_step == step => {
                step_parts.node_state = StepNodeState::Running;
            }
            ActivePairing::Failed(pairing_failure) if pairing_failure.step() == step => {
                step_parts.node_state = StepNodeState::Failed;
                step_parts.status_text = pairing_failure.to_string();
            }
            _ => {}
        }
        if let StepButtonSlot::Shown(step_button) = &mut step_parts.button {
            step_button.suggested |= step_parts.node_state == StepNodeState::Warning;
        }
        step_parts
    }

    fn present_step(
        &self,
        step: PairingStep,
        first_missing: Option<PairingStep>,
    ) -> StepPresentation {
        let definition = step_definition(step);
        let step_parts = self.step_parts(step, first_missing);
        let missing_prerequisite = self.missing_prerequisite(step);
        let status_text = match missing_prerequisite {
            Some(prerequisite_step) => format!(
                "Needs {}",
                step_definition(prerequisite_step).title.to_lowercase()
            ),
            None => step_parts.status_text,
        };
        StepPresentation {
            step,
            title: definition.title,
            protocol_term: definition.protocol_term,
            protocol_details: definition.protocol_details,
            visible: is_step_applicable(self.snapshot, step),
            sensitive: missing_prerequisite.is_none(),
            node_state: step_parts.node_state,
            status_text,
            button: step_parts.button,
            policy_selector: if step == PairingStep::Authorization {
                PolicySelector::Present {
                    selected: connection_policy(self.snapshot),
                    sensitive: self.snapshot.paired && !self.is_busy(),
                }
            } else {
                PolicySelector::Absent
            },
            needs_attention: is_step_applicable(self.snapshot, step)
                && matches!(
                    step_parts.node_state,
                    StepNodeState::Next
                        | StepNodeState::Running
                        | StepNodeState::Failed
                        | StepNodeState::Warning
                ),
            offers_forget: step == PairingStep::Bonding && self.snapshot.paired,
            property_rows: step_parts.property_rows,
        }
    }

    fn present_radios(&self) -> RadioSectionPresentation {
        let connected_elsewhere = |radio_bond: &RadioBond| {
            self.radio_bonds.iter().any(|other_bond| {
                matches!(other_bond.membership, RadioMembership::Known(other_state) if other_state.connected)
                    && other_bond.radio.adapter_name != radio_bond.radio.adapter_name
            })
        };
        let also_bonded_labels: Vec<_> = self
            .radio_bonds
            .iter()
            .filter(|radio_bond| !radio_bond.shown && radio_bond.is_bonded())
            .map(|radio_bond| radio_bond.radio.radio_label.clone())
            .collect();
        RadioSectionPresentation {
            visible: self.radio_bonds.len() > 1,
            shown_radio_label: self.shown_radio_label(),
            also_bonded_via: also_bonded_labels.join(", "),
            rows: self
                .radio_bonds
                .iter()
                .map(|radio_bond| RadioRowPresentation {
                    radio_label: radio_bond.radio.radio_label.clone(),
                    state_text: describe_radio_membership(radio_bond.membership),
                    may_be_stale: radio_bond.is_bonded()
                        && !matches!(radio_bond.membership, RadioMembership::Known(state) if state.connected)
                        && connected_elsewhere(radio_bond),
                    control: match (radio_bond.shown, radio_bond.membership) {
                        (true, _) => RadioControl::Shown,
                        (false, RadioMembership::Known(_)) => RadioControl::Show(radio_bond.radio.adapter_name.clone()),
                        (false, RadioMembership::Unknown) => RadioControl::PairVia(radio_bond.radio.adapter_name.clone()),
                    },
                })
                .collect(),
        }
    }

    pub fn present(&self) -> TimelinePresentation {
        let first_missing = first_missing_step(self.snapshot);
        let applicable: Vec<_> = applicable_steps(self.snapshot).collect();
        let completed_steps = first_missing
            .and_then(|missing_step| applicable.iter().position(|step| *step == missing_step))
            .unwrap_or(applicable.len());
        TimelinePresentation {
            summary: self.summary(),
            completed_steps,
            applicable_step_count: applicable.len(),
            steps: PAIRING_STEP_DEFINITIONS
                .iter()
                .map(|definition| self.present_step(definition.step, first_missing))
                .collect(),
            radios: self.present_radios(),
        }
    }
}

struct StepParts {
    node_state: StepNodeState,
    status_text: String,
    button: StepButtonSlot,
    property_rows: Vec<PropertyRow>,
}

fn in_range(snapshot: &DeviceSnapshot) -> bool {
    matches!(snapshot.signal_strength, SignalStrength::Measured(_))
}

fn distance_row(distance_estimate: DistanceEstimate) -> PropertyRow {
    match distance_estimate {
        DistanceEstimate::Estimated(distance) => hinted_property(
            "Distance",
            format!(
                "≈ {:.1}",
                distance.into_format_args(meter, DisplayStyle::Abbreviation)
            ),
            "Rough estimate from signal strength",
        ),
        DistanceEstimate::Unknown => inline_property("Distance", "Unknown"),
    }
}

fn describe_radio_membership(membership: RadioMembership) -> &'static str {
    match membership {
        RadioMembership::Unknown => "Doesn't know this device",
        RadioMembership::Known(state) if state.connected => "Connected",
        RadioMembership::Known(state) if !state.bonded => "Seen, not bonded",
        RadioMembership::Known(state) if state.trusted => "Bonded, trusted",
        RadioMembership::Known(_) => "Bonded, not trusted",
    }
}

fn kernel_input_rows(kernel_input_devices: &KernelInputDevices) -> Vec<PropertyRow> {
    let main_input_name = kernel_input_devices
        .input_nodes
        .first()
        .map(|input_node| input_node.input_name.clone())
        .unwrap_or_default();
    let node_rows = kernel_input_devices.input_nodes.iter().map(|input_node| {
        let input_label = input_node
            .input_name
            .strip_prefix(main_input_name.as_str())
            .map(str::trim)
            .filter(|name_suffix| !name_suffix.is_empty())
            .unwrap_or("Main");
        let device_paths: Vec<_> = input_node
            .device_nodes
            .iter()
            .map(|device_node| format!("/dev/input/{device_node}"))
            .collect();
        stacked_property(input_label, device_paths.join(", "))
    });
    std::iter::once(hinted_property(
        "Driver",
        kernel_input_devices.driver_name.clone(),
        "Linux HID driver",
    ))
    .chain(node_rows)
    .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use overskride_bluetooth::device_guess::DeviceAppearance;
    use overskride_bluetooth::device_snapshot::{Manufacturer, TransmitPower};
    use overskride_bluetooth::session::DeviceLocation;

    use overskride_bluetooth::pairing_failure::BondingFailure;

    use super::*;

    fn gamepad_snapshot() -> DeviceSnapshot {
        DeviceSnapshot {
            location: DeviceLocation {
                adapter_name: "hci0".to_string(),
                address: overskride_bluetooth::Address::new([1, 2, 3, 4, 5, 6]),
            },
            alias: "DualSense".to_string(),
            icon_name: "input-gaming".to_string(),
            signal_strength: SignalStrength::Unknown,
            transmit_power: TransmitPower::Unknown,
            reported_battery: BatteryReading::Unknown,
            connected: false,
            paired: false,
            bonded: false,
            trusted: false,
            blocked: false,
            legacy_pairing: false,
            address_type: "public".to_string(),
            device_class: DeviceClass::Unknown,
            services: BTreeSet::new(),
            manufacturer: Manufacturer::Unknown,
            manufacturer_data: HashMap::new(),
            service_data: HashMap::new(),
            appearance: DeviceAppearance::Unknown,
            kernel_input: KernelInputBinding::Absent,
        }
    }

    fn present(snapshot: &DeviceSnapshot, active_pairing: &ActivePairing) -> TimelinePresentation {
        TimelineInputs {
            snapshot,
            radio_bonds: &[],
            active_pairing,
        }
        .present()
    }

    fn step_of(presentation: &TimelinePresentation, step: PairingStep) -> &StepPresentation {
        presentation
            .steps
            .iter()
            .find(|step_presentation| step_presentation.step == step)
            .expect("every step is presented")
    }

    #[test]
    fn unpaired_gamepad_suggests_pairing_with_gamepad_hint() {
        let presentation = present(&gamepad_snapshot(), &ActivePairing::Idle);
        let pairing = step_of(&presentation, PairingStep::Pairing);
        assert_eq!(pairing.node_state, StepNodeState::Next);
        assert!(pairing.status_text.starts_with("Hold PS + Create"));
        assert!(
            matches!(&pairing.button, StepButtonSlot::Shown(step_button) if step_button.suggested)
        );
        assert_eq!(presentation.completed_steps, 0);
        assert_eq!(presentation.summary, "Not paired yet.");
    }

    #[test]
    fn running_step_disables_buttons_and_shows_spinner_state() {
        let presentation = present(
            &gamepad_snapshot(),
            &ActivePairing::Running(PairingStep::Pairing),
        );
        let pairing = step_of(&presentation, PairingStep::Pairing);
        assert_eq!(pairing.node_state, StepNodeState::Running);
        assert!(
            matches!(&pairing.button, StepButtonSlot::Shown(step_button) if !step_button.sensitive)
        );
        assert_eq!(presentation.summary, "Working on pairing…");
    }

    #[test]
    fn failed_step_shows_reason() {
        let failure = ActivePairing::Failed(BondingFailure::LinkKeyNotStored.into());
        let mut snapshot = gamepad_snapshot();
        snapshot.paired = true;
        let presentation = present(&snapshot, &failure);
        let bonding = step_of(&presentation, PairingStep::Bonding);
        assert_eq!(bonding.node_state, StepNodeState::Failed);
        assert_eq!(
            bonding.status_text,
            BondingFailure::LinkKeyNotStored.to_string()
        );
        assert_eq!(presentation.summary, "Bonding failed.");
        assert!(bonding.offers_forget);
        assert!(bonding.needs_attention);
    }

    fn attention_steps(presentation: &TimelinePresentation) -> Vec<PairingStep> {
        presentation
            .steps
            .iter()
            .filter(|step_presentation| step_presentation.needs_attention)
            .map(|step_presentation| step_presentation.step)
            .collect()
    }

    #[test]
    fn unpaired_device_opens_only_pairing() {
        let presentation = present(&gamepad_snapshot(), &ActivePairing::Idle);
        assert_eq!(attention_steps(&presentation), vec![PairingStep::Pairing]);
        assert!(!step_of(&presentation, PairingStep::Bonding).offers_forget);
    }

    #[test]
    fn running_step_is_the_only_open_step() {
        let mut snapshot = gamepad_snapshot();
        snapshot.paired = true;
        snapshot.bonded = true;
        snapshot.trusted = true;
        let presentation = present(&snapshot, &ActivePairing::Running(PairingStep::Connection));
        assert_eq!(
            attention_steps(&presentation),
            vec![PairingStep::Connection]
        );
    }

    #[test]
    fn complete_non_input_device_opens_nothing() {
        let mut snapshot = gamepad_snapshot();
        snapshot.icon_name = "audio-headphones".to_string();
        snapshot.paired = true;
        snapshot.bonded = true;
        snapshot.trusted = true;
        snapshot.connected = true;
        let presentation = present(&snapshot, &ActivePairing::Idle);
        assert!(attention_steps(&presentation).is_empty());
        assert!(step_of(&presentation, PairingStep::Bonding).offers_forget);
    }

    #[test]
    fn steps_after_missing_prerequisite_say_what_they_need() {
        let presentation = present(&gamepad_snapshot(), &ActivePairing::Idle);
        let authorization = step_of(&presentation, PairingStep::Authorization);
        assert!(!authorization.sensitive);
        assert_eq!(authorization.status_text, "Needs bonding");
    }

    fn bonded_trusted_snapshot() -> DeviceSnapshot {
        let mut snapshot = gamepad_snapshot();
        snapshot.paired = true;
        snapshot.bonded = true;
        snapshot.trusted = true;
        snapshot.icon_name = "phone".to_string();
        snapshot
    }

    #[test]
    fn distance_is_rendered_by_uom_in_meters() {
        let distance = uom::si::f32::Length::new::<meter>(2.04);
        assert_eq!(
            distance_row(DistanceEstimate::Estimated(distance)),
            hinted_property("Distance", "≈ 2.0 m", "Rough estimate from signal strength")
        );
    }

    #[test]
    fn device_in_range_suggests_connecting() {
        let mut snapshot = bonded_trusted_snapshot();
        snapshot.signal_strength = SignalStrength::Measured(-59);
        let presentation = present(&snapshot, &ActivePairing::Idle);
        let signal = step_of(&presentation, PairingStep::Signal);
        assert_eq!(signal.node_state, StepNodeState::Done);
        assert_eq!(signal.status_text, "Strong signal, in range");
        let connection = step_of(&presentation, PairingStep::Connection);
        assert_eq!(connection.node_state, StepNodeState::Next);
        assert!(connection.status_text.starts_with("In range"));
        assert!(
            connection
                .property_rows
                .iter()
                .all(|property_row| !matches!(property_row, PropertyRow::InlineWithIcon { key, .. } | PropertyRow::Inline { key, .. } if key == "Signal strength"))
        );
        assert_eq!(
            presentation.summary,
            "Bonded and trusted. In range, but not connected."
        );
    }

    #[test]
    fn unmeasured_device_asks_for_a_scan() {
        let presentation = present(&bonded_trusted_snapshot(), &ActivePairing::Idle);
        let signal = step_of(&presentation, PairingStep::Signal);
        assert_eq!(signal.node_state, StepNodeState::Next);
        assert!(signal.status_text.contains("press Scan"));
        assert_eq!(
            step_of(&presentation, PairingStep::Connection).status_text,
            "Waiting: turn the device on or bring it closer"
        );
    }

    #[test]
    fn connected_input_device_without_kernel_node_warns() {
        let mut snapshot = gamepad_snapshot();
        snapshot.paired = true;
        snapshot.bonded = true;
        snapshot.trusted = true;
        snapshot.connected = true;
        let presentation = present(&snapshot, &ActivePairing::Idle);
        assert_eq!(
            step_of(&presentation, PairingStep::InputDevice).node_state,
            StepNodeState::Warning
        );
        assert_eq!(
            presentation.summary,
            "Connected, but no input device appeared."
        );
        assert_eq!(presentation.completed_steps, 5);
    }
}
