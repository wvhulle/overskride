use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

#[derive(Clone, Copy, Debug)]
pub struct AlertChoice {
    pub response_id: &'static str,
    pub label: &'static str,
    pub appearance: adw::ResponseAppearance,
}

pub const CANCEL_CHOICE: AlertChoice = AlertChoice {
    response_id: "cancel",
    label: "Cancel",
    appearance: adw::ResponseAppearance::Default,
};

pub const fn plain_choice(response_id: &'static str, label: &'static str) -> AlertChoice {
    AlertChoice {
        response_id,
        label,
        appearance: adw::ResponseAppearance::Default,
    }
}

pub const fn suggested_choice(response_id: &'static str, label: &'static str) -> AlertChoice {
    AlertChoice {
        response_id,
        label,
        appearance: adw::ResponseAppearance::Suggested,
    }
}

pub const fn destructive_choice(response_id: &'static str, label: &'static str) -> AlertChoice {
    AlertChoice {
        response_id,
        label,
        appearance: adw::ResponseAppearance::Destructive,
    }
}

pub fn build_alert(heading: &str, body: &str, choices: &[AlertChoice]) -> adw::AlertDialog {
    let alert_dialog = adw::AlertDialog::new(Some(heading), Some(body));
    choices.iter().for_each(|alert_choice| {
        alert_dialog.add_response(alert_choice.response_id, alert_choice.label);
        alert_dialog.set_response_appearance(alert_choice.response_id, alert_choice.appearance);
    });
    if let Some(first_choice) = choices.first() {
        alert_dialog.set_close_response(first_choice.response_id);
    }
    if let Some(last_choice) = choices.last() {
        alert_dialog.set_default_response(Some(last_choice.response_id));
    }
    alert_dialog
}

pub fn answer_entry(input_purpose: gtk::InputPurpose) -> gtk::Entry {
    gtk::Entry::builder()
        .input_purpose(input_purpose)
        .activates_default(true)
        .build()
}

pub async fn choose_response(alert_dialog: &adw::AlertDialog) -> String {
    let parent_window = relm4::main_application().active_window();
    alert_dialog
        .clone()
        .choose_future(parent_window.as_ref())
        .await
        .to_string()
}
