use relm4::actions::{ActionName, RelmAction, RelmActionGroup};
use relm4::gtk::prelude::*;

use crate::attention_dialogs::answer_from_notification;

relm4::new_action_group!(pub AppActionGroup, "app");
relm4::new_stateless_action!(pub QuitAction, AppActionGroup, "quit");
relm4::new_stateless_action!(pub ShowWindowAction, AppActionGroup, "show-window");

pub struct AnswerDialogAction;

impl ActionName for AnswerDialogAction {
    type Group = AppActionGroup;
    type Target = (u64, String);
    type State = ();

    const NAME: &'static str = "answer-dialog";
}

pub fn present_main_window() {
    if let Some(main_window) = relm4::main_application().windows().first() {
        main_window.present();
    }
}

pub fn register_app_actions() {
    let quit_action: RelmAction<QuitAction> =
        RelmAction::new_stateless(|_quit_action| relm4::main_application().quit());
    let show_window_action: RelmAction<ShowWindowAction> =
        RelmAction::new_stateless(|_show_window_action| present_main_window());
    let answer_dialog_action: RelmAction<AnswerDialogAction> = RelmAction::new_with_target_value(
        |_answer_dialog_action, (dialog_identifier, response_id): (u64, String)| {
            answer_from_notification(dialog_identifier, &response_id);
        },
    );
    let mut app_actions = RelmActionGroup::<AppActionGroup>::new();
    app_actions.add_action(quit_action);
    app_actions.add_action(show_window_action);
    app_actions.add_action(answer_dialog_action);
    app_actions.register_for_main_application();
}
