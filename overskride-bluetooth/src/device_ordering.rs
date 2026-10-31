use std::cmp::Ordering;

use crate::device_snapshot::{DeviceSnapshot, SignalStrength};

fn signal_rank(signal_strength: SignalStrength) -> i32 {
    match signal_strength {
        SignalStrength::Measured(rssi) => i32::from(rssi),
        SignalStrength::Unknown => i32::MIN,
    }
}

pub fn compare_devices(first_device: &DeviceSnapshot, second_device: &DeviceSnapshot) -> Ordering {
    second_device
        .connected
        .cmp(&first_device.connected)
        .then_with(|| second_device.paired.cmp(&first_device.paired))
        .then_with(|| second_device.has_meaningful_name().cmp(&first_device.has_meaningful_name()))
        .then_with(|| signal_rank(second_device.signal_strength).cmp(&signal_rank(first_device.signal_strength)))
        .then_with(|| first_device.display_name().cmp(second_device.display_name()))
        .then_with(|| first_device.address().cmp(&second_device.address()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_snapshot::tests::sample_snapshot;

    fn named_snapshot(alias: &str, last_address_byte: u8) -> DeviceSnapshot {
        let mut snapshot = sample_snapshot();
        snapshot.alias = alias.to_string();
        snapshot.location.address = bluer::Address::new([0, 0, 0, 0, 0, last_address_byte]);
        snapshot
    }

    #[test]
    fn connected_devices_come_first() {
        let mut connected_device = named_snapshot("Zebra", 1);
        connected_device.connected = true;
        let idle_device = named_snapshot("Aardvark", 2);
        assert_eq!(compare_devices(&connected_device, &idle_device), Ordering::Less);
    }

    #[test]
    fn stronger_signal_sorts_earlier() {
        let mut near_device = named_snapshot("Near", 1);
        near_device.signal_strength = SignalStrength::Measured(-40);
        let mut far_device = named_snapshot("Far", 2);
        far_device.signal_strength = SignalStrength::Measured(-90);
        assert_eq!(compare_devices(&near_device, &far_device), Ordering::Less);
    }

    #[test]
    fn unnamed_devices_sort_after_named_ones() {
        let named_device = named_snapshot("Speaker", 1);
        let unnamed_device = named_snapshot("00-00-00-00-00-02", 2);
        assert_eq!(compare_devices(&named_device, &unnamed_device), Ordering::Less);
    }

    #[test]
    fn ordering_is_total_for_equal_devices() {
        let first_device = named_snapshot("Same", 1);
        let second_device = named_snapshot("Same", 2);
        assert_eq!(compare_devices(&first_device, &second_device), Ordering::Less);
        assert_eq!(compare_devices(&first_device, &first_device), Ordering::Equal);
    }
}
