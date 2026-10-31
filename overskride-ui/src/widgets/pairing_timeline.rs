use adw::prelude::*;
use adw::subclass::prelude::*;
use glib::Object;
use gtk::glib;
use std::cell::RefCell;
use std::rc::Rc;

use crate::pairing_stages::{ConnectionPolicy, PairingStages, PairingStep, RadioBond, PAIRING_STEP_DEFINITIONS};

const CONNECTION_POLICY_CHOICES: [(ConnectionPolicy, &str); 3] = [
    (ConnectionPolicy::Blocked, "Blocked"),
    (ConnectionPolicy::AskEachTime, "Ask Each Time"),
    (ConnectionPolicy::Trusted, "Trusted"),
];

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PairingStepAction {
    Pair,
    PairAgain,
    ForgetDevice,
    SetConnectionPolicy(ConnectionPolicy),
    Connect,
    Disconnect,
    ShowRadio(String),
    PairViaRadio(String),
    AnswerServiceAuthorization(ServiceAuthorizationAnswer),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ServiceAuthorizationAnswer {
    Deny,
    AllowOnce,
    AlwaysAllow,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StepNodeState {
    Done,
    Next,
    Running,
    Warning,
    Failed,
    Pending,
}

type StepActionHandler = Rc<dyn Fn(PairingStepAction)>;

pub struct StepRowWidgets {
    step: PairingStep,
    expander_row: adw::ExpanderRow,
    status_icon: gtk::Image,
    status_spinner: gtk::Spinner,
    action_button: gtk::Button,
    policy_selector: Option<(gtk::DropDown, glib::SignalHandlerId)>,
    property_rows: RefCell<Vec<adw::ActionRow>>,
    trailing_rows: Vec<gtk::Widget>,
}

struct RenderedStep {
    node_state: StepNodeState,
    status_text: String,
    step_action: Option<(PairingStepAction, &'static str)>,
    action_sensitive: bool,
}

const STATUS_STYLE_CLASSES: [&str; 4] = ["success", "accent", "warning", "error"];

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/kaii_lb/Overskride/gtk/pairing-timeline.ui")]
    pub struct PairingTimeline {
        #[template_child]
        pub pairing_group: TemplateChild<adw::PreferencesGroup>,

        pub radio_expander_row: RefCell<Option<adw::ExpanderRow>>,
        pub radio_detail_rows: RefCell<Vec<adw::ActionRow>>,
        pub step_rows: RefCell<Vec<StepRowWidgets>>,
        pub step_actions: RefCell<Vec<(PairingStep, PairingStepAction)>>,
        pub step_action_handler: RefCell<Option<StepActionHandler>>,
        pub service_authorization_row: RefCell<Option<adw::ActionRow>>,

        pub battery_percent: RefCell<Option<i8>>,
        pub last_rendered_stages: RefCell<Option<PairingStages>>,
        pub radio_info_icon: RefCell<Option<gtk::Image>>,
        pub step_progress_bar: RefCell<Option<gtk::LevelBar>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PairingTimeline {
        const NAME: &'static str = "PairingTimeline";
        type Type = super::PairingTimeline;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PairingTimeline {
        fn constructed(&self) {
            self.parent_constructed();
            let timeline = self.obj();
            timeline.build_rows();
            timeline.render(&PairingStages::default());
        }
    }

    impl WidgetImpl for PairingTimeline {}
    impl BinImpl for PairingTimeline {}
}

glib::wrapper! {
    pub struct PairingTimeline(ObjectSubclass<imp::PairingTimeline>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PairingTimeline {
    pub fn new() -> Self {
        Object::builder().build()
    }

    pub fn connect_step_action<F: Fn(PairingStepAction) + 'static>(&self, step_action_handler: F) {
        *self.imp().step_action_handler.borrow_mut() = Some(Rc::new(step_action_handler));
    }

    fn emit_step_action(&self, step_action: PairingStepAction) {
        let step_action_handler = self.imp().step_action_handler.borrow().clone();
        if let Some(step_action_handler) = step_action_handler {
            step_action_handler(step_action);
        }
    }

    fn build_rows(&self) {
        let pairing_group = self.imp().pairing_group.get();

        let step_progress_bar = gtk::LevelBar::builder()
            .mode(gtk::LevelBarMode::Discrete)
            .min_value(0.0)
            .width_request(120)
            .valign(gtk::Align::Center)
            .build();
        for default_offset_name in ["low", "high", "full"] {
            step_progress_bar.remove_offset_value(Some(default_offset_name));
        }
        pairing_group.set_header_suffix(Some(&step_progress_bar));
        *self.imp().step_progress_bar.borrow_mut() = Some(step_progress_bar);

        let radio_expander_row = adw::ExpanderRow::builder().title("Radio").build();
        radio_expander_row.add_prefix(&gtk::Image::from_icon_name("bluetooth-symbolic"));
        pairing_group.add(&radio_expander_row);
        *self.imp().radio_expander_row.borrow_mut() = Some(radio_expander_row);

        let mut step_rows = Vec::new();
        for definition in PAIRING_STEP_DEFINITIONS.iter() {
            let step = definition.step;
            let expander_row = adw::ExpanderRow::builder().title(definition.title).build();

            let status_icon = gtk::Image::from_icon_name("content-loading-symbolic");
            let status_spinner = gtk::Spinner::builder().visible(false).build();
            expander_row.add_prefix(&status_icon);
            expander_row.add_prefix(&status_spinner);

            expander_row.add_suffix(&build_protocol_info_button(definition.protocol_term, definition.protocol_details));

            let action_button = gtk::Button::builder().valign(gtk::Align::Center).visible(false).build();
            let weak_timeline = self.downgrade();
            action_button.connect_clicked(move |_| {
                if let Some(timeline) = weak_timeline.upgrade() {
                    timeline.run_step_action(step);
                }
            });
            expander_row.add_suffix(&action_button);

            let policy_selector = (step == PairingStep::Authorization).then(|| {
                let policy_labels: Vec<&str> = CONNECTION_POLICY_CHOICES.iter().map(|(_, policy_label)| *policy_label).collect();
                let policy_dropdown = gtk::DropDown::from_strings(&policy_labels);
                policy_dropdown.set_valign(gtk::Align::Center);
                let weak_timeline = self.downgrade();
                let policy_handler = policy_dropdown.connect_selected_notify(move |changed_dropdown| {
                    let chosen_policy = CONNECTION_POLICY_CHOICES
                        .get(changed_dropdown.selected() as usize)
                        .map(|(policy, _)| *policy);
                    if let (Some(timeline), Some(chosen_policy)) = (weak_timeline.upgrade(), chosen_policy) {
                        timeline.emit_step_action(PairingStepAction::SetConnectionPolicy(chosen_policy));
                    }
                });
                expander_row.add_suffix(&policy_dropdown);
                (policy_dropdown, policy_handler)
            });

            let mut trailing_rows: Vec<gtk::Widget> = Vec::new();

            if step == PairingStep::Bonding {
                let forget_device_row = adw::ActionRow::builder()
                    .title("Forget Device")
                    .subtitle("Deletes the link key and trust stored for this radio. The device has to pair again before it can connect.")
                    .subtitle_lines(0)
                    .activatable(false)
                    .build();
                let forget_device_button = gtk::Button::builder().label("Forget").valign(gtk::Align::Center).build();
                forget_device_button.add_css_class("destructive-action");
                let weak_timeline = self.downgrade();
                forget_device_button.connect_clicked(move |_| {
                    if let Some(timeline) = weak_timeline.upgrade() {
                        timeline.emit_step_action(PairingStepAction::ForgetDevice);
                    }
                });
                forget_device_row.add_suffix(&forget_device_button);
                expander_row.add_row(&forget_device_row);
                trailing_rows.push(forget_device_row.upcast());
            }

            pairing_group.add(&expander_row);
            step_rows.push(StepRowWidgets {
                step,
                expander_row,
                status_icon,
                status_spinner,
                action_button,
                policy_selector,
                property_rows: RefCell::new(Vec::new()),
                trailing_rows,
            });
        }
        *self.imp().step_rows.borrow_mut() = step_rows;
    }

    fn authorization_step_row(&self) -> Option<adw::ExpanderRow> {
        self.imp()
            .step_rows
            .borrow()
            .iter()
            .find(|step_row| step_row.step == PairingStep::Authorization)
            .map(|step_row| step_row.expander_row.clone())
    }

    pub fn show_service_authorization_request(&self, service_name: &str, radio_label: &str) {
        self.clear_service_authorization_request();
        let Some(authorization_expander_row) = self.authorization_step_row() else {
            return;
        };

        let service_authorization_row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&format!("Wants to connect: {}", service_name)).as_str())
            .subtitle(
                glib::markup_escape_text(&format!("Via {}. Answer within 25 seconds, or the request is denied.", radio_label)).as_str(),
            )
            .subtitle_lines(0)
            .activatable(false)
            .build();
        service_authorization_row.add_css_class("warning");

        let answer_buttons_box = gtk::Box::builder().spacing(6).valign(gtk::Align::Center).build();
        for (answer_label, authorization_answer) in [
            ("Deny", ServiceAuthorizationAnswer::Deny),
            ("Allow Once", ServiceAuthorizationAnswer::AllowOnce),
            ("Always Allow", ServiceAuthorizationAnswer::AlwaysAllow),
        ] {
            let answer_button = gtk::Button::with_label(answer_label);
            if authorization_answer == ServiceAuthorizationAnswer::AlwaysAllow {
                answer_button.add_css_class("suggested-action");
            }
            let weak_timeline = self.downgrade();
            answer_button.connect_clicked(move |_| {
                if let Some(timeline) = weak_timeline.upgrade() {
                    timeline.clear_service_authorization_request();
                    timeline.emit_step_action(PairingStepAction::AnswerServiceAuthorization(authorization_answer));
                }
            });
            answer_buttons_box.append(&answer_button);
        }
        service_authorization_row.add_suffix(&answer_buttons_box);

        authorization_expander_row.add_row(&service_authorization_row);
        authorization_expander_row.set_expanded(true);
        *self.imp().service_authorization_row.borrow_mut() = Some(service_authorization_row);
    }

    pub fn clear_service_authorization_request(&self) {
        let pending_row = self.imp().service_authorization_row.borrow_mut().take();
        if let (Some(pending_row), Some(authorization_expander_row)) = (pending_row, self.authorization_step_row()) {
            authorization_expander_row.remove(&pending_row);
        }
    }

    fn run_step_action(&self, step: PairingStep) {
        let chosen_action = self
            .imp()
            .step_actions
            .borrow()
            .iter()
            .find(|(action_step, _)| *action_step == step)
            .map(|(_, step_action)| step_action.clone());

        if let Some(chosen_action) = chosen_action {
            self.emit_step_action(chosen_action);
        }
    }

    pub fn set_battery_level(&self, battery_percent: i8) {
        let known_battery_percent = (battery_percent >= 0).then_some(battery_percent);
        *self.imp().battery_percent.borrow_mut() = known_battery_percent;

        let last_rendered_stages = self.imp().last_rendered_stages.borrow().clone();
        if let Some(last_rendered_stages) = last_rendered_stages {
            self.render(&last_rendered_stages);
        }
    }

    pub fn render(&self, stages: &PairingStages) {
        *self.imp().last_rendered_stages.borrow_mut() = Some(stages.clone());
        let battery_percent = *self.imp().battery_percent.borrow();
        self.imp()
            .pairing_group
            .get()
            .set_description(Some(&summarize_pairing_stages(stages)));
        self.render_radio_row(stages);

        let first_missing_step = stages.first_missing_step();
        let is_busy = stages.running_step.is_some();
        let mut step_actions = Vec::new();
        self.render_step_progress(stages, first_missing_step);

        for step_row in self.imp().step_rows.borrow().iter() {
            step_row.expander_row.set_visible(stages.is_step_applicable(step_row.step));
            let rendered_step = render_step(step_row.step, stages, first_missing_step);

            if let Some((policy_dropdown, policy_handler)) = &step_row.policy_selector {
                let policy_position = CONNECTION_POLICY_CHOICES
                    .iter()
                    .position(|(policy, _)| *policy == stages.connection_policy())
                    .unwrap_or(1);
                policy_dropdown.block_signal(policy_handler);
                policy_dropdown.set_selected(policy_position as u32);
                policy_dropdown.unblock_signal(policy_handler);
                policy_dropdown.set_sensitive(stages.paired && !is_busy);
            }

            step_row
                .expander_row
                .set_subtitle(&glib::markup_escape_text(&rendered_step.status_text));
            replace_property_rows(step_row, &step_properties(step_row.step, stages, battery_percent));
            apply_node_state(step_row, rendered_step.node_state);

            let missing_prerequisite = missing_prerequisite_step(step_row.step, stages);
            step_row.expander_row.set_sensitive(missing_prerequisite.is_none());
            if let Some(missing_prerequisite) = missing_prerequisite {
                step_row
                    .expander_row
                    .set_subtitle(&format!("Needs {}", step_title(missing_prerequisite).to_lowercase()));
            }

            match rendered_step.step_action {
                Some((step_action, button_label)) => {
                    step_actions.push((step_row.step, step_action));
                    let action_button = &step_row.action_button;
                    action_button.set_label(button_label);
                    action_button.set_sensitive(rendered_step.action_sensitive && !is_busy);
                    if first_missing_step == Some(step_row.step) || rendered_step.node_state == StepNodeState::Warning {
                        action_button.add_css_class("suggested-action");
                    } else {
                        action_button.remove_css_class("suggested-action");
                    }
                    action_button.set_visible(true);
                }
                None => step_row.action_button.set_visible(false),
            }
        }

        *self.imp().step_actions.borrow_mut() = step_actions;
    }

    fn render_step_progress(&self, stages: &PairingStages, first_missing_step: Option<PairingStep>) {
        let Some(step_progress_bar) = self.imp().step_progress_bar.borrow().clone() else {
            return;
        };
        let applicable_steps: Vec<PairingStep> = stages.applicable_steps().collect();
        let completed_in_order = first_missing_step
            .and_then(|missing_step| applicable_steps.iter().position(|step| *step == missing_step))
            .unwrap_or(applicable_steps.len());

        step_progress_bar.set_max_value(applicable_steps.len().max(1) as f64);
        step_progress_bar.set_value(completed_in_order as f64);
        step_progress_bar.set_tooltip_text(Some(&format!("{} of {} steps done", completed_in_order, applicable_steps.len())));
    }

    fn render_radio_row(&self, stages: &PairingStages) {
        let Some(radio_expander_row) = self.imp().radio_expander_row.borrow().clone() else {
            return;
        };

        for radio_detail_row in self.imp().radio_detail_rows.borrow_mut().drain(..) {
            radio_expander_row.remove(&radio_detail_row);
        }

        radio_expander_row.set_visible(stages.radio_bonds.len() > 1);

        let shown_radio_label = stages
            .shown_radio()
            .map(|radio_bond| radio_bond.radio_label.clone())
            .unwrap_or_default();
        radio_expander_row.set_subtitle(&glib::markup_escape_text(&shown_radio_label));

        if let Some(previous_radio_info_icon) = self.imp().radio_info_icon.borrow_mut().take() {
            radio_expander_row.remove(&previous_radio_info_icon);
        }
        let other_bonded_labels: Vec<String> = stages
            .other_bonded_radios()
            .map(|radio_bond| radio_bond.radio_label.clone())
            .collect();
        if !other_bonded_labels.is_empty() {
            let radio_info_icon = gtk::Image::from_icon_name("dialog-information-symbolic");
            radio_info_icon.set_tooltip_text(Some(&format!("Also bonded via {}", other_bonded_labels.join(", "))));
            radio_expander_row.add_suffix(&radio_info_icon);
            *self.imp().radio_info_icon.borrow_mut() = Some(radio_info_icon);
        }

        let mut radio_detail_rows = Vec::new();
        for radio_bond in stages.radio_bonds.iter() {
            let radio_detail_row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&radio_bond.radio_label).as_str())
                .subtitle(glib::markup_escape_text(&describe_radio_bond(radio_bond)).as_str())
                .subtitle_lines(0)
                .activatable(false)
                .build();

            if radio_bond_may_be_stale(radio_bond, stages) {
                let stale_bond_icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
                stale_bond_icon.add_css_class("warning");
                stale_bond_icon.set_tooltip_text(Some("May be stale: the device is connected through another radio"));
                radio_detail_row.add_suffix(&stale_bond_icon);
            }

            if radio_bond.shown {
                let shown_label = gtk::Label::new(Some("Shown"));
                shown_label.add_css_class("dim-label");
                radio_detail_row.add_suffix(&shown_label);
            } else {
                let (button_label, step_action) = if radio_bond.known {
                    ("Show", PairingStepAction::ShowRadio(radio_bond.adapter_name.clone()))
                } else {
                    (
                        "Pair via This Radio",
                        PairingStepAction::PairViaRadio(radio_bond.adapter_name.clone()),
                    )
                };
                let radio_button = gtk::Button::builder().label(button_label).valign(gtk::Align::Center).build();
                let weak_timeline = self.downgrade();
                let switching_description = format!("Switching to {}…", radio_bond.radio_label);
                radio_button.connect_clicked(move |clicked_button| {
                    let switching_spinner = gtk::Spinner::builder().spinning(true).build();
                    clicked_button.set_child(Some(&switching_spinner));
                    clicked_button.set_sensitive(false);

                    if let Some(timeline) = weak_timeline.upgrade() {
                        timeline
                            .imp()
                            .pairing_group
                            .get()
                            .set_description(Some(&glib::markup_escape_text(&switching_description)));
                        let step_action = step_action.clone();
                        glib::idle_add_local_once(move || timeline.emit_step_action(step_action));
                    }
                });
                radio_detail_row.add_suffix(&radio_button);
            }

            radio_expander_row.add_row(&radio_detail_row);
            radio_detail_rows.push(radio_detail_row);
        }
        *self.imp().radio_detail_rows.borrow_mut() = radio_detail_rows;
    }
}

impl Default for PairingTimeline {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_node_state(step_row: &StepRowWidgets, node_state: StepNodeState) {
    let status_icon = &step_row.status_icon;
    for style_class in STATUS_STYLE_CLASSES {
        status_icon.remove_css_class(style_class);
    }
    status_icon.remove_css_class("dim-label");

    let (icon_name, style_class) = match node_state {
        StepNodeState::Done => ("object-select-symbolic", "success"),
        StepNodeState::Next | StepNodeState::Running => ("go-next-symbolic", "accent"),
        StepNodeState::Warning => ("dialog-warning-symbolic", "warning"),
        StepNodeState::Failed => ("dialog-error-symbolic", "error"),
        StepNodeState::Pending => ("content-loading-symbolic", "dim-label"),
    };
    status_icon.set_icon_name(Some(icon_name));
    status_icon.add_css_class(style_class);

    let is_running = node_state == StepNodeState::Running;
    status_icon.set_visible(!is_running);
    step_row.status_spinner.set_visible(is_running);
    step_row.status_spinner.set_spinning(is_running);
}

fn describe_radio_bond(radio_bond: &RadioBond) -> String {
    let radio_state = if !radio_bond.known {
        "Doesn't know this device"
    } else if radio_bond.connected {
        "Connected"
    } else if !radio_bond.bonded {
        "Seen, not bonded"
    } else if radio_bond.trusted {
        "Bonded, trusted"
    } else {
        "Bonded, not trusted"
    };
    radio_state.to_string()
}

fn radio_bond_may_be_stale(radio_bond: &RadioBond, stages: &PairingStages) -> bool {
    radio_bond.bonded
        && !radio_bond.connected
        && stages
            .radio_bonds
            .iter()
            .any(|other_bond| other_bond.connected && other_bond.adapter_name != radio_bond.adapter_name)
}

fn missing_prerequisite_step(step: PairingStep, stages: &PairingStages) -> Option<PairingStep> {
    let prerequisite_step = match step {
        PairingStep::Pairing => return None,
        PairingStep::Bonding => PairingStep::Pairing,
        PairingStep::Authorization => PairingStep::Bonding,
        PairingStep::Connection => PairingStep::Pairing,
        PairingStep::InputDevice => PairingStep::Connection,
    };
    let step_is_active =
        stages.running_step == Some(step) || stages.failed_step.as_ref().is_some_and(|(failed_step, _)| *failed_step == step);
    let blocked_by_prerequisite = !stages.is_step_done(prerequisite_step) && !stages.is_step_done(step) && !step_is_active;
    blocked_by_prerequisite.then_some(prerequisite_step)
}

fn summarize_pairing_stages(stages: &PairingStages) -> String {
    if let Some(running_step) = stages.running_step {
        return format!("Working on {}…", step_title(running_step).to_lowercase());
    }
    if let Some((failed_step, reason)) = &stages.failed_step {
        return format!("{} failed: {}", step_title(*failed_step), reason);
    }
    let summary_sentence = if !stages.paired {
        "Not paired yet."
    } else if !stages.bonded {
        "Paired, but no link key was stored, so trust can't be saved."
    } else if stages.blocked {
        "Blocked. Every connection is rejected."
    } else if !stages.trusted {
        "Bonded. Each connection needs approval."
    } else if !stages.connected {
        "Bonded and trusted. Reconnects when turned on."
    } else if stages.is_input_device && stages.kernel_input_devices.is_none() {
        "Connected, but no input device appeared."
    } else {
        "Connected and ready."
    };
    summary_sentence.to_string()
}

fn step_title(step: PairingStep) -> &'static str {
    PAIRING_STEP_DEFINITIONS
        .iter()
        .find(|definition| definition.step == step)
        .map(|definition| definition.title)
        .unwrap_or("Step")
}

fn render_step(step: PairingStep, stages: &PairingStages, first_missing_step: Option<PairingStep>) -> RenderedStep {
    let default_node_state = if stages.is_step_done(step) {
        StepNodeState::Done
    } else if first_missing_step == Some(step) {
        StepNodeState::Next
    } else {
        StepNodeState::Pending
    };

    let mut rendered_step = match step {
        PairingStep::Pairing => RenderedStep {
            node_state: default_node_state,
            status_text: if stages.paired {
                "Link key generated".to_string()
            } else if stages.is_gamepad {
                "Hold PS + Create until the light flashes, then press Pair".to_string()
            } else {
                "Put the device in pairing mode, then press Pair".to_string()
            },
            step_action: (!stages.paired).then_some((PairingStepAction::Pair, "Pair")),
            action_sensitive: true,
        },
        PairingStep::Bonding => {
            let bond_missing = stages.paired && !stages.bonded;
            RenderedStep {
                node_state: if bond_missing { StepNodeState::Warning } else { default_node_state },
                status_text: if stages.bonded {
                    "Link key stored".to_string()
                } else if bond_missing {
                    "No link key stored".to_string()
                } else {
                    "Happens together with pairing".to_string()
                },
                step_action: stages.paired.then_some((PairingStepAction::PairAgain, "Pair Again")),
                action_sensitive: true,
            }
        }
        PairingStep::Authorization => RenderedStep {
            node_state: if stages.blocked {
                StepNodeState::Warning
            } else {
                default_node_state
            },
            status_text: match (stages.connection_policy(), stages.bonded) {
                (ConnectionPolicy::Blocked, _) => "Blocked".to_string(),
                (ConnectionPolicy::Trusted, true) => "Trusted".to_string(),
                (ConnectionPolicy::Trusted, false) => "Trusted until it disconnects (no bond)".to_string(),
                (ConnectionPolicy::AskEachTime, true) => "Asks before each connection".to_string(),
                (ConnectionPolicy::AskEachTime, false) => "Needs a bond before trust can be saved".to_string(),
            },
            step_action: None,
            action_sensitive: false,
        },
        PairingStep::InputDevice => render_input_device_step(stages, default_node_state),
        PairingStep::Connection => {
            let waiting_for_device = !stages.connected && stages.bonded && stages.trusted;
            RenderedStep {
                node_state: if waiting_for_device {
                    StepNodeState::Next
                } else {
                    default_node_state
                },
                status_text: if stages.connected {
                    "Connected".to_string()
                } else if waiting_for_device {
                    "Waiting for the device".to_string()
                } else {
                    "Not connected".to_string()
                },
                step_action: Some(if stages.connected {
                    (PairingStepAction::Disconnect, "Disconnect")
                } else {
                    (PairingStepAction::Connect, "Connect")
                }),
                action_sensitive: stages.paired,
            }
        }
    };

    if stages.running_step == Some(step) {
        rendered_step.node_state = StepNodeState::Running;
    }
    if let Some((failed_step, reason)) = &stages.failed_step {
        if *failed_step == step {
            rendered_step.node_state = StepNodeState::Failed;
            rendered_step.status_text = reason.clone();
        }
    }

    rendered_step
}

fn render_input_device_step(stages: &PairingStages, default_node_state: StepNodeState) -> RenderedStep {
    let status_text = match &stages.kernel_input_devices {
        Some(kernel_input_devices) => {
            let input_count = kernel_input_devices.input_nodes.len();
            format!("{} input device{}", input_count, if input_count == 1 { "" } else { "s" })
        }
        None if stages.connected => "No input device".to_string(),
        None => "Not connected".to_string(),
    };

    let input_missing_while_connected = stages.connected && stages.kernel_input_devices.is_none();
    RenderedStep {
        node_state: if input_missing_while_connected {
            StepNodeState::Warning
        } else {
            default_node_state
        },
        status_text,
        step_action: None,
        action_sensitive: false,
    }
}

fn build_protocol_info_button(protocol_term: &str, protocol_details: &str) -> gtk::MenuButton {
    let popover_content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    let term_label = gtk::Label::builder().label(protocol_term).xalign(0.0).build();
    term_label.add_css_class("heading");
    let details_label = gtk::Label::builder()
        .label(protocol_details)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(42)
        .build();
    popover_content.append(&term_label);
    popover_content.append(&details_label);

    let info_button = gtk::MenuButton::builder()
        .icon_name("help-about-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(protocol_term)
        .popover(&gtk::Popover::builder().child(&popover_content).build())
        .build();
    info_button.add_css_class("flat");
    info_button
}

fn replace_property_rows(step_row: &StepRowWidgets, step_properties: &[StepProperty]) {
    for previous_property_row in step_row.property_rows.borrow_mut().drain(..) {
        step_row.expander_row.remove(&previous_property_row);
    }
    for trailing_row in step_row.trailing_rows.iter() {
        step_row.expander_row.remove(trailing_row);
    }

    let mut property_rows = Vec::new();
    for step_property in step_properties {
        let property_row = match step_property.value_layout {
            ValueLayout::Inline => build_inline_value_row(step_property),
            ValueLayout::Stacked => build_stacked_value_row(step_property),
        };
        step_row.expander_row.add_row(&property_row);
        property_rows.push(property_row);
    }
    for trailing_row in step_row.trailing_rows.iter() {
        step_row.expander_row.add_row(trailing_row);
    }
    *step_row.property_rows.borrow_mut() = property_rows;
}

fn build_inline_value_row(step_property: &StepProperty) -> adw::ActionRow {
    let value_row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&step_property.key).as_str())
        .activatable(false)
        .build();
    if let Some(hint_text) = &step_property.hint {
        value_row.set_subtitle(&glib::markup_escape_text(hint_text));
    }

    let value_box = gtk::Box::builder().spacing(6).valign(gtk::Align::Center).build();
    if let Some(value_icon_name) = &step_property.value_icon_name {
        value_box.append(&gtk::Image::from_icon_name(value_icon_name));
    }
    value_box.append(&gtk::Label::builder().label(step_property.value.as_str()).selectable(true).build());
    value_row.add_suffix(&value_box);
    value_row
}

fn build_stacked_value_row(step_property: &StepProperty) -> adw::ActionRow {
    let value_row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&step_property.key).as_str())
        .subtitle(glib::markup_escape_text(&step_property.value).as_str())
        .subtitle_selectable(true)
        .activatable(false)
        .build();
    value_row.add_css_class("property");
    if let Some(hint_text) = &step_property.hint {
        value_row.set_tooltip_text(Some(hint_text));
    }
    value_row
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ValueLayout {
    Inline,
    Stacked,
}

pub struct StepProperty {
    key: String,
    value: String,
    hint: Option<String>,
    value_layout: ValueLayout,
    value_icon_name: Option<String>,
}

impl StepProperty {
    fn stacked(mut self) -> Self {
        self.value_layout = ValueLayout::Stacked;
        self
    }

    fn with_value_icon(mut self, value_icon_name: String) -> Self {
        self.value_icon_name = Some(value_icon_name);
        self
    }
}

fn step_property(property_key: &str, property_value: impl Into<String>) -> StepProperty {
    StepProperty {
        key: property_key.to_string(),
        value: property_value.into(),
        hint: None,
        value_layout: ValueLayout::Inline,
        value_icon_name: None,
    }
}

fn hinted_step_property(property_key: &str, property_value: impl Into<String>, hint_text: &str) -> StepProperty {
    StepProperty {
        key: property_key.to_string(),
        value: property_value.into(),
        hint: Some(hint_text.to_string()),
        value_layout: ValueLayout::Inline,
        value_icon_name: None,
    }
}

fn step_properties(step: PairingStep, stages: &PairingStages, battery_percent: Option<i8>) -> Vec<StepProperty> {
    let shown_radio = stages.shown_radio();
    let radio_label = shown_radio.map(|radio_bond| radio_bond.radio_label.clone()).unwrap_or_default();

    match step {
        PairingStep::Pairing => {
            let mut pairing_properties = vec![
                if stages.legacy_pairing {
                    hinted_step_property("Method", "Legacy PIN pairing", "Older, weaker method")
                } else {
                    step_property("Method", "Secure Simple Pairing")
                },
                hinted_step_property(
                    "Address",
                    stages.device_address.clone(),
                    &format!("{} address", capitalize_address_type(&stages.address_type)),
                ),
            ];
            if let Some(device_class) = stages.device_class {
                pairing_properties.push(step_property("Device class", format!("0x{:06x}", device_class)));
            }
            pairing_properties
        }
        PairingStep::Bonding => {
            let mut bonding_properties = vec![if stages.bonded {
                hinted_step_property("Link key", "Stored", "Survives power-off and reboot")
            } else {
                hinted_step_property("Link key", "Not stored", "Pair again with the device in pairing mode")
            }];
            if let Some(shown_radio) = shown_radio.filter(|_| stages.bonded) {
                bonding_properties.push(
                    step_property(
                        "Stored in",
                        format!("/var/lib/bluetooth/{}/{}/info", shown_radio.adapter_address, stages.device_address),
                    )
                    .stacked(),
                );
            }
            if !radio_label.is_empty() {
                bonding_properties.push(hinted_step_property("Radio", radio_label, "Each radio keeps its own bond").stacked());
            }
            bonding_properties
        }
        PairingStep::Authorization => vec![
            match stages.connection_policy() {
                ConnectionPolicy::Blocked => hinted_step_property("Policy", "Blocked", "Every connection is rejected"),
                ConnectionPolicy::AskEachTime => {
                    hinted_step_property("Policy", "Ask each time", "The pairing agent approves each connection")
                }
                ConnectionPolicy::Trusted => hinted_step_property("Policy", "Trusted", "Connects without asking"),
            },
            if stages.bonded {
                step_property("Saved", "Yes")
            } else {
                hinted_step_property("Saved", "No", "Only kept while connected, because there is no bond")
            },
        ],
        PairingStep::Connection => {
            let waiting_for_device = !stages.connected && stages.bonded && stages.trusted;
            let mut connection_properties = vec![if stages.connected {
                step_property("State", "Connected")
            } else if waiting_for_device {
                hinted_step_property("State", "Waiting", "Turn the device on; it connects by itself")
            } else {
                step_property("State", "Not connected")
            }];
            if !radio_label.is_empty() {
                connection_properties.push(step_property("Radio", radio_label).stacked());
            }
            if let Some(signal_strength) = stages.signal_strength {
                connection_properties.push(hinted_step_property(
                    "Signal strength",
                    format!("{} dBm", signal_strength),
                    "Only measured while scanning",
                ));
            }
            match battery_percent {
                Some(battery_percent) if stages.connected => {
                    let battery_source = if stages
                        .kernel_input_devices
                        .as_ref()
                        .is_some_and(|kernel_input_devices| kernel_input_devices.battery_percent.is_some())
                    {
                        "Reported by the input driver"
                    } else {
                        "Reported by BlueZ"
                    };
                    let rounded_battery_level = (i32::from(battery_percent.clamp(0, 100)) + 5) / 10 * 10;
                    connection_properties.push(
                        hinted_step_property("Battery", format!("{}%", battery_percent), battery_source)
                            .with_value_icon(format!("battery-level-{}-symbolic", rounded_battery_level)),
                    );
                }
                None if stages.connected => connection_properties.push(step_property("Battery", "Not reported")),
                _ => {}
            }
            connection_properties
        }
        PairingStep::InputDevice => match &stages.kernel_input_devices {
            Some(kernel_input_devices) => {
                let mut input_properties = vec![hinted_step_property(
                    "Driver",
                    kernel_input_devices.driver_name.clone(),
                    "Linux HID driver",
                )];
                let main_input_name = kernel_input_devices
                    .input_nodes
                    .first()
                    .map(|input_node| input_node.input_name.clone())
                    .unwrap_or_default();
                for input_node in kernel_input_devices.input_nodes.iter() {
                    let input_label = input_node
                        .input_name
                        .strip_prefix(main_input_name.as_str())
                        .map(str::trim)
                        .filter(|name_suffix| !name_suffix.is_empty())
                        .unwrap_or("Main");
                    let device_paths = input_node
                        .device_nodes
                        .iter()
                        .map(|device_node| format!("/dev/input/{}", device_node))
                        .collect::<Vec<String>>()
                        .join(", ");
                    input_properties.push(step_property(input_label, device_paths).stacked());
                }

                input_properties
            }
            None if stages.connected => vec![hinted_step_property("Input devices", "None", "Check the kernel log (dmesg)")],
            None => vec![hinted_step_property(
                "Input devices",
                "None yet",
                "Created automatically once connected",
            )],
        },
    }
}

fn capitalize_address_type(address_type: &str) -> String {
    let mut address_type_characters = address_type.chars();
    match address_type_characters.next() {
        Some(first_character) => first_character
            .to_uppercase()
            .chain(address_type_characters.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}
