use std::collections::HashMap;

pub const FIXTURE_ADAPTER_NAME: &str = "hci0";

const AUDIO_SINK_SERVICE: &str = "0000110b-0000-1000-8000-00805f9b34fb";
const AUDIO_REMOTE_CONTROL_SERVICE: &str = "0000110e-0000-1000-8000-00805f9b34fb";
const HANDS_FREE_SERVICE: &str = "0000111e-0000-1000-8000-00805f9b34fb";
const HUMAN_INTERFACE_SERVICE: &str = "00001124-0000-1000-8000-00805f9b34fb";
const OBJECT_PUSH_SERVICE: &str = "00001105-0000-1000-8000-00805f9b34fb";
const BATTERY_SERVICE: &str = "0000180f-0000-1000-8000-00805f9b34fb";
const GENERIC_ACCESS_SERVICE: &str = "00001800-0000-1000-8000-00805f9b34fb";
const SONY_COMPANY_IDENTIFIER: u16 = 0x012d;
const LOGITECH_COMPANY_IDENTIFIER: u16 = 0x01da;
const GOOGLE_COMPANY_IDENTIFIER: u16 = 0x00e0;

pub struct FakeAdapter {
    pub address: String,
    pub alias: String,
    pub powered: bool,
    pub discoverable: bool,
    pub discoverable_timeout_seconds: u32,
}

pub enum FakeBatteryLevel {
    NotReported,
    Percentage(u8),
}

pub struct FakeDevice {
    pub address: &'static str,
    pub address_type: &'static str,
    pub alias: &'static str,
    pub icon_name: &'static str,
    pub device_class: u32,
    pub appearance: u16,
    pub service_uuids: Vec<&'static str>,
    pub manufacturer_data: HashMap<u16, Vec<u8>>,
    pub paired: bool,
    pub bonded: bool,
    pub trusted: bool,
    pub connected: bool,
    pub signal_strength_dbm: i16,
    pub battery_level: FakeBatteryLevel,
}

impl FakeDevice {
    pub fn object_path_segment(&self) -> String {
        format!("dev_{}", self.address.replace(':', "_"))
    }
}

pub fn fixture_adapter() -> FakeAdapter {
    FakeAdapter {
        address: "C8:94:02:6B:3A:11".to_string(),
        alias: "Framework Laptop".to_string(),
        powered: true,
        discoverable: false,
        discoverable_timeout_seconds: 180,
    }
}

pub fn fixture_devices() -> Vec<FakeDevice> {
    vec![
        FakeDevice {
            address: "4C:87:5D:2A:91:0E",
            address_type: "public",
            alias: "WH-1000XM5",
            icon_name: "audio-headset",
            device_class: 0x0024_0404,
            appearance: 0,
            service_uuids: vec![
                AUDIO_SINK_SERVICE,
                AUDIO_REMOTE_CONTROL_SERVICE,
                HANDS_FREE_SERVICE,
            ],
            manufacturer_data: HashMap::from([(SONY_COMPANY_IDENTIFIER, vec![0x04, 0x00])]),
            paired: true,
            bonded: true,
            trusted: true,
            connected: true,
            signal_strength_dbm: -48,
            battery_level: FakeBatteryLevel::Percentage(80),
        },
        FakeDevice {
            address: "7C:66:EF:30:12:A4",
            address_type: "public",
            alias: "DualSense Wireless Controller",
            icon_name: "input-gaming",
            device_class: 0x0000_2508,
            appearance: 0,
            service_uuids: vec![HUMAN_INTERFACE_SERVICE],
            manufacturer_data: HashMap::from([(SONY_COMPANY_IDENTIFIER, vec![0x02])]),
            paired: true,
            bonded: true,
            trusted: true,
            connected: false,
            signal_strength_dbm: -61,
            battery_level: FakeBatteryLevel::NotReported,
        },
        FakeDevice {
            address: "58:24:29:B8:3C:11",
            address_type: "public",
            alias: "Pixel 8",
            icon_name: "phone",
            device_class: 0x005a_020c,
            appearance: 0,
            service_uuids: vec![OBJECT_PUSH_SERVICE, HANDS_FREE_SERVICE],
            manufacturer_data: HashMap::from([(GOOGLE_COMPANY_IDENTIFIER, vec![0x01])]),
            paired: true,
            bonded: true,
            trusted: true,
            connected: false,
            signal_strength_dbm: -72,
            battery_level: FakeBatteryLevel::NotReported,
        },
        FakeDevice {
            address: "D2:6E:41:9A:07:3B",
            address_type: "random",
            alias: "MX Master 3S",
            icon_name: "input-mouse",
            device_class: 0,
            appearance: 0x03c2,
            service_uuids: vec![HUMAN_INTERFACE_SERVICE, BATTERY_SERVICE],
            manufacturer_data: HashMap::from([(LOGITECH_COMPANY_IDENTIFIER, vec![0x01, 0x02])]),
            paired: false,
            bonded: false,
            trusted: false,
            connected: false,
            signal_strength_dbm: -66,
            battery_level: FakeBatteryLevel::NotReported,
        },
        FakeDevice {
            address: "6A:1F:C3:04:B7:52",
            address_type: "random",
            alias: "6A-1F-C3-04-B7-52",
            icon_name: "",
            device_class: 0,
            appearance: 0,
            service_uuids: vec![GENERIC_ACCESS_SERVICE],
            manufacturer_data: HashMap::new(),
            paired: false,
            bonded: false,
            trusted: false,
            connected: false,
            signal_strength_dbm: -88,
            battery_level: FakeBatteryLevel::NotReported,
        },
    ]
}
