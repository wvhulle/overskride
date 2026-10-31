use std::path::PathBuf;

use relm4::gtk::{gio, glib, prelude::*};

pub const APPLICATION_ID: &str = "io.github.kaii_lb.Overskride";
const RECEIVED_FILES_SUBFOLDER: &str = "Bluetooth";
const AUTO_ACCEPT_SENDERS_KEY: &str = "auto-accept-senders";
pub const RUN_IN_BACKGROUND_KEY: &str = "run-in-background";
pub const START_AT_LOGIN_KEY: &str = "start-at-login";

const SHOW_UNNAMED_DEVICES_KEY: &str = "show-unnamed-devices";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreferenceChange {
    RunInBackground(bool),
    StartAtLogin(bool),
    ShowUnnamedDevices(bool),
    AutoAcceptSenders,
    Unrelated,
}

#[derive(Clone, Debug)]
pub struct GsettingsStoredPreferences {
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

fn read_preference_change(settings: &gio::Settings, setting_key: &str) -> PreferenceChange {
    match setting_key {
        RUN_IN_BACKGROUND_KEY => PreferenceChange::RunInBackground(settings.boolean(setting_key)),
        START_AT_LOGIN_KEY => PreferenceChange::StartAtLogin(settings.boolean(setting_key)),
        SHOW_UNNAMED_DEVICES_KEY => {
            PreferenceChange::ShowUnnamedDevices(settings.boolean(setting_key))
        }
        AUTO_ACCEPT_SENDERS_KEY => PreferenceChange::AutoAcceptSenders,
        _ => PreferenceChange::Unrelated,
    }
}

impl GsettingsStoredPreferences {
    pub fn load() -> Self {
        Self {
            settings: installed_settings().unwrap_or_else(bundled_settings),
        }
    }

    pub fn settings(&self) -> &gio::Settings {
        &self.settings
    }

    pub fn show_unnamed_devices(&self) -> bool {
        self.settings.boolean(SHOW_UNNAMED_DEVICES_KEY)
    }

    pub fn set_show_unnamed_devices(&self, show_unnamed_devices: bool) {
        let _unwritable_setting_is_not_fatal = self
            .settings
            .set_boolean(SHOW_UNNAMED_DEVICES_KEY, show_unnamed_devices);
    }

    pub fn run_in_background(&self) -> bool {
        self.settings.boolean(RUN_IN_BACKGROUND_KEY)
    }

    pub fn start_at_login(&self) -> bool {
        self.settings.boolean(START_AT_LOGIN_KEY)
    }

    pub fn accepts_files_from(&self, sender_address: bindrune_bluez::Address) -> bool {
        let sender_address = sender_address.to_string();
        self.settings
            .strv(AUTO_ACCEPT_SENDERS_KEY)
            .iter()
            .any(|accepted_address| accepted_address.as_str() == sender_address)
    }

    pub fn allow_auto_accept(&self, sender_address: bindrune_bluez::Address) {
        if self.accepts_files_from(sender_address) {
            return;
        }
        let mut accepted_addresses = self.auto_accept_addresses();
        accepted_addresses.push(sender_address.to_string());
        self.store_auto_accept_addresses(&accepted_addresses);
    }

    pub fn revoke_auto_accept(&self, sender_address: bindrune_bluez::Address) {
        let sender_address = sender_address.to_string();
        let remaining_addresses = self
            .auto_accept_addresses()
            .into_iter()
            .filter(|accepted_address| *accepted_address != sender_address)
            .collect::<Vec<_>>();
        self.store_auto_accept_addresses(&remaining_addresses);
    }

    pub fn connect_preference_changes<F: Fn(PreferenceChange) + 'static>(
        &self,
        on_preference_change: F,
    ) {
        self.settings
            .connect_changed(None, move |changed_settings, setting_key| {
                on_preference_change(read_preference_change(changed_settings, setting_key));
            });
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
