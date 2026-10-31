use relm4::gtk::gio;
use relm4::gtk::prelude::*;

use crate::widgets::alert_dialogs::AlertChoice;

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
