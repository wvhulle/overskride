use std::collections::{BTreeSet, HashMap};

use bluer::UuidExt;

const APPLE_COMPANY_ID: u16 = 0x004C;
const MICROSOFT_COMPANY_ID: u16 = 0x0006;
const SAMSUNG_COMPANY_ID: u16 = 0x0075;
const GOOGLE_COMPANY_ID: u16 = 0x00E0;
const SWIFT_PAIR_PREFIX: [u8; 3] = [0x03, 0x00, 0x80];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceAppearance {
    Unknown,
    Reported(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceGuess {
    pub label: &'static str,
    pub icon_name: &'static str,
    pub evidence: &'static str,
}

const fn device_guess(
    label: &'static str,
    icon_name: &'static str,
    evidence: &'static str,
) -> DeviceGuess {
    DeviceGuess {
        label,
        icon_name,
        evidence,
    }
}

fn guess_from_apple_payload(continuity_payload: &[u8]) -> DeviceGuess {
    match continuity_payload.first() {
        Some(0x07) => device_guess(
            "Apple AirPods or Beats",
            "audio-headphones-symbolic",
            "Apple proximity pairing message",
        ),
        Some(0x12) => device_guess(
            "Apple Find My Accessory",
            "find-location-symbolic",
            "Apple Find My message, sent by AirTags and lost Apple devices",
        ),
        Some(0x10) => device_guess(
            "Apple iPhone, iPad or Mac",
            "phone-apple-iphone-symbolic",
            "Apple nearby info message",
        ),
        Some(0x02) => device_guess("iBeacon", "find-location-symbolic", "Apple iBeacon message"),
        Some(0x09 | 0x0C | 0x0F) => device_guess(
            "Apple Device",
            "computer-apple-ipad-symbolic",
            "Apple AirPlay or Handoff message",
        ),
        _ => device_guess(
            "Apple Device",
            "computer-apple-ipad-symbolic",
            "Apple manufacturer data",
        ),
    }
}

fn guess_from_microsoft_payload(beacon_payload: &[u8]) -> DeviceGuess {
    if beacon_payload.starts_with(&SWIFT_PAIR_PREFIX) {
        device_guess(
            "Swift Pair Accessory",
            "bluetooth-symbolic",
            "Microsoft Swift Pair message",
        )
    } else {
        device_guess(
            "Windows PC",
            "computer-symbolic",
            "Microsoft Connected Devices Platform beacon",
        )
    }
}

fn guess_from_manufacturer_data(manufacturer_data: &HashMap<u16, Vec<u8>>) -> Option<DeviceGuess> {
    if let Some(continuity_payload) = manufacturer_data.get(&APPLE_COMPANY_ID) {
        return Some(guess_from_apple_payload(continuity_payload));
    }
    if let Some(beacon_payload) = manufacturer_data.get(&MICROSOFT_COMPANY_ID) {
        return Some(guess_from_microsoft_payload(beacon_payload));
    }
    if manufacturer_data.contains_key(&SAMSUNG_COMPANY_ID) {
        return Some(device_guess(
            "Samsung Device",
            "phone-symbolic",
            "Samsung manufacturer data",
        ));
    }
    if manufacturer_data.contains_key(&GOOGLE_COMPANY_ID) {
        return Some(device_guess(
            "Google Device",
            "phone-symbolic",
            "Google manufacturer data",
        ));
    }
    None
}

fn guess_from_short_service(short_service_id: u16) -> Option<DeviceGuess> {
    let service_guess = match short_service_id {
        0xFD59 | 0xFD5A => device_guess(
            "Samsung SmartTag",
            "find-location-symbolic",
            "Samsung SmartTag service",
        ),
        0xFE2C => device_guess(
            "Fast Pair Accessory",
            "audio-headphones-symbolic",
            "Google Fast Pair service",
        ),
        0xFEAA => device_guess(
            "Eddystone Beacon",
            "find-location-symbolic",
            "Eddystone beacon service",
        ),
        0xFD6F => device_guess(
            "Phone",
            "phone-symbolic",
            "Exposure Notification service, sent by phones",
        ),
        0xFEEC | 0xFEED => device_guess("Tile Tracker", "find-location-symbolic", "Tile service"),
        0x180D => device_guess(
            "Heart Rate Sensor",
            "emblem-favorite-symbolic",
            "Heart Rate service",
        ),
        0x1812 => device_guess(
            "Input Device",
            "input-keyboard-symbolic",
            "HID over GATT service",
        ),
        _ => return None,
    };
    Some(service_guess)
}

fn guess_from_services(
    service_data: &HashMap<bluer::Uuid, Vec<u8>>,
    services: &BTreeSet<bluer::Uuid>,
) -> Option<DeviceGuess> {
    services
        .iter()
        .chain(service_data.keys())
        .filter_map(UuidExt::as_u16)
        .find_map(guess_from_short_service)
}

fn guess_from_appearance(appearance: u16) -> Option<DeviceGuess> {
    const EVIDENCE: &str = "Appearance value";
    let appearance_guess = match appearance {
        0x03C1 => device_guess("Keyboard", "input-keyboard-symbolic", EVIDENCE),
        0x03C2 => device_guess("Mouse", "input-mouse-symbolic", EVIDENCE),
        0x03C3 | 0x03C4 => device_guess("Game Controller", "input-gaming-symbolic", EVIDENCE),
        0x03C5 => device_guess("Drawing Tablet", "input-tablet-symbolic", EVIDENCE),
        _ => match appearance >> 6 {
            0x01 => device_guess("Phone", "phone-symbolic", EVIDENCE),
            0x02 => device_guess("Computer", "computer-symbolic", EVIDENCE),
            0x03 => device_guess("Watch", "preferences-system-time-symbolic", EVIDENCE),
            0x05 => device_guess("Display", "video-display-symbolic", EVIDENCE),
            0x08 | 0x09 => device_guess("Tag", "find-location-symbolic", EVIDENCE),
            0x0A => device_guess("Media Player", "multimedia-player-symbolic", EVIDENCE),
            0x0D => device_guess("Heart Rate Sensor", "emblem-favorite-symbolic", EVIDENCE),
            0x0F => device_guess("Input Device", "input-keyboard-symbolic", EVIDENCE),
            0x21 => device_guess("Speaker", "audio-speakers-symbolic", EVIDENCE),
            0x25 => device_guess("Headphones", "audio-headphones-symbolic", EVIDENCE),
            0x26 => device_guess("Hearing Aid", "audio-headphones-symbolic", EVIDENCE),
            _ => return None,
        },
    };
    Some(appearance_guess)
}

pub fn guess_device_kind(
    manufacturer_data: &HashMap<u16, Vec<u8>>,
    service_data: &HashMap<bluer::Uuid, Vec<u8>>,
    services: &BTreeSet<bluer::Uuid>,
    appearance: DeviceAppearance,
) -> Option<DeviceGuess> {
    let appearance_guess = match appearance {
        DeviceAppearance::Reported(appearance_value) => guess_from_appearance(appearance_value),
        DeviceAppearance::Unknown => None,
    };
    appearance_guess
        .or_else(|| guess_from_services(service_data, services))
        .or_else(|| guess_from_manufacturer_data(manufacturer_data))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess_from_manufacturer(company_id: u16, payload: &[u8]) -> Option<DeviceGuess> {
        guess_device_kind(
            &HashMap::from([(company_id, payload.to_vec())]),
            &HashMap::new(),
            &BTreeSet::new(),
            DeviceAppearance::Unknown,
        )
    }

    #[test]
    fn apple_proximity_pairing_is_airpods() {
        let airpods_guess = guess_from_manufacturer(APPLE_COMPANY_ID, &[0x07, 0x19, 0x01]);
        assert_eq!(
            airpods_guess.map(|guess| guess.label),
            Some("Apple AirPods or Beats")
        );
    }

    #[test]
    fn microsoft_beacon_is_windows_pc() {
        let windows_guess = guess_from_manufacturer(MICROSOFT_COMPANY_ID, &[0x01, 0x09, 0x20]);
        assert_eq!(windows_guess.map(|guess| guess.label), Some("Windows PC"));
        let swift_pair_guess =
            guess_from_manufacturer(MICROSOFT_COMPANY_ID, &[0x03, 0x00, 0x80, 0x01]);
        assert_eq!(
            swift_pair_guess.map(|guess| guess.label),
            Some("Swift Pair Accessory")
        );
    }

    #[test]
    fn fast_pair_service_data_is_accessory() {
        let fast_pair_guess = guess_device_kind(
            &HashMap::new(),
            &HashMap::from([(bluer::Uuid::from_u16(0xFE2C), vec![0x00])]),
            &BTreeSet::new(),
            DeviceAppearance::Unknown,
        );
        assert_eq!(
            fast_pair_guess.map(|guess| guess.label),
            Some("Fast Pair Accessory")
        );
    }

    #[test]
    fn appearance_watch_category_is_watch() {
        let watch_guess = guess_device_kind(
            &HashMap::new(),
            &HashMap::new(),
            &BTreeSet::new(),
            DeviceAppearance::Reported(0x00C1),
        );
        assert_eq!(watch_guess.map(|guess| guess.label), Some("Watch"));
    }

    #[test]
    fn appearance_wins_over_manufacturer() {
        let gamepad_guess = guess_device_kind(
            &HashMap::from([(SAMSUNG_COMPANY_ID, vec![0x01])]),
            &HashMap::new(),
            &BTreeSet::new(),
            DeviceAppearance::Reported(0x03C4),
        );
        assert_eq!(
            gamepad_guess.map(|guess| guess.label),
            Some("Game Controller")
        );
    }

    #[test]
    fn nothing_advertised_gives_no_guess() {
        assert_eq!(
            guess_device_kind(
                &HashMap::new(),
                &HashMap::new(),
                &BTreeSet::new(),
                DeviceAppearance::Unknown
            ),
            None
        );
    }
}
