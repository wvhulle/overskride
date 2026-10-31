use bindrune_bluez::{
    device_pairing::pairing_step_progress::{
        PAIRING_STEP_DEFINITIONS, is_step_applicable, is_step_done,
    },
    remote_device::{
        linux_kernel_input_devices::KernelInputBinding,
        remote_device_snapshot::{DeviceSnapshot, SignalStrength},
    },
};

use crate::icon_names;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingStatusBadge {
    pub icon_name: &'static str,
    pub style_class: &'static str,
    pub tooltip: String,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalQuality {
    Strong,
    Good,
    Fair,
    Weak,
    VeryWeak,
    Lost,
}

impl SignalQuality {
    pub fn from_rssi(rssi: i16) -> Self {
        match rssi {
            -60.. => Self::Strong,
            -70..=-61 => Self::Good,
            -80..=-71 => Self::Fair,
            -90..=-81 => Self::Weak,
            -100..=-91 => Self::VeryWeak,
            _ => Self::Lost,
        }
    }

    pub fn quality_label(self) -> &'static str {
        match self {
            Self::Strong => "Strong",
            Self::Good => "Good",
            Self::Fair => "Fair",
            Self::Weak => "Weak",
            Self::VeryWeak => "Very weak",
            Self::Lost => "Barely reachable",
        }
    }

    pub fn quality_icon_name(self) -> &'static str {
        match self {
            Self::Strong => icon_names::custom::RSSI_HIGH_SYMBOLIC,
            Self::Good => icon_names::custom::RSSI_MEDIUM_SYMBOLIC,
            Self::Fair => icon_names::custom::RSSI_LOW_SYMBOLIC,
            Self::Weak => icon_names::custom::RSSI_DEAD_SYMBOLIC,
            Self::VeryWeak => icon_names::custom::RSSI_NONE_SYMBOLIC,
            Self::Lost => icon_names::custom::RSSI_NOT_FOUND_SYMBOLIC,
        }
    }
}

pub fn signal_icon_name(signal_strength: SignalStrength) -> &'static str {
    match signal_strength {
        SignalStrength::Unknown => icon_names::custom::RSSI_NONE_SYMBOLIC,
        SignalStrength::Measured(rssi) => SignalQuality::from_rssi(rssi).quality_icon_name(),
    }
}

pub fn device_icon_name(snapshot: &DeviceSnapshot) -> &str {
    if snapshot.icon_name.is_empty() {
        snapshot
            .device_guess()
            .map_or(icon_names::shipped::BLUETOOTH, |device_guess| {
                device_guess.icon_name
            })
    } else {
        &snapshot.icon_name
    }
}

pub fn symbolic_device_icon_name(snapshot: &DeviceSnapshot) -> String {
    let icon_name = device_icon_name(snapshot);
    if icon_name.ends_with("-symbolic") {
        icon_name.to_string()
    } else {
        format!("{icon_name}-symbolic")
    }
}

pub fn describe_device_type(snapshot: &DeviceSnapshot) -> &str {
    if !snapshot.icon_name.is_empty() {
        &snapshot.icon_name
    } else {
        snapshot
            .device_guess()
            .map_or("Unknown", |device_guess| device_guess.label)
    }
}

pub fn type_guess_evidence(snapshot: &DeviceSnapshot) -> Option<&'static str> {
    if snapshot.icon_name.is_empty() {
        snapshot
            .device_guess()
            .map(|device_guess| device_guess.evidence)
    } else {
        None
    }
}

pub fn describe_address_type(snapshot: &DeviceSnapshot) -> &'static str {
    if snapshot.address_type == "random" {
        "Random, may change every few minutes"
    } else {
        "Public, fixed by the manufacturer"
    }
}

pub fn input_device_missing(snapshot: &DeviceSnapshot) -> bool {
    snapshot.connected
        && snapshot.is_input_device()
        && snapshot.kernel_input == KernelInputBinding::Absent
}

pub fn pairing_status_badge(snapshot: &DeviceSnapshot) -> PairingStatusBadge {
    let (icon_name, style_class, status_summary) = if snapshot.blocked {
        (
            "action-unavailable-symbolic",
            "error",
            "Blocked\nEvery connection is rejected",
        )
    } else if snapshot.paired && !snapshot.bonded {
        (
            "dialog-warning-symbolic",
            "warning",
            "Bond missing\nTrust can't be saved",
        )
    } else if input_device_missing(snapshot) {
        (
            "dialog-warning-symbolic",
            "warning",
            "Connected, but no input device appeared",
        )
    } else if snapshot.connected {
        ("bluetooth-active-symbolic", "success", "Connected")
    } else if snapshot.bonded && snapshot.trusted {
        (
            "bluetooth-disconnected-symbolic",
            "dim-label",
            "Ready\nReconnects when turned on",
        )
    } else {
        (
            "changes-prevent-symbolic",
            "dim-label",
            "Bonded\nEach connection needs approval",
        )
    };

    let step_overview = PAIRING_STEP_DEFINITIONS
        .iter()
        .filter(|definition| is_step_applicable(snapshot, definition.step))
        .map(|definition| {
            let step_state = if is_step_done(snapshot, definition.step) {
                "done"
            } else {
                "not yet"
            };
            format!("{}: {step_state}", definition.title)
        })
        .collect::<Vec<_>>()
        .join("\n");

    PairingStatusBadge {
        icon_name,
        style_class,
        tooltip: format!("{status_summary}\n\n{step_overview}"),
        visible: snapshot.paired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_icons_get_weaker_with_lower_rssi() {
        assert_eq!(
            signal_icon_name(SignalStrength::Measured(-50)),
            icon_names::custom::RSSI_HIGH_SYMBOLIC
        );
        assert_eq!(
            signal_icon_name(SignalStrength::Measured(-75)),
            icon_names::custom::RSSI_LOW_SYMBOLIC
        );
        assert_eq!(
            signal_icon_name(SignalStrength::Measured(-120)),
            icon_names::custom::RSSI_NOT_FOUND_SYMBOLIC
        );
        assert_eq!(
            signal_icon_name(SignalStrength::Unknown),
            icon_names::custom::RSSI_NONE_SYMBOLIC
        );
    }
}
