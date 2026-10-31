use std::path::PathBuf;

use relm4::gtk::gio;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;

pub const APPLICATION_ID: &str = "io.github.kaii_lb.Overskride";
const RECEIVED_FILES_SUBFOLDER: &str = "Bluetooth";
const AUTO_ACCEPT_SENDERS_KEY: &str = "auto-accept-senders";

#[derive(Clone, Debug)]
pub struct AppPreferences {
    settings: gio::Settings,
}

fn installed_settings() -> Option<gio::Settings> {
    let installed_source = gio::SettingsSchemaSource::default()?;
    installed_source.lookup(APPLICATION_ID, true)?;
    Some(gio::Settings::new(APPLICATION_ID))
}

fn bundled_settings() -> gio::Settings {
    let installed_source = gio::SettingsSchemaSource::default();
    let bundled_source = gio::SettingsSchemaSource::from_directory(
        env!("OVERSKRIDE_COMPILED_SCHEMAS"),
        installed_source.as_ref(),
        false,
    )
    .expect("the settings schema is compiled at build time");
    let Some(bundled_schema) = bundled_source.lookup(APPLICATION_ID, false) else {
        panic!("the compiled schema directory lacks {APPLICATION_ID}");
    };
    gio::Settings::new_full(&bundled_schema, None::<&gio::SettingsBackend>, None)
}

impl AppPreferences {
    pub fn load() -> Self {
        Self {
            settings: installed_settings().unwrap_or_else(bundled_settings),
        }
    }

    pub fn settings(&self) -> &gio::Settings {
        &self.settings
    }

    pub fn show_unnamed_devices(&self) -> bool {
        self.settings.boolean("show-unnamed-devices")
    }

    pub fn set_show_unnamed_devices(&self, show_unnamed_devices: bool) {
        let _unwritable_setting_is_not_fatal = self
            .settings
            .set_boolean("show-unnamed-devices", show_unnamed_devices);
    }

    pub fn accepts_files_from(&self, sender_address: overskride_bluetooth::Address) -> bool {
        let sender_address = sender_address.to_string();
        self.settings
            .strv(AUTO_ACCEPT_SENDERS_KEY)
            .iter()
            .any(|accepted_address| accepted_address.as_str() == sender_address)
    }

    pub fn allow_auto_accept(&self, sender_address: overskride_bluetooth::Address) {
        if self.accepts_files_from(sender_address) {
            return;
        }
        let mut accepted_addresses = self.auto_accept_addresses();
        accepted_addresses.push(sender_address.to_string());
        self.store_auto_accept_addresses(&accepted_addresses);
    }

    pub fn revoke_auto_accept(&self, sender_address: overskride_bluetooth::Address) {
        let sender_address = sender_address.to_string();
        let remaining_addresses = self
            .auto_accept_addresses()
            .into_iter()
            .filter(|accepted_address| *accepted_address != sender_address)
            .collect::<Vec<_>>();
        self.store_auto_accept_addresses(&remaining_addresses);
    }

    pub fn connect_auto_accept_changed<F: Fn() + 'static>(&self, on_auto_accept_changed: F) {
        self.settings.connect_changed(
            Some(AUTO_ACCEPT_SENDERS_KEY),
            move |_changed_settings, _setting_key| on_auto_accept_changed(),
        );
    }

    fn auto_accept_addresses(&self) -> Vec<String> {
        self.settings
            .strv(AUTO_ACCEPT_SENDERS_KEY)
            .iter()
            .map(|accepted_address| accepted_address.to_string())
            .collect()
    }

    fn store_auto_accept_addresses(&self, accepted_addresses: &[String]) {
        let address_slices = accepted_addresses
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let _unwritable_setting_is_not_fatal = self
            .settings
            .set_strv(AUTO_ACCEPT_SENDERS_KEY, address_slices.as_slice());
    }

    pub fn remembered_adapter_name(&self) -> String {
        self.settings.string("current-adapter-name").to_string()
    }

    pub fn remember_adapter_name(&self, adapter_name: &str) {
        let _unwritable_setting_is_not_fatal = self
            .settings
            .set_string("current-adapter-name", adapter_name);
    }

    pub fn received_files_folder(&self) -> PathBuf {
        let stored_folder = self.settings.string("store-folder");
        if stored_folder.is_empty() {
            return glib::user_special_dir(glib::UserDirectory::Downloads)
                .unwrap_or_else(glib::home_dir)
                .join(RECEIVED_FILES_SUBFOLDER);
        }
        PathBuf::from(stored_folder.as_str())
    }

    pub fn set_received_files_folder(&self, received_files_folder: &std::path::Path) {
        let _unwritable_setting_is_not_fatal = self
            .settings
            .set_string("store-folder", &received_files_folder.to_string_lossy());
    }
}
