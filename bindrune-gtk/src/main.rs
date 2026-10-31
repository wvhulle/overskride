mod adapter_detail;
mod application_preferences_page;
mod background_application_lifecycle;
mod device_detail;
mod header_transfer_popover;
mod incoming_request_prompts;
mod shared_device_appearance;
mod shared_page_widget_templates;
mod sidebar_content_split;
mod sidebar_navigation;
#[allow(dead_code)]
mod icon_names {
    include!(concat!(env!("OUT_DIR"), "/icon_names.rs"));
}

use relm4::{adw, gtk::prelude::*};

use crate::background_application_lifecycle::{
    gsettings_stored_preferences::{APPLICATION_ID, GsettingsStoredPreferences},
    login_autostart_entry::BACKGROUND_FLAG,
    main_window_shell::{MainWindowShell, MainWindowShellInit},
    startup_page_argument::{is_startup_page_argument, startup_page_from_arguments},
};

fn main() {
    relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    let (background_flags, remaining_arguments): (Vec<String>, Vec<String>) =
        std::env::args().partition(|argument| argument == BACKGROUND_FLAG);
    let (startup_page_arguments, forwarded_arguments): (Vec<String>, Vec<String>) =
        remaining_arguments
            .into_iter()
            .partition(|argument| is_startup_page_argument(argument));
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
    relm_application.run_async::<MainWindowShell>(MainWindowShellInit {
        preferences: GsettingsStoredPreferences::load(),
        startup_page: startup_page_from_arguments(&startup_page_arguments),
    });
}
