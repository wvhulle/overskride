use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk::gio;

use crate::widgets::alert_dialogs::{AlertChoice, choose_response};

pub const ANSWER_DIALOG_ACTION: &str = "app.answer-dialog";
pub const SHOW_WINDOW_ACTION: &str = "app.show-window";

static NEXT_DIALOG_IDENTIFIER: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static OPEN_ATTENTION_DIALOGS: RefCell<HashMap<u64, adw::AlertDialog>> =
        RefCell::new(HashMap::new());
}

fn notification_identifier(dialog_identifier: u64) -> String {
    format!("attention-dialog-{dialog_identifier}")
}

fn window_needs_attention() -> bool {
    relm4::main_application()
        .active_window()
        .is_none_or(|active_window| !active_window.is_visible() || !active_window.is_active())
}

fn send_dialog_notification(
    dialog_identifier: u64,
    alert_dialog: &adw::AlertDialog,
    answer_choices: &[AlertChoice],
) {
    let notification = gio::Notification::new(&alert_dialog.heading().unwrap_or_default());
    notification.set_body(alert_dialog.body().as_str().into());
    notification.set_priority(gio::NotificationPriority::High);
    notification.set_default_action(SHOW_WINDOW_ACTION);
    answer_choices.iter().for_each(|answer_choice| {
        notification.add_button_with_target_value(
            answer_choice.label,
            ANSWER_DIALOG_ACTION,
            Some(&(dialog_identifier, answer_choice.response_id).to_variant()),
        );
    });
    relm4::main_application().send_notification(
        Some(&notification_identifier(dialog_identifier)),
        &notification,
    );
}

struct RegisteredAttentionDialog {
    dialog_identifier: u64,
}

impl Drop for RegisteredAttentionDialog {
    fn drop(&mut self) {
        OPEN_ATTENTION_DIALOGS.with_borrow_mut(|open_dialogs| {
            open_dialogs.remove(&self.dialog_identifier);
        });
        relm4::main_application()
            .withdraw_notification(&notification_identifier(self.dialog_identifier));
    }
}

pub async fn choose_with_notification(
    alert_dialog: &adw::AlertDialog,
    answer_choices: &[AlertChoice],
) -> String {
    let dialog_identifier = NEXT_DIALOG_IDENTIFIER.fetch_add(1, Ordering::Relaxed);
    OPEN_ATTENTION_DIALOGS.with_borrow_mut(|open_dialogs| {
        open_dialogs.insert(dialog_identifier, alert_dialog.clone());
    });
    let _registered_dialog = RegisteredAttentionDialog { dialog_identifier };
    if window_needs_attention() {
        send_dialog_notification(dialog_identifier, alert_dialog, answer_choices);
    }
    choose_response(alert_dialog).await
}

pub fn answer_from_notification(dialog_identifier: u64, response_id: &str) {
    let answered_dialog = OPEN_ATTENTION_DIALOGS
        .with_borrow(|open_dialogs| open_dialogs.get(&dialog_identifier).cloned());
    if let Some(answered_dialog) = answered_dialog {
        answered_dialog.emit_by_name::<()>("response", &[&response_id]);
        answered_dialog.force_close();
    }
}
