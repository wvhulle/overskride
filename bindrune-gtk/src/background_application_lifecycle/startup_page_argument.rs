const OPEN_DEVICE_FLAG: &str = "--open-device=";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupPage {
    AdapterOverview,
    Device(bindrune_bluez::Address),
}

pub fn is_startup_page_argument(argument: &str) -> bool {
    argument.starts_with(OPEN_DEVICE_FLAG)
}

pub fn startup_page_from_arguments(startup_arguments: &[String]) -> StartupPage {
    startup_arguments
        .iter()
        .filter_map(|argument| argument.strip_prefix(OPEN_DEVICE_FLAG))
        .find_map(|address_text| address_text.parse().ok())
        .map_or(StartupPage::AdapterOverview, StartupPage::Device)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_device_argument_selects_that_device() {
        let startup_arguments = vec!["--open-device=10:20:30:40:50:60".to_string()];
        assert_eq!(
            startup_page_from_arguments(&startup_arguments),
            StartupPage::Device(bindrune_bluez::Address::new([
                0x10, 0x20, 0x30, 0x40, 0x50, 0x60
            ]))
        );
    }

    #[test]
    fn malformed_address_falls_back_to_the_adapter_overview() {
        let startup_arguments = vec!["--open-device=not-an-address".to_string()];
        assert_eq!(
            startup_page_from_arguments(&startup_arguments),
            StartupPage::AdapterOverview
        );
    }
}
