use std::{
    collections::{BTreeSet, HashMap},
    str::FromStr,
};

use bluer::DeviceProperty;
use uom::si::{f32::Length, length::meter};

use crate::{
    bluez_dbus_session::DeviceLocation,
    bluez_operation_error::BluetoothError,
    remote_device::{
        advertised_device_capabilities::DeviceCapability,
        gatt_service_uuid_names::describe_service,
        linux_kernel_input_devices::{KernelInputBinding, find_kernel_input_devices},
        nameless_device_type_guess::{DeviceAppearance, DeviceGuess, guess_device_kind},
    },
};

pub const UNKNOWN_DEVICE_NAME: &str = "Unknown Device";
pub const HUMAN_INTERFACE_DEVICE_SERVICE: bluer::Uuid =
    bluer::Uuid::from_u128(0x0000_1124_0000_1000_8000_0080_5f9b_34fb);
const FALLBACK_RSSI_AT_ONE_METER: i16 = -59;
const FREE_SPACE_LOSS_AT_ONE_METER: i16 = 41;
const PATH_LOSS_EXPONENT: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryReading {
    Unknown,
    Percentage(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalStrength {
    Unknown,
    Measured(i16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransmitPower {
    Unknown,
    Reported(i16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceClass {
    Unknown,
    Reported(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Manufacturer {
    Unknown,
    Identified(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DistanceEstimate {
    Unknown,
    Estimated(Length),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceSnapshot {
    pub location: DeviceLocation,
    pub alias: String,
    pub icon_name: String,
    pub signal_strength: SignalStrength,
    pub transmit_power: TransmitPower,
    pub reported_battery: BatteryReading,
    pub connected: bool,
    pub paired: bool,
    pub bonded: bool,
    pub trusted: bool,
    pub blocked: bool,
    pub legacy_pairing: bool,
    pub address_type: String,
    pub device_class: DeviceClass,
    pub services: BTreeSet<bluer::Uuid>,
    pub manufacturer: Manufacturer,
    pub manufacturer_data: HashMap<u16, Vec<u8>>,
    pub service_data: HashMap<bluer::Uuid, Vec<u8>>,
    pub appearance: DeviceAppearance,
    pub kernel_input: KernelInputBinding,
}

fn identify_manufacturer(manufacturer_data: &HashMap<u16, Vec<u8>>) -> Manufacturer {
    manufacturer_data
        .keys()
        .max()
        .and_then(|company_identifier| bluer::id::Manufacturer::try_from(*company_identifier).ok())
        .map_or(Manufacturer::Unknown, |manufacturer| {
            Manufacturer::Identified(manufacturer.to_string())
        })
}

fn signal_strength_from(rssi: Option<i16>) -> SignalStrength {
    rssi.map_or(SignalStrength::Unknown, SignalStrength::Measured)
}

fn kernel_input_for(address: bluer::Address, connected: bool) -> KernelInputBinding {
    if connected {
        find_kernel_input_devices(address)
    } else {
        KernelInputBinding::Absent
    }
}

pub fn alias_looks_like_address(alias: &str) -> bool {
    bluer::Address::from_str(&alias.replace('-', ":")).is_ok()
}

impl DeviceSnapshot {
    pub(crate) async fn read(
        device: &bluer::Device,
        location: DeviceLocation,
    ) -> Result<Self, BluetoothError> {
        let paired = device.is_paired().await?;
        let connected = device.is_connected().await?;
        let manufacturer_data = device.manufacturer_data().await?.unwrap_or_default();
        Ok(Self {
            alias: device.alias().await?,
            icon_name: device.icon().await?.unwrap_or_default(),
            signal_strength: signal_strength_from(device.rssi().await?),
            transmit_power: device
                .tx_power()
                .await?
                .map_or(TransmitPower::Unknown, TransmitPower::Reported),
            reported_battery: device
                .battery_percentage()
                .await?
                .map_or(BatteryReading::Unknown, BatteryReading::Percentage),
            connected,
            paired,
            bonded: device.is_bonded().await?.unwrap_or(paired),
            trusted: device.is_trusted().await?,
            blocked: device.is_blocked().await?,
            legacy_pairing: device.is_legacy_pairing().await?,
            address_type: device.address_type().await?.to_string(),
            device_class: device
                .class()
                .await?
                .map_or(DeviceClass::Unknown, DeviceClass::Reported),
            services: device
                .uuids()
                .await?
                .unwrap_or_default()
                .into_iter()
                .collect(),
            manufacturer: identify_manufacturer(&manufacturer_data),
            manufacturer_data,
            service_data: device.service_data().await?.unwrap_or_default(),
            appearance: device
                .appearance()
                .await?
                .map_or(DeviceAppearance::Unknown, DeviceAppearance::Reported),
            kernel_input: kernel_input_for(location.address, connected),
            location,
        })
    }

    pub fn apply_property(&mut self, changed_property: DeviceProperty) -> PropertyRelevance {
        match changed_property {
            DeviceProperty::Alias(alias) => self.alias = alias,
            DeviceProperty::Icon(icon_name) => self.icon_name = icon_name,
            DeviceProperty::Rssi(rssi) => self.signal_strength = SignalStrength::Measured(rssi),
            DeviceProperty::TxPower(transmit_power) => {
                self.transmit_power = TransmitPower::Reported(transmit_power);
            }
            DeviceProperty::BatteryPercentage(battery_percent) => {
                self.reported_battery = BatteryReading::Percentage(battery_percent);
            }
            DeviceProperty::Connected(connected) => {
                self.connected = connected;
                return PropertyRelevance::ConnectionChanged;
            }
            DeviceProperty::Paired(paired) => self.paired = paired,
            DeviceProperty::Bonded(bonded) => self.bonded = bonded,
            DeviceProperty::Trusted(trusted) => self.trusted = trusted,
            DeviceProperty::Blocked(blocked) => self.blocked = blocked,
            DeviceProperty::LegacyPairing(legacy_pairing) => self.legacy_pairing = legacy_pairing,
            DeviceProperty::Class(device_class) => {
                self.device_class = DeviceClass::Reported(device_class);
            }
            DeviceProperty::Uuids(services) => self.services = services.into_iter().collect(),
            DeviceProperty::ManufacturerData(manufacturer_data) => {
                self.manufacturer = identify_manufacturer(&manufacturer_data);
                self.manufacturer_data = manufacturer_data;
            }
            DeviceProperty::ServiceData(service_data) => self.service_data = service_data,
            DeviceProperty::Appearance(appearance_value) => {
                self.appearance = DeviceAppearance::Reported(appearance_value);
            }
            _ => return PropertyRelevance::Ignored,
        }
        PropertyRelevance::Displayed
    }

    pub(crate) fn refresh_kernel_input(&mut self) {
        self.kernel_input = kernel_input_for(self.location.address, self.connected);
    }

    pub fn address(&self) -> bluer::Address {
        self.location.address
    }

    pub fn has_meaningful_name(&self) -> bool {
        !alias_looks_like_address(&self.alias)
    }

    pub fn device_guess(&self) -> Option<DeviceGuess> {
        guess_device_kind(
            &self.manufacturer_data,
            &self.service_data,
            &self.services,
            self.appearance,
        )
    }

    pub fn display_name(&self) -> &str {
        if self.has_meaningful_name() {
            &self.alias
        } else {
            self.device_guess()
                .map_or(UNKNOWN_DEVICE_NAME, |device_guess| device_guess.label)
        }
    }

    pub fn short_address(&self) -> String {
        let address_octets = self.address().0;
        format!("{:02X}:{:02X}", address_octets[4], address_octets[5])
    }

    pub fn battery(&self) -> BatteryReading {
        match (self.reported_battery, &self.kernel_input) {
            (BatteryReading::Percentage(battery_percent), _) => {
                BatteryReading::Percentage(battery_percent)
            }
            (BatteryReading::Unknown, KernelInputBinding::Bound(kernel_input_devices)) => {
                kernel_input_devices.battery
            }
            (BatteryReading::Unknown, KernelInputBinding::Absent) => BatteryReading::Unknown,
        }
    }

    pub fn is_gamepad(&self) -> bool {
        self.icon_name == "input-gaming"
    }

    pub fn is_input_device(&self) -> bool {
        self.services.contains(&HUMAN_INTERFACE_DEVICE_SERVICE)
            || self.icon_name.starts_with("input-")
    }

    pub fn offers_object_push(&self) -> bool {
        self.offers_capability(DeviceCapability::ReceivesFiles)
    }

    pub fn offers_capability(&self, capability: DeviceCapability) -> bool {
        capability.is_offered_by(&self.advertised_services())
    }

    pub fn advertised_services(&self) -> BTreeSet<bluer::Uuid> {
        self.services
            .iter()
            .chain(self.service_data.keys())
            .copied()
            .collect()
    }

    pub fn made_by_apple(&self) -> bool {
        matches!(&self.manufacturer, Manufacturer::Identified(manufacturer_name) if manufacturer_name.starts_with("Apple"))
    }

    pub fn service_names(&self) -> Vec<String> {
        self.advertised_services()
            .into_iter()
            .filter_map(describe_service)
            .collect()
    }

    pub fn distance_estimate(&self) -> DistanceEstimate {
        let SignalStrength::Measured(rssi) = self.signal_strength else {
            return DistanceEstimate::Unknown;
        };
        let rssi_at_one_meter = match self.transmit_power {
            TransmitPower::Reported(transmit_power) => {
                transmit_power.saturating_sub(FREE_SPACE_LOSS_AT_ONE_METER)
            }
            TransmitPower::Unknown => FALLBACK_RSSI_AT_ONE_METER,
        };
        let path_loss = f32::from(rssi_at_one_meter) - f32::from(rssi);
        DistanceEstimate::Estimated(Length::new::<meter>(
            10f32.powf(path_loss / (10.0 * PATH_LOSS_EXPONENT)),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyRelevance {
    Displayed,
    ConnectionChanged,
    Ignored,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample_snapshot() -> DeviceSnapshot {
        DeviceSnapshot {
            location: DeviceLocation {
                adapter_name: "hci0".to_string(),
                address: bluer::Address::new([0x10, 0x20, 0x30, 0x40, 0x50, 0x60]),
            },
            alias: "DualSense Wireless Controller".to_string(),
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

    #[test]
    fn address_shaped_alias_is_shown_as_unknown_device() {
        let mut snapshot = sample_snapshot();
        snapshot.alias = "10-20-30-40-50-60".to_string();
        assert_eq!(snapshot.display_name(), UNKNOWN_DEVICE_NAME);
        assert!(!snapshot.has_meaningful_name());
    }

    #[test]
    fn nameless_device_is_named_after_its_guess() {
        let mut snapshot = sample_snapshot();
        snapshot.alias = "10-20-30-40-50-60".to_string();
        snapshot.manufacturer_data = HashMap::from([(0x004C, vec![0x07, 0x19])]);
        assert_eq!(snapshot.display_name(), "Apple AirPods or Beats");
        assert!(!snapshot.has_meaningful_name());
    }

    #[test]
    fn short_address_shows_last_two_octets() {
        assert_eq!(sample_snapshot().short_address(), "50:60");
    }

    #[test]
    fn applying_connected_requests_kernel_refresh() {
        let mut snapshot = sample_snapshot();
        assert_eq!(
            snapshot.apply_property(DeviceProperty::Connected(true)),
            PropertyRelevance::ConnectionChanged
        );
        assert!(snapshot.connected);
    }

    #[test]
    fn applying_rssi_updates_signal_strength() {
        let mut snapshot = sample_snapshot();
        assert_eq!(
            snapshot.apply_property(DeviceProperty::Rssi(-70)),
            PropertyRelevance::Displayed
        );
        assert_eq!(snapshot.signal_strength, SignalStrength::Measured(-70));
    }

    #[test]
    fn distance_needs_a_signal_measurement() {
        let mut snapshot = sample_snapshot();
        assert_eq!(snapshot.distance_estimate(), DistanceEstimate::Unknown);
        snapshot.signal_strength = SignalStrength::Measured(FALLBACK_RSSI_AT_ONE_METER);
        assert_eq!(
            snapshot.distance_estimate(),
            DistanceEstimate::Estimated(Length::new::<meter>(1.0))
        );
    }

    #[test]
    fn advertised_transmit_power_is_reduced_by_the_loss_over_one_meter() {
        let mut snapshot = sample_snapshot();
        snapshot.transmit_power = TransmitPower::Reported(12);
        snapshot.signal_strength = SignalStrength::Measured(12 - FREE_SPACE_LOSS_AT_ONE_METER);
        assert_eq!(
            snapshot.distance_estimate(),
            DistanceEstimate::Estimated(Length::new::<meter>(1.0))
        );
        snapshot.signal_strength = SignalStrength::Measured(-68);
        let DistanceEstimate::Estimated(distance) = snapshot.distance_estimate() else {
            panic!("a measured signal gives a distance");
        };
        assert!((15.0..30.0).contains(&distance.get::<meter>()));
    }

    #[test]
    fn reported_battery_wins_over_kernel_battery() {
        let mut snapshot = sample_snapshot();
        snapshot.kernel_input = KernelInputBinding::Bound(
            crate::remote_device::linux_kernel_input_devices::KernelInputDevices {
                driver_name: "playstation".to_string(),
                input_nodes: Vec::new(),
                hidraw_nodes: Vec::new(),
                battery: BatteryReading::Percentage(40),
            },
        );
        assert_eq!(snapshot.battery(), BatteryReading::Percentage(40));
        snapshot.reported_battery = BatteryReading::Percentage(80);
        assert_eq!(snapshot.battery(), BatteryReading::Percentage(80));
    }
}
