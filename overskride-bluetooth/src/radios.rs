use std::ffi::OsStr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioDescription {
    pub adapter_name: String,
    pub radio_label: String,
    pub hardware: RadioHardware,
}

impl RadioDescription {
    pub fn radio_subtitle(&self) -> &str {
        if self.radio_label == self.adapter_name {
            self.hardware.attachment.describe_attachment()
        } else {
            &self.adapter_name
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RadioAttachment {
    Usb,
    Pci,
    Sdio,
    Serial,
    #[default]
    Unknown,
}

impl RadioAttachment {
    fn from_subsystem_name(subsystem_name: &str) -> Self {
        match subsystem_name {
            "usb" => Self::Usb,
            "pci" => Self::Pci,
            "sdio" => Self::Sdio,
            "serial" | "serial-base" | "tty" => Self::Serial,
            _ => Self::Unknown,
        }
    }

    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Usb => "media-removable-symbolic",
            Self::Pci | Self::Sdio | Self::Serial | Self::Unknown => "computer-symbolic",
        }
    }

    pub fn describe_attachment(self) -> &'static str {
        match self {
            Self::Usb => "USB",
            Self::Pci => "Built-in (PCI)",
            Self::Sdio => "Built-in (SDIO)",
            Self::Serial => "Built-in (serial)",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RadioHardware {
    pub attachment: RadioAttachment,
    pub vendor_name: Option<String>,
    pub model_name: Option<String>,
    pub kernel_driver: Option<String>,
    pub hardware_id: Option<String>,
}

fn non_empty_text(raw_value: Option<&OsStr>) -> Option<String> {
    let trimmed_text = raw_value?.to_str()?.trim();
    (!trimmed_text.is_empty()).then(|| trimmed_text.to_string())
}

fn strip_hex_prefix(hex_text: String) -> String {
    hex_text
        .strip_prefix("0x")
        .map_or_else(|| hex_text.clone(), str::to_string)
}

fn lookup_hwdb_model(identity_device: &udev::Device) -> Option<String> {
    let modalias = identity_device.property_value("MODALIAS")?;
    let hardware_database = udev::Hwdb::new().ok()?;
    non_empty_text(hardware_database.query_one(modalias, OsStr::new("ID_MODEL_FROM_DATABASE")))
}

fn read_usb_hardware(
    usb_device: &udev::Device,
    attachment: RadioAttachment,
    kernel_driver: Option<String>,
) -> RadioHardware {
    let usb_ids = non_empty_text(usb_device.attribute_value("idVendor"))
        .zip(non_empty_text(usb_device.attribute_value("idProduct")));
    RadioHardware {
        attachment,
        vendor_name: non_empty_text(usb_device.attribute_value("manufacturer"))
            .or_else(|| non_empty_text(usb_device.property_value("ID_VENDOR_FROM_DATABASE"))),
        model_name: non_empty_text(usb_device.attribute_value("product"))
            .or_else(|| non_empty_text(usb_device.property_value("ID_MODEL_FROM_DATABASE")))
            .or_else(|| lookup_hwdb_model(usb_device)),
        kernel_driver,
        hardware_id: usb_ids.map(|(vendor_id, product_id)| format!("{vendor_id}:{product_id}")),
    }
}

fn read_bus_hardware(
    controller_device: &udev::Device,
    attachment: RadioAttachment,
    kernel_driver: Option<String>,
) -> RadioHardware {
    let bus_ids = non_empty_text(controller_device.attribute_value("vendor"))
        .zip(non_empty_text(controller_device.attribute_value("device")));
    RadioHardware {
        attachment,
        vendor_name: non_empty_text(controller_device.property_value("ID_VENDOR_FROM_DATABASE")),
        model_name: non_empty_text(controller_device.property_value("ID_MODEL_FROM_DATABASE"))
            .or_else(|| lookup_hwdb_model(controller_device)),
        kernel_driver,
        hardware_id: bus_ids.map(|(vendor_id, device_id)| {
            format!(
                "{}:{}",
                strip_hex_prefix(vendor_id),
                strip_hex_prefix(device_id)
            )
        }),
    }
}

pub fn read_radio_hardware(adapter_name: &str) -> RadioHardware {
    let Some(controller_device) =
        udev::Device::from_subsystem_sysname("bluetooth".to_string(), adapter_name.to_string())
            .ok()
            .and_then(|adapter_device| adapter_device.parent())
    else {
        return RadioHardware::default();
    };
    let attachment = controller_device
        .subsystem()
        .and_then(OsStr::to_str)
        .map_or(
            RadioAttachment::Unknown,
            RadioAttachment::from_subsystem_name,
        );
    let kernel_driver = non_empty_text(controller_device.driver());
    let usb_device = controller_device
        .parent_with_subsystem_devtype("usb", "usb_device")
        .ok()
        .flatten();
    match &usb_device {
        Some(usb_device) if attachment == RadioAttachment::Usb => {
            read_usb_hardware(usb_device, attachment, kernel_driver)
        }
        _ => read_bus_hardware(&controller_device, attachment, kernel_driver),
    }
}

fn label_radio(adapter_name: &str, hardware: &RadioHardware) -> String {
    let product_name = match (&hardware.vendor_name, &hardware.model_name) {
        (Some(vendor_name), Some(model_name)) if model_name.starts_with(vendor_name.as_str()) => {
            Some(model_name.clone())
        }
        (Some(vendor_name), Some(model_name)) => Some(format!("{vendor_name} {model_name}")),
        (None, Some(model_name)) => Some(model_name.clone()),
        (Some(vendor_name), None) => Some(format!("{vendor_name} adapter")),
        (None, None) => None,
    };
    product_name.unwrap_or_else(|| adapter_name.to_string())
}

pub fn describe_radio(adapter_name: &str) -> RadioDescription {
    let hardware = read_radio_hardware(adapter_name);
    RadioDescription {
        adapter_name: adapter_name.to_string(),
        radio_label: label_radio(adapter_name, &hardware),
        hardware,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hardware_with_names(vendor_name: Option<&str>, model_name: Option<&str>) -> RadioHardware {
        RadioHardware {
            vendor_name: vendor_name.map(str::to_string),
            model_name: model_name.map(str::to_string),
            ..RadioHardware::default()
        }
    }

    #[test]
    fn label_combines_vendor_and_model() {
        assert_eq!(
            label_radio(
                "hci1",
                &hardware_with_names(Some("Realtek"), Some("Bluetooth Radio"))
            ),
            "Realtek Bluetooth Radio"
        );
    }

    #[test]
    fn label_skips_vendor_repeated_in_model() {
        assert_eq!(
            label_radio(
                "hci0",
                &hardware_with_names(Some("ASUS"), Some("ASUS USB-BT500"))
            ),
            "ASUS USB-BT500"
        );
    }

    #[test]
    fn label_falls_back_to_vendor_adapter() {
        assert_eq!(
            label_radio(
                "hci0",
                &hardware_with_names(Some("Intel Corporation"), None)
            ),
            "Intel Corporation adapter"
        );
    }

    #[test]
    fn label_without_names_is_adapter_name() {
        assert_eq!(label_radio("hci0", &RadioHardware::default()), "hci0");
    }

    #[test]
    fn subsystem_names_map_to_attachments() {
        assert_eq!(
            RadioAttachment::from_subsystem_name("usb"),
            RadioAttachment::Usb
        );
        assert_eq!(
            RadioAttachment::from_subsystem_name("pci"),
            RadioAttachment::Pci
        );
        assert_eq!(
            RadioAttachment::from_subsystem_name("serial-base"),
            RadioAttachment::Serial
        );
        assert_eq!(
            RadioAttachment::from_subsystem_name("platform"),
            RadioAttachment::Unknown
        );
    }
}
