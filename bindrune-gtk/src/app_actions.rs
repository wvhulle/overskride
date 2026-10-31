use relm4::actions::ActionName;

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
