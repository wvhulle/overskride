use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityCategory {
    Audio,
    Calls,
    Data,
    Files,
    Input,
    Sensors,
}

impl CapabilityCategory {
    pub const ALL: [Self; 6] = [
        Self::Audio,
        Self::Calls,
        Self::Data,
        Self::Files,
        Self::Input,
        Self::Sensors,
    ];

    pub fn category_title(self) -> &'static str {
        match self {
            Self::Audio => "Audio",
            Self::Calls => "Calls",
            Self::Data => "Data",
            Self::Files => "Files",
            Self::Input => "Input",
            Self::Sensors => "Sensors",
        }
    }

    pub fn shown_when_empty(self) -> bool {
        self != Self::Sensors
    }

    pub fn capabilities(self) -> impl Iterator<Item = DeviceCapability> {
        DeviceCapability::ALL
            .into_iter()
            .filter(move |capability| capability.category() == self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceCapability {
    PlaysAudioHere,
    PlaysAudioFromHere,
    RemoteControl,
    RoutesPhoneCalls,
    ActsAsHeadset,
    SharesInternet,
    SharesContacts,
    SharesMessages,
    SimAccess,
    SerialPort,
    ReceivesFiles,
    BrowsesFiles,
    InputDevice,
    HealthSensor,
}

const fn bluetooth_base_uuid(short_uuid: u32) -> bluer::Uuid {
    bluer::Uuid::from_u128(((short_uuid as u128) << 96) | 0x0000_1000_8000_0080_5f9b_34fb)
}

impl DeviceCapability {
    pub const ALL: [Self; 14] = [
        Self::PlaysAudioHere,
        Self::PlaysAudioFromHere,
        Self::RemoteControl,
        Self::RoutesPhoneCalls,
        Self::ActsAsHeadset,
        Self::SharesInternet,
        Self::SharesContacts,
        Self::SharesMessages,
        Self::SimAccess,
        Self::SerialPort,
        Self::ReceivesFiles,
        Self::BrowsesFiles,
        Self::InputDevice,
        Self::HealthSensor,
    ];

    pub fn category(self) -> CapabilityCategory {
        match self {
            Self::PlaysAudioHere | Self::PlaysAudioFromHere | Self::RemoteControl => {
                CapabilityCategory::Audio
            }
            Self::RoutesPhoneCalls | Self::ActsAsHeadset => CapabilityCategory::Calls,
            Self::SharesInternet
            | Self::SharesContacts
            | Self::SharesMessages
            | Self::SimAccess
            | Self::SerialPort => CapabilityCategory::Data,
            Self::ReceivesFiles | Self::BrowsesFiles => CapabilityCategory::Files,
            Self::InputDevice => CapabilityCategory::Input,
            Self::HealthSensor => CapabilityCategory::Sensors,
        }
    }

    pub fn capability_title(self) -> &'static str {
        match self {
            Self::PlaysAudioHere => "Play its audio on this computer",
            Self::PlaysAudioFromHere => "Play this computer's audio",
            Self::RemoteControl => "Media play/pause and volume",
            Self::RoutesPhoneCalls => "Route phone calls through this computer",
            Self::ActsAsHeadset => "Act as a headset for calls",
            Self::SharesInternet => "Share internet",
            Self::SharesContacts => "Share contacts",
            Self::SharesMessages => "Share text messages",
            Self::SimAccess => "SIM access",
            Self::SerialPort => "Serial port",
            Self::ReceivesFiles => "Receive files",
            Self::BrowsesFiles => "Browse files",
            Self::InputDevice => "Keyboard, mouse or controller",
            Self::HealthSensor => "Health or fitness sensor",
        }
    }

    fn providing_short_uuids(self) -> &'static [u32] {
        match self {
            Self::PlaysAudioHere => &[0x110a],
            Self::PlaysAudioFromHere => &[0x110b],
            Self::RemoteControl => &[0x110c, 0x110e],
            Self::RoutesPhoneCalls => &[0x1112, 0x111f],
            Self::ActsAsHeadset => &[0x1108, 0x111e],
            Self::SharesInternet => &[0x1115, 0x1116, 0x1117],
            Self::SharesContacts => &[0x112f],
            Self::SharesMessages => &[0x1132],
            Self::SimAccess => &[0x112d],
            Self::SerialPort => &[0x1101],
            Self::ReceivesFiles => &[0x1105],
            Self::BrowsesFiles => &[0x1106],
            Self::InputDevice => &[0x1124],
            Self::HealthSensor => &[0x180d, 0x1809, 0x1816, 0x1400, 0x1401, 0x1402],
        }
    }

    pub fn is_offered_by(self, advertised_services: &BTreeSet<bluer::Uuid>) -> bool {
        self.providing_short_uuids()
            .iter()
            .any(|short_uuid| advertised_services.contains(&bluetooth_base_uuid(*short_uuid)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iphone_services() -> BTreeSet<bluer::Uuid> {
        [
            0x110a, 0x110c, 0x110d, 0x110e, 0x1116, 0x111f, 0x112f, 0x1132, 0x1200, 0x1800, 0x1801,
            0x180a,
        ]
        .into_iter()
        .map(bluetooth_base_uuid)
        .collect()
    }

    #[test]
    fn short_uuid_expands_to_bluetooth_base() {
        assert_eq!(
            bluetooth_base_uuid(0x1105).to_string(),
            "00001105-0000-1000-8000-00805f9b34fb"
        );
    }

    #[test]
    fn iphone_offers_audio_calls_and_data_but_no_files() {
        let advertised_services = iphone_services();
        [
            DeviceCapability::PlaysAudioHere,
            DeviceCapability::RemoteControl,
            DeviceCapability::RoutesPhoneCalls,
            DeviceCapability::SharesInternet,
            DeviceCapability::SharesContacts,
            DeviceCapability::SharesMessages,
        ]
        .into_iter()
        .for_each(|capability| assert!(capability.is_offered_by(&advertised_services)));
        [
            DeviceCapability::PlaysAudioFromHere,
            DeviceCapability::ActsAsHeadset,
            DeviceCapability::ReceivesFiles,
            DeviceCapability::BrowsesFiles,
            DeviceCapability::InputDevice,
        ]
        .into_iter()
        .for_each(|capability| assert!(!capability.is_offered_by(&advertised_services)));
    }

    #[test]
    fn every_capability_belongs_to_a_listed_category() {
        let categorized_count: usize = CapabilityCategory::ALL
            .into_iter()
            .map(|category| category.capabilities().count())
            .sum();
        assert_eq!(categorized_count, DeviceCapability::ALL.len());
    }
}
