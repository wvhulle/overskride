use crate::bluetooth_error::BluetoothError;
use crate::device_snapshot::DeviceSnapshot;
use crate::kernel_input_devices::KernelInputBinding;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairingStep {
    Pairing,
    Bonding,
    Authorization,
    Connection,
    InputDevice,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConnectionPolicy {
    Blocked,
    AskEachTime,
    Trusted,
}

pub struct PairingStepDefinition {
    pub step: PairingStep,
    pub title: &'static str,
    pub protocol_term: &'static str,
    pub protocol_details: &'static str,
}

pub const PAIRING_STEP_DEFINITIONS: [PairingStepDefinition; 5] = [
    PairingStepDefinition {
        step: PairingStep::Pairing,
        title: "Pairing",
        protocol_term: "Inquiry + Secure Simple Pairing",
        protocol_details: "A new device must first be discoverable: in pairing mode it answers the inquiry packets this computer sends while scanning (the Scan button in the sidebar), which is how it appears in the list. Pairing then exchanges input/output capabilities and public keys and authenticates both sides; devices without a display or keyboard, such as gamepads, use the Just Works model. The result is a shared link key. Already bonded devices skip all of this and reconnect directly.",
    },
    PairingStepDefinition {
        step: PairingStep::Bonding,
        title: "Bonding",
        protocol_term: "Link key stored",
        protocol_details: "Bonding means keeping the link key after pairing. BlueZ writes it to /var/lib/bluetooth/<adapter>/<device>/info, separately for every radio. Without a stored key the next connection needs a new pairing, and BlueZ forgets the device once it disconnects.",
    },
    PairingStepDefinition {
        step: PairingStep::Authorization,
        title: "Authorization",
        protocol_term: "BlueZ connection policy",
        protocol_details: "Not part of the Bluetooth protocol. When a device opens a profile connection (input, audio), BlueZ rejects it if the device is blocked, lets it through if the device is trusted, and otherwise asks the pairing agent. The setting is saved next to the link key, so it only survives for bonded devices.",
    },
    PairingStepDefinition {
        step: PairingStep::Connection,
        title: "Connection",
        protocol_term: "ACL link + L2CAP channels",
        protocol_details: "The device pages this computer, both sides authenticate with the stored link key and encrypt the ACL link, then the profile opens its L2CAP channels. For input devices these are the HID control (PSM 17) and interrupt (PSM 19) channels.",
    },
    PairingStepDefinition {
        step: PairingStep::InputDevice,
        title: "Input Device",
        protocol_term: "Linux HID driver",
        protocol_details: "Happens automatically. BlueZ passes the HID reports to the kernel, which binds a HID driver (hid-playstation for DualSense controllers). The driver creates the input devices that games and Steam read from /dev/input, such as buttons, motion sensors and touchpad, and reports the battery level.",
    },
];

#[derive(Debug)]
pub enum PairingProgress {
    Running(PairingStep),
    Failed(PairingStep, BluetoothError),
    Finished,
}

pub fn is_step_applicable(snapshot: &DeviceSnapshot, step: PairingStep) -> bool {
    step != PairingStep::InputDevice || snapshot.is_input_device()
}

pub fn is_step_done(snapshot: &DeviceSnapshot, step: PairingStep) -> bool {
    match step {
        PairingStep::Pairing => snapshot.paired,
        PairingStep::Bonding => snapshot.bonded,
        PairingStep::Authorization => snapshot.trusted && !snapshot.blocked,
        PairingStep::Connection => snapshot.connected,
        PairingStep::InputDevice => matches!(snapshot.kernel_input, KernelInputBinding::Bound(_)),
    }
}

pub fn applicable_steps(snapshot: &DeviceSnapshot) -> impl Iterator<Item = PairingStep> + '_ {
    PAIRING_STEP_DEFINITIONS
        .iter()
        .map(|definition| definition.step)
        .filter(|step| is_step_applicable(snapshot, *step))
}

pub fn first_missing_step(snapshot: &DeviceSnapshot) -> Option<PairingStep> {
    applicable_steps(snapshot).find(|step| !is_step_done(snapshot, *step))
}

pub fn connection_policy(snapshot: &DeviceSnapshot) -> ConnectionPolicy {
    if snapshot.blocked {
        ConnectionPolicy::Blocked
    } else if snapshot.trusted {
        ConnectionPolicy::Trusted
    } else {
        ConnectionPolicy::AskEachTime
    }
}

pub fn step_definition(step: PairingStep) -> &'static PairingStepDefinition {
    PAIRING_STEP_DEFINITIONS
        .iter()
        .find(|definition| definition.step == step)
        .expect("every pairing step has a definition")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_snapshot::tests::sample_snapshot;

    #[test]
    fn unpaired_device_misses_pairing_first() {
        assert_eq!(first_missing_step(&sample_snapshot()), Some(PairingStep::Pairing));
    }

    #[test]
    fn paired_without_bond_misses_bonding() {
        let mut snapshot = sample_snapshot();
        snapshot.paired = true;
        assert_eq!(first_missing_step(&snapshot), Some(PairingStep::Bonding));
    }

    #[test]
    fn gamepad_needs_kernel_input_device_last() {
        let mut snapshot = sample_snapshot();
        snapshot.paired = true;
        snapshot.bonded = true;
        snapshot.trusted = true;
        snapshot.connected = true;
        assert_eq!(first_missing_step(&snapshot), Some(PairingStep::InputDevice));
    }

    #[test]
    fn non_input_device_skips_input_step() {
        let mut snapshot = sample_snapshot();
        snapshot.icon_name = "audio-headphones".to_string();
        snapshot.paired = true;
        snapshot.bonded = true;
        snapshot.trusted = true;
        snapshot.connected = true;
        assert_eq!(first_missing_step(&snapshot), None);
    }

    #[test]
    fn blocked_overrides_trusted_policy() {
        let mut snapshot = sample_snapshot();
        snapshot.trusted = true;
        snapshot.blocked = true;
        assert_eq!(connection_policy(&snapshot), ConnectionPolicy::Blocked);
        assert!(!is_step_done(&snapshot, PairingStep::Authorization));
    }
}
