use bindrune_bluez::remote_device::linux_kernel_input_devices::{
    KernelInputBinding, KernelInputDevices,
};

use crate::{
    device_detail::pairing_step_timeline::pairing_timeline_view_model::{
        PropertyRow, StepButtonSlot, StepNodeState, StepParts, TimelineInputs, hinted_property,
        stacked_property,
    },
    shared_device_appearance::input_device_missing,
};

impl TimelineInputs<'_> {
    pub(super) fn input_device_step(&self, default_state: StepNodeState) -> StepParts {
        let snapshot = self.snapshot;
        let (status_text, property_rows) = match &snapshot.kernel_input {
            KernelInputBinding::Bound(kernel_input_devices) => {
                let input_count = kernel_input_devices.input_nodes.len();
                let plural_suffix = if input_count == 1 { "" } else { "s" };
                (
                    format!("{input_count} input device{plural_suffix}"),
                    kernel_input_rows(kernel_input_devices),
                )
            }
            KernelInputBinding::Absent if snapshot.connected => (
                "No input device".to_string(),
                vec![hinted_property(
                    "Input devices",
                    "None",
                    "Check the kernel log (dmesg)",
                )],
            ),
            KernelInputBinding::Absent => (
                "Not connected".to_string(),
                vec![hinted_property(
                    "Input devices",
                    "None yet",
                    "Created automatically once connected",
                )],
            ),
        };
        StepParts {
            node_state: if input_device_missing(snapshot) {
                StepNodeState::Warning
            } else {
                default_state
            },
            status_text,
            button: StepButtonSlot::Hidden,
            property_rows,
        }
    }
}

fn kernel_input_rows(kernel_input_devices: &KernelInputDevices) -> Vec<PropertyRow> {
    let main_input_name = kernel_input_devices
        .input_nodes
        .first()
        .map(|input_node| input_node.input_name.clone())
        .unwrap_or_default();
    let node_rows = kernel_input_devices.input_nodes.iter().map(|input_node| {
        let input_label = input_node
            .input_name
            .strip_prefix(main_input_name.as_str())
            .map(str::trim)
            .filter(|name_suffix| !name_suffix.is_empty())
            .unwrap_or("Main");
        let device_paths: Vec<_> = input_node
            .device_nodes
            .iter()
            .map(|device_node| format!("/dev/input/{device_node}"))
            .collect();
        stacked_property(input_label, device_paths.join(", "))
    });
    std::iter::once(hinted_property(
        "Driver",
        kernel_input_devices.driver_name.clone(),
        "Linux HID driver",
    ))
    .chain(node_rows)
    .collect()
}
