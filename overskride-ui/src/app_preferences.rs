use std::path::PathBuf;

use relm4::gtk::gio;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;

pub const APPLICATION_ID: &str = "io.github.kaii_lb.Overskride";
const RECEIVED_FILES_SUBFOLDER: &str = "Bluetooth";

#[derive(Clone, Debug)]
pub struct AppPreferences {
    settings: gio::Settings,
}

impl AppPreferences {
    pub fn load() -> Self {
        Self {
            settings: gio::Settings::new(APPLICATION_ID),
        }
    }

    pub fn settings(&self) -> &gio::Settings {
        &self.settings
    }

    pub fn hide_unknown_devices(&self) -> bool {
        self.settings.boolean("hide-unknown-devices")
    }

    pub fn auto_accept_from_trusted(&self) -> bool {
        self.settings.boolean("auto-accept-from-trusted")
    }

    pub fn keep_accepting_after_first_file(&self) -> bool {
        self.settings.boolean("first-auto-accept")
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
