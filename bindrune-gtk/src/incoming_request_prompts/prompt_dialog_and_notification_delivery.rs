use relm4::{adw, adw::prelude::*, gtk, gtk::gio};

const ANSWER_DIALOG_ACTION: &str = "app.answer-dialog";
const SHOW_WINDOW_ACTION: &str = "app.show-window";

fn notification_identifier(prompt_identifier: u64) -> String {
    format!("prompt-{prompt_identifier}")
}

pub fn window_needs_attention() -> bool {
    relm4::main_application()
        .active_window()
        .is_none_or(|active_window| !active_window.is_visible() || !active_window.is_active())
}

pub fn notify_about_prompt(
    prompt_identifier: u64,
    heading: &str,
    body: &str,
    answer_choices: &[AlertChoice],
) {
    let notification = gio::Notification::new(heading);
    notification.set_body(Some(body));
    notification.set_priority(gio::NotificationPriority::High);
    notification.set_default_action(SHOW_WINDOW_ACTION);
    answer_choices.iter().for_each(|answer_choice| {
        notification.add_button_with_target_value(
            answer_choice.label,
            ANSWER_DIALOG_ACTION,
            Some(&(prompt_identifier, answer_choice.response_id).to_variant()),
        );
    });
    relm4::main_application().send_notification(
        Some(&notification_identifier(prompt_identifier)),
        &notification,
    );
}

pub fn withdraw_prompt_notification(prompt_identifier: u64) {
    relm4::main_application().withdraw_notification(&notification_identifier(prompt_identifier));
}

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
