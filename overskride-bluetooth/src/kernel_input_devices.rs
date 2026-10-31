use std::path::{Path, PathBuf};

use crate::device_snapshot::BatteryReading;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KernelInputNode {
    pub input_name: String,
    pub device_nodes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KernelInputDevices {
    pub driver_name: String,
    pub input_nodes: Vec<KernelInputNode>,
    pub battery: BatteryReading,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KernelInputBinding {
    Absent,
    Bound(KernelInputDevices),
}

const INPUT_CLASS_DIRECTORY: &str = "/sys/class/input";
const POWER_SUPPLY_CLASS_DIRECTORY: &str = "/sys/class/power_supply";
const DEVICE_NODE_PREFIXES: [&str; 3] = ["event", "js", "mouse"];

pub(crate) fn read_trimmed_attribute(attribute_path: &Path) -> Option<String> {
    let attribute_text = std::fs::read_to_string(attribute_path).ok()?;
    let trimmed_text = attribute_text.trim();
    (!trimmed_text.is_empty()).then(|| trimmed_text.to_string())
}

fn sorted_directory_entries(directory_path: &Path) -> Vec<PathBuf> {
    let mut entry_paths: Vec<_> = std::fs::read_dir(directory_path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|directory_entry| directory_entry.path())
        .collect();
    entry_paths.sort();
    entry_paths
}

fn is_device_node_name(file_name: &str) -> bool {
    DEVICE_NODE_PREFIXES.iter().any(|node_prefix| {
        file_name
            .strip_prefix(node_prefix)
            .is_some_and(|node_number| {
                !node_number.is_empty()
                    && node_number
                        .chars()
                        .all(|character| character.is_ascii_digit())
            })
    })
}

fn read_battery_for_hid_device(hid_device_directory: &Path) -> Option<u8> {
    let power_supply_directory = sorted_directory_entries(Path::new(POWER_SUPPLY_CLASS_DIRECTORY))
        .into_iter()
        .find(|power_supply_directory| {
            std::fs::canonicalize(power_supply_directory.join("device"))
                .ok()
                .as_deref()
                == Some(hid_device_directory)
        })?;
    let capacity_text = read_trimmed_attribute(&power_supply_directory.join("capacity"))?;
    capacity_text.parse().ok()
}

fn read_driver_name(hid_device_directory: &Path) -> Option<String> {
    let driver_directory = std::fs::canonicalize(hid_device_directory.join("driver")).ok()?;
    Some(driver_directory.file_name()?.to_string_lossy().to_string())
}

fn read_input_node(input_directory: &Path) -> KernelInputNode {
    let device_nodes = sorted_directory_entries(input_directory)
        .into_iter()
        .filter_map(|entry_path| Some(entry_path.file_name()?.to_string_lossy().to_string()))
        .filter(|file_name| is_device_node_name(file_name))
        .collect();
    KernelInputNode {
        input_name: read_trimmed_attribute(&input_directory.join("name")).unwrap_or_default(),
        device_nodes,
    }
}

pub fn find_kernel_input_devices(address: bluer::Address) -> KernelInputBinding {
    let address_text = address.to_string().to_lowercase();
    let matching_input_directories: Vec<_> =
        sorted_directory_entries(Path::new(INPUT_CLASS_DIRECTORY))
            .into_iter()
            .filter(|input_directory| {
                read_trimmed_attribute(&input_directory.join("uniq")).is_some_and(
                    |unique_identifier| unique_identifier.to_lowercase() == address_text,
                )
            })
            .collect();
    if matching_input_directories.is_empty() {
        return KernelInputBinding::Absent;
    }

    let hid_device_directories: Vec<_> = matching_input_directories
        .iter()
        .filter_map(|input_directory| std::fs::canonicalize(input_directory.join("device")).ok())
        .collect();
    let driver_name = hid_device_directories
        .iter()
        .find_map(|hid_device_directory| read_driver_name(hid_device_directory))
        .unwrap_or_default();
    let battery = hid_device_directories
        .iter()
        .find_map(|hid_device_directory| read_battery_for_hid_device(hid_device_directory))
        .map_or(BatteryReading::Unknown, |battery_percent| {
            BatteryReading::Percentage(battery_percent.min(100))
        });

    KernelInputBinding::Bound(KernelInputDevices {
        driver_name,
        input_nodes: matching_input_directories
            .iter()
            .map(|input_directory| read_input_node(input_directory))
            .collect(),
        battery,
    })
}

#[cfg(test)]
mod tests {
    use super::is_device_node_name;

    #[test]
    fn device_node_names_need_a_number_suffix() {
        assert!(is_device_node_name("event12"));
        assert!(is_device_node_name("js0"));
        assert!(!is_device_node_name("event"));
        assert!(!is_device_node_name("eventx"));
        assert!(!is_device_node_name("power"));
    }
}
