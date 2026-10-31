mod about;
mod adapter_settings;
mod app_preferences;
mod app_window;
mod device_list;
mod device_page;
mod device_status;
mod main_view;
mod pairing_presentation;
mod pairing_prompts;
mod pairing_timeline;
mod transfers_popover;
mod widgets;

use relm4::adw;
use relm4::gtk::gio;
use relm4::gtk::prelude::*;

use crate::app_preferences::{APPLICATION_ID, AppPreferences};
use crate::app_window::AppWindow;

fn main() {
    gio::resources_register_include!("overskride.gresource")
        .expect("the gresource is embedded at build time");
    let application = adw::Application::builder()
        .application_id(APPLICATION_ID)
        .build();
    application.set_accels_for_action("win.refresh-devices", &["<Control>r"]);
    application.set_accels_for_action("window.close", &["<Control>q"]);
    let relm_application = relm4::RelmApp::from_app(application);
    relm_application.run_async::<AppWindow>(AppPreferences::load());
}
