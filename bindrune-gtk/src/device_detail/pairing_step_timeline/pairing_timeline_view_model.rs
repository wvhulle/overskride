use bindrune_bluez::{
    bluetooth_adapter::device_bond_per_adapter::RadioBond,
    device_pairing::{
        pairing_step_failure::PairingFailure,
        pairing_step_progress::{ConnectionPolicy, PairingStep},
    },
    remote_device::remote_device_snapshot::DeviceSnapshot,
};

use crate::device_detail::pairing_step_timeline::link_quality_presentation::LinkMeasurements;

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
    pub link_measurements: &'inputs LinkMeasurements,
}

pub(super) fn inline_property(key: &str, value: impl Into<String>) -> PropertyRow {
    PropertyRow::Inline {
        key: key.to_string(),
        value: value.into(),
        hint: PropertyHint::Absent,
    }
}

pub(super) fn hinted_property(key: &str, value: impl Into<String>, hint_text: &str) -> PropertyRow {
    PropertyRow::Inline {
        key: key.to_string(),
        value: value.into(),
        hint: PropertyHint::Shown(hint_text.to_string()),
    }
}

pub(super) fn stacked_property(key: &str, value: impl Into<String>) -> PropertyRow {
    PropertyRow::Stacked {
        key: key.to_string(),
        value: value.into(),
        hint: PropertyHint::Absent,
    }
}

pub(super) struct StepParts {
    pub(super) node_state: StepNodeState,
    pub(super) status_text: String,
    pub(super) button: StepButtonSlot,
    pub(super) property_rows: Vec<PropertyRow>,
}
