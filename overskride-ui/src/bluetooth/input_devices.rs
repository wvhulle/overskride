use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct KernelInputNode {
    pub input_name: String,
    pub device_nodes: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct KernelInputDevices {
    pub driver_name: String,
    pub input_nodes: Vec<KernelInputNode>,
    pub battery_percent: Option<u8>,
}

fn read_trimmed_attribute(attribute_path: &Path) -> Option<String> {
    let attribute_text = std::fs::read_to_string(attribute_path).ok()?;
    let trimmed_text = attribute_text.trim();
    (!trimmed_text.is_empty()).then(|| trimmed_text.to_string())
}

fn list_directory_entries(directory_path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(directory_path)
        .map(|directory_entries| {
            directory_entries
                .filter_map(|directory_entry| directory_entry.ok())
                .map(|directory_entry| directory_entry.path())
                .collect()
        })
        .unwrap_or_default()
}

fn is_device_node_name(file_name: &str) -> bool {
    ["event", "js", "mouse"].iter().any(|node_prefix| {
        file_name
            .strip_prefix(node_prefix)
            .is_some_and(|node_number| !node_number.is_empty() && node_number.chars().all(|character| character.is_ascii_digit()))
    })
}

fn read_battery_for_hid_device(hid_device_directory: &Path) -> Option<u8> {
    list_directory_entries(Path::new("/sys/class/power_supply"))
        .into_iter()
        .find(|power_supply_directory| {
            std::fs::canonicalize(power_supply_directory.join("device")).ok().as_deref() == Some(hid_device_directory)
        })
        .and_then(|power_supply_directory| read_trimmed_attribute(&power_supply_directory.join("capacity")))
        .and_then(|capacity_text| capacity_text.parse().ok())
}

pub fn find_kernel_input_devices(address: bluer::Address) -> Option<KernelInputDevices> {
    let address_text = address.to_string().to_lowercase();
    let mut kernel_input_devices: Option<KernelInputDevices> = None;
    let mut input_directories = list_directory_entries(Path::new("/sys/class/input"));
    input_directories.sort();

    for input_directory in input_directories {
        let unique_identifier = read_trimmed_attribute(&input_directory.join("uniq"));
        if unique_identifier.map(|identifier| identifier.to_lowercase()) != Some(address_text.clone()) {
            continue;
        }

        let hid_device_directory = std::fs::canonicalize(input_directory.join("device")).ok();
        let found_devices = kernel_input_devices.get_or_insert_with(KernelInputDevices::default);

        if found_devices.driver_name.is_empty() {
            if let Some(hid_device_directory) = &hid_device_directory {
                found_devices.driver_name = std::fs::canonicalize(hid_device_directory.join("driver"))
                    .ok()
                    .and_then(|driver_directory| {
                        driver_directory
                            .file_name()
                            .map(|driver_name| driver_name.to_string_lossy().to_string())
                    })
                    .unwrap_or_default();
            }
        }
        if found_devices.battery_percent.is_none() {
            found_devices.battery_percent = hid_device_directory.as_deref().and_then(read_battery_for_hid_device);
        }

        let mut device_nodes: Vec<String> = list_directory_entries(&input_directory)
            .into_iter()
            .filter_map(|entry_path| entry_path.file_name().map(|file_name| file_name.to_string_lossy().to_string()))
            .filter(|file_name| is_device_node_name(file_name))
            .collect();
        device_nodes.sort();
        found_devices.input_nodes.push(KernelInputNode {
            input_name: read_trimmed_attribute(&input_directory.join("name")).unwrap_or_default(),
            device_nodes,
        });
    }

    kernel_input_devices
}
