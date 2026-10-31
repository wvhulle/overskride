mod about;
mod adapter_page;
mod adapter_row;
mod app_actions;
mod app_preferences;
mod app_settings_page;
mod app_window;
mod attention_notifications;
mod capability_overview;
mod device_list;
mod device_page;
mod device_status;
#[allow(dead_code)]
mod icon_names {
    include!(concat!(env!("OUT_DIR"), "/icon_names.rs"));
}
mod login_autostart;
mod main_view;
mod pairing_presentation;
mod pairing_timeline;
mod prompt_center;
mod transfers_popover;
mod tray_icon;
mod widgets;

use relm4::adw;
use relm4::gtk::prelude::*;

use crate::app_preferences::{APPLICATION_ID, AppPreferences};
use crate::app_window::{AppWindow, AppWindowInit};
use crate::login_autostart::BACKGROUND_FLAG;

fn main() {
    relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    let (background_flags, forwarded_arguments): (Vec<String>, Vec<String>) =
        std::env::args().partition(|argument| argument == BACKGROUND_FLAG);
    let started_in_background = !background_flags.is_empty();
    let application = adw::Application::builder()
        .application_id(APPLICATION_ID)
        .build();
    application.set_accels_for_action("win.refresh-devices", &["<Control>r"]);
    application.set_accels_for_action("window.close", &["<Control>w"]);
    application.set_accels_for_action("app.quit", &["<Control>q"]);
    let relm_application = relm4::RelmApp::from_app(application)
        .visible_on_activate(!started_in_background)
        .with_args(forwarded_arguments);
    relm_application.run_async::<AppWindow>(AppWindowInit {
        preferences: AppPreferences::load(),
    });
}
