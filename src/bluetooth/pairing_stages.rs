use async_channel::Sender;

use crate::input_devices::{find_kernel_input_devices, KernelInputDevices};
use crate::message::Message;
use crate::radios::list_radios;
use crate::window::OVERSKRIDE_PROPS;

const HUMAN_INTERFACE_DEVICE_SERVICE: bluer::Uuid = bluer::Uuid::from_u128(0x00001124_0000_1000_8000_00805f9b34fb);

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

#[derive(Clone, Debug, Default)]
pub struct RadioBond {
    pub adapter_name: String,
    pub adapter_address: String,
    pub radio_label: String,
    pub known: bool,
    pub bonded: bool,
    pub trusted: bool,
    pub connected: bool,
    pub shown: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PairingStages {
    pub radio_bonds: Vec<RadioBond>,
    pub paired: bool,
    pub bonded: bool,
    pub trusted: bool,
    pub blocked: bool,
    pub connected: bool,
    pub is_gamepad: bool,
    pub is_input_device: bool,
    pub kernel_input_devices: Option<KernelInputDevices>,
    pub device_address: String,
    pub address_type: String,
    pub legacy_pairing: bool,
    pub device_class: Option<u32>,
    pub signal_strength: Option<i16>,
    pub running_step: Option<PairingStep>,
    pub failed_step: Option<(PairingStep, String)>,
}

impl PairingStages {
    pub fn is_step_applicable(&self, step: PairingStep) -> bool {
        step != PairingStep::InputDevice || self.is_input_device
    }

    pub fn is_step_done(&self, step: PairingStep) -> bool {
        match step {
            PairingStep::Pairing => self.paired,
            PairingStep::Bonding => self.bonded,
            PairingStep::Authorization => self.trusted && !self.blocked,
            PairingStep::Connection => self.connected,
            PairingStep::InputDevice => self.kernel_input_devices.is_some(),
        }
    }

    pub fn applicable_steps(&self) -> impl Iterator<Item = PairingStep> + '_ {
        PAIRING_STEP_DEFINITIONS
            .iter()
            .map(|definition| definition.step)
            .filter(|step| self.is_step_applicable(*step))
    }

    pub fn first_missing_step(&self) -> Option<PairingStep> {
        self.applicable_steps().find(|step| !self.is_step_done(*step))
    }

    pub fn connection_policy(&self) -> ConnectionPolicy {
        if self.blocked {
            ConnectionPolicy::Blocked
        } else if self.trusted {
            ConnectionPolicy::Trusted
        } else {
            ConnectionPolicy::AskEachTime
        }
    }

    pub fn shown_radio(&self) -> Option<&RadioBond> {
        self.radio_bonds.iter().find(|radio_bond| radio_bond.shown)
    }

    pub fn other_bonded_radios(&self) -> impl Iterator<Item = &RadioBond> {
        self.radio_bonds.iter().filter(|radio_bond| !radio_bond.shown && radio_bond.bonded)
    }

    pub fn with_running_step(mut self, step: PairingStep) -> Self {
        self.running_step = Some(step);
        self.failed_step = None;
        self
    }

    pub fn with_failed_step(mut self, step: PairingStep, reason: String) -> Self {
        self.running_step = None;
        self.failed_step = Some((step, reason));
        self
    }
}

pub async fn read_pairing_stages(device: &bluer::Device) -> bluer::Result<PairingStages> {
    let paired = device.is_paired().await?;
    let connected = device.is_connected().await?;
    let bonded = device.is_bonded().await?.unwrap_or(paired);
    let icon_name = device.icon().await?.unwrap_or_default();
    let offers_human_interface_service = device.uuids().await?.unwrap_or_default().contains(&HUMAN_INTERFACE_DEVICE_SERVICE);
    let kernel_input_devices = if connected {
        find_kernel_input_devices(device.address())
    } else {
        None
    };

    Ok(PairingStages {
        radio_bonds: Vec::new(),
        paired,
        bonded,
        trusted: device.is_trusted().await?,
        blocked: device.is_blocked().await?,
        connected,
        is_gamepad: icon_name == "input-gaming",
        is_input_device: offers_human_interface_service || icon_name.starts_with("input-"),
        kernel_input_devices,
        device_address: device.address().to_string(),
        address_type: device.address_type().await?.to_string(),
        legacy_pairing: device.is_legacy_pairing().await?,
        device_class: device.class().await?,
        signal_strength: device.rssi().await?,
        running_step: None,
        failed_step: None,
    })
}

pub async fn send_pairing_stages(device: &bluer::Device, mut stages: PairingStages, sender: &Sender<Message>) {
    let device_page_is_open = OVERSKRIDE_PROPS.lock().unwrap().address == device.address();
    if device_page_is_open && stages.radio_bonds.is_empty() {
        stages.radio_bonds = read_radio_bonds(device).await.unwrap_or_default();
    }

    sender
        .send(Message::SwitchPairingStages(device.address(), stages))
        .await
        .expect("cannot send message");
}

pub async fn send_current_pairing_stages(device: &bluer::Device, sender: &Sender<Message>) -> bluer::Result<PairingStages> {
    let stages = read_pairing_stages(device).await?;
    send_pairing_stages(device, stages.clone(), sender).await;
    Ok(stages)
}

async fn read_radio_bonds(device: &bluer::Device) -> bluer::Result<Vec<RadioBond>> {
    let session = bluer::Session::new().await?;
    let device_address = device.address();
    let mut radio_bonds = Vec::new();

    for radio in list_radios(&session).await? {
        let adapter = session.adapter(&radio.adapter_name)?;
        let known = adapter.device_addresses().await?.contains(&device_address);
        let mut radio_bond = RadioBond {
            shown: radio.adapter_name == device.adapter_name(),
            adapter_address: adapter.address().await?.to_string(),
            adapter_name: radio.adapter_name,
            radio_label: radio.radio_label,
            known,
            ..Default::default()
        };

        if known {
            let radio_device = adapter.device(device_address)?;
            let paired = radio_device.is_paired().await?;
            radio_bond.bonded = radio_device.is_bonded().await?.unwrap_or(paired);
            radio_bond.trusted = radio_device.is_trusted().await?;
            radio_bond.connected = radio_device.is_connected().await?;
        }

        radio_bonds.push(radio_bond);
    }

    Ok(radio_bonds)
}
