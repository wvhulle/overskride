use relm4::adw;
use relm4::adw::prelude::*;

use crate::app_preferences::APPLICATION_ID;

pub fn show_about() {
    let about_dialog = adw::AboutDialog::builder()
        .application_name("Overskride")
        .application_icon(APPLICATION_ID)
        .developer_name("kaii")
        .version(env!("CARGO_PKG_VERSION"))
        .developers(["kaii https://github.com/kaii-lb"])
        .copyright("© 2023 kaii")
        .license_type(relm4::gtk::License::Gpl30)
        .build();
    let parent_window = relm4::main_application().active_window();
    about_dialog.present(parent_window.as_ref());
}
