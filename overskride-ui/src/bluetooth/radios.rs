use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioDescription {
    pub adapter_name: String,
    pub radio_label: String,
}

fn read_trimmed_attribute(attribute_path: &Path) -> Option<String> {
    let attribute_text = std::fs::read_to_string(attribute_path).ok()?;
    let trimmed_text = attribute_text.trim();
    (!trimmed_text.is_empty()).then(|| trimmed_text.to_string())
}

fn find_usb_product_name(adapter_name: &str) -> Option<String> {
    let device_directory = std::fs::canonicalize(PathBuf::from("/sys/class/bluetooth").join(adapter_name).join("device")).ok()?;

    device_directory.ancestors().take(3).find_map(|candidate_directory| {
        let product_name = read_trimmed_attribute(&candidate_directory.join("product"))?;
        let manufacturer_name = read_trimmed_attribute(&candidate_directory.join("manufacturer"));
        Some(match manufacturer_name {
            Some(manufacturer_name) if !product_name.starts_with(&manufacturer_name) => format!("{} {}", manufacturer_name, product_name),
            _ => product_name,
        })
    })
}

pub fn describe_radio(adapter_name: &str) -> String {
    match find_usb_product_name(adapter_name) {
        Some(product_name) => format!("{} ({})", product_name, adapter_name),
        None => adapter_name.to_string(),
    }
}

pub async fn list_radios(session: &bluer::Session) -> bluer::Result<Vec<RadioDescription>> {
    let mut adapter_names = session.adapter_names().await?;
    adapter_names.sort();

    Ok(adapter_names
        .into_iter()
        .map(|adapter_name| RadioDescription {
            radio_label: describe_radio(&adapter_name),
            adapter_name,
        })
        .collect())
}
