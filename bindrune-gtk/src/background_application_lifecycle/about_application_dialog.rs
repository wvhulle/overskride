use relm4::{adw, adw::prelude::*};

use crate::background_application_lifecycle::gsettings_stored_preferences::APPLICATION_ID;

pub fn show_about() {
    let about_dialog = adw::AboutDialog::builder()
        .application_name("Overskride")
        .application_icon(APPLICATION_ID)
        .developer_name("kaii and Willem Vanhulle")
        .version(env!("CARGO_PKG_VERSION"))
        .developers(["kaii https://github.com/kaii-lb", "Willem Vanhulle"])
        .copyright("© 2023 kaii, © 2026 Willem Vanhulle")
        .license_type(relm4::gtk::License::Gpl30)
        .build();
    let parent_window = relm4::main_application().active_window();
    about_dialog.present(parent_window.as_ref());
}
