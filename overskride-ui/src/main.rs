mod about;
mod adapter_page;
mod adapter_row;
mod app_actions;
mod app_preferences;
mod app_settings_page;
mod app_window;
mod attention_dialogs;
mod capability_overview;
mod device_list;
mod device_page;
mod device_status;
mod login_autostart;
mod main_view;
mod pairing_presentation;
mod pairing_prompts;
mod pairing_timeline;
mod transfers_popover;
mod tray_icon;
mod widgets;

use std::cell::Cell;

use relm4::adw;
use relm4::gtk::gio;
use relm4::gtk::prelude::*;

use crate::app_actions::{present_main_window, register_app_actions};
use crate::app_preferences::{APPLICATION_ID, AppPreferences};
use crate::app_window::AppWindow;
use crate::login_autostart::BACKGROUND_FLAG;

fn main() {
    gio::resources_register_include!("overskride.gresource")
        .expect("the gresource is embedded at build time");
    let (background_flags, forwarded_arguments): (Vec<String>, Vec<String>) =
        std::env::args().partition(|argument| argument == BACKGROUND_FLAG);
    let started_in_background = !background_flags.is_empty();
    let application = adw::Application::builder()
        .application_id(APPLICATION_ID)
        .build();
    application.set_accels_for_action("win.refresh-devices", &["<Control>r"]);
    application.set_accels_for_action("window.close", &["<Control>w"]);
    application.set_accels_for_action("app.quit", &["<Control>q"]);
    let first_activation_pending = Cell::new(true);
    application.connect_activate(move |_activated_application| {
        let stay_hidden = first_activation_pending.replace(false) && started_in_background;
        if !stay_hidden {
            present_main_window();
        }
    });
    let relm_application = relm4::RelmApp::from_app(application)
        .visible_on_activate(false)
        .with_args(forwarded_arguments);
    register_app_actions();
    relm_application.run_async::<AppWindow>(AppPreferences::load());
}
