use bindrune_bluez::device_pairing::pairing_step_progress::ConnectionPolicy;
use relm4::{
    adw,
    adw::prelude::*,
    factory::{DynamicIndex, FactoryComponent, FactorySender, FactoryView},
    gtk,
    gtk::glib,
};

use crate::{
    device_detail::pairing_step_timeline::pairing_timeline_view_model::{
        PolicySelector, PropertyHint, PropertyRow, StepButtonSlot, StepNodeState, StepPresentation,
        TimelineAction,
    },
    shared_page_widget_templates::protocol_info_button,
};

struct PolicyChoice {
    policy: ConnectionPolicy,
    label: &'static str,
}

const POLICY_CHOICES: [PolicyChoice; 3] = [
    PolicyChoice {
        policy: ConnectionPolicy::Blocked,
        label: "Blocked",
    },
    PolicyChoice {
        policy: ConnectionPolicy::AskEachTime,
        label: "Ask Each Time",
    },
    PolicyChoice {
        policy: ConnectionPolicy::Trusted,
        label: "Trusted",
    },
];

const NODE_STYLE_CLASSES: [&str; 5] = ["success", "accent", "warning", "error", "dim-label"];

#[derive(Debug)]
pub struct PairingStepRow {
    presentation: StepPresentation,
    previously_needed_attention: bool,
    presentation_changed: bool,
}

#[derive(Debug)]
pub enum PairingStepRowInput {
    Present(StepPresentation),
    ActionButtonClicked,
    PolicyChosen(u32),
    ForgetClicked,
}

pub struct PairingStepRowWidgets {
    status_icon: gtk::Image,
    status_spinner: gtk::Spinner,
    action_button: gtk::Button,
    policy_dropdown: gtk::DropDown,
    policy_handler: glib::SignalHandlerId,
    property_rows: Vec<adw::PreferencesRow>,
}

fn node_icon(node_state: StepNodeState) -> (&'static str, &'static str) {
    match node_state {
        StepNodeState::Done => ("object-select-symbolic", "success"),
        StepNodeState::Next | StepNodeState::Running => ("media-record-symbolic", "accent"),
        StepNodeState::Warning => ("dialog-warning-symbolic", "warning"),
        StepNodeState::Failed => ("dialog-error-symbolic", "error"),
        StepNodeState::Pending => ("content-loading-symbolic", "dim-label"),
    }
}

fn apply_hint(property_row: &adw::ActionRow, hint: &PropertyHint) {
    if let PropertyHint::Shown(hint_text) = hint {
        property_row.set_subtitle(&glib::markup_escape_text(hint_text));
    }
}

fn inline_value_row(
    key: &str,
    value: &str,
    hint: &PropertyHint,
    icon_name: &str,
) -> adw::ActionRow {
    let value_row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(key).as_str())
        .activatable(false)
        .build();
    apply_hint(&value_row, hint);
    let value_box = gtk::Box::builder()
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    if !icon_name.is_empty() {
        value_box.append(&gtk::Image::from_icon_name(icon_name));
    }
    value_box.append(&gtk::Label::builder().label(value).selectable(true).build());
    value_row.add_suffix(&value_box);
    value_row
}

fn stacked_value_row(key: &str, value: &str, hint: &PropertyHint) -> adw::ActionRow {
    let value_row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(key).as_str())
        .subtitle(glib::markup_escape_text(value).as_str())
        .subtitle_selectable(true)
        .activatable(false)
        .css_classes(["property"])
        .build();
    if let PropertyHint::Shown(hint_text) = hint {
        value_row.set_tooltip_text(Some(hint_text));
    }
    value_row
}

fn forget_device_row(sender: &FactorySender<PairingStepRow>) -> adw::ActionRow {
    let forget_row = adw::ActionRow::builder()
        .title("Forget Device")
        .subtitle(
            "Deletes the link key and trust stored for this radio. The device has to pair again \
             before it can connect.",
        )
        .subtitle_lines(0)
        .activatable(false)
        .build();
    let forget_button = gtk::Button::builder()
        .label("Forget")
        .valign(gtk::Align::Center)
        .css_classes(["destructive-action"])
        .build();
    let forget_sender = sender.clone();
    forget_button.connect_clicked(move |_clicked_button| {
        forget_sender.input(PairingStepRowInput::ForgetClicked);
    });
    forget_row.add_suffix(&forget_button);
    forget_row
}

fn build_property_row(property_row: &PropertyRow) -> adw::PreferencesRow {
    let action_row = match property_row {
        PropertyRow::Inline { key, value, hint } => inline_value_row(key, value, hint, ""),
        PropertyRow::InlineWithIcon {
            key,
            value,
            hint,
            icon_name,
        } => inline_value_row(key, value, hint, icon_name),
        PropertyRow::Stacked { key, value, hint } => stacked_value_row(key, value, hint),
    };
    action_row.upcast()
}

impl PairingStepRow {
    fn render_step(
        &self,
        expander_row: &adw::ExpanderRow,
        step_widgets: &mut PairingStepRowWidgets,
        sender: &FactorySender<Self>,
    ) {
        let step_presentation = &self.presentation;
        if step_presentation.needs_attention && !self.previously_needed_attention {
            expander_row.set_expanded(true);
        }
        expander_row.set_visible(step_presentation.visible);
        expander_row.set_sensitive(step_presentation.sensitive);
        expander_row.set_subtitle(&glib::markup_escape_text(&step_presentation.status_text));

        let (icon_name, style_class) = node_icon(step_presentation.node_state);
        let is_running = step_presentation.node_state == StepNodeState::Running;
        NODE_STYLE_CLASSES.iter().for_each(|node_style_class| {
            step_widgets.status_icon.remove_css_class(node_style_class)
        });
        step_widgets.status_icon.set_icon_name(Some(icon_name));
        step_widgets.status_icon.add_css_class(style_class);
        step_widgets.status_icon.set_visible(!is_running);
        step_widgets.status_spinner.set_visible(is_running);
        step_widgets.status_spinner.set_spinning(is_running);

        match &step_presentation.button {
            StepButtonSlot::Hidden => step_widgets.action_button.set_visible(false),
            StepButtonSlot::Shown(step_button) => {
                step_widgets.action_button.set_label(step_button.label);
                step_widgets
                    .action_button
                    .set_sensitive(step_button.sensitive);
                if step_button.suggested {
                    step_widgets.action_button.add_css_class("suggested-action");
                } else {
                    step_widgets
                        .action_button
                        .remove_css_class("suggested-action");
                }
                step_widgets.action_button.set_visible(true);
            }
        }

        match step_presentation.policy_selector {
            PolicySelector::Absent => step_widgets.policy_dropdown.set_visible(false),
            PolicySelector::Present {
                selected,
                sensitive,
            } => {
                let selected_position = POLICY_CHOICES
                    .iter()
                    .position(|policy_choice| policy_choice.policy == selected)
                    .unwrap_or_default();
                step_widgets
                    .policy_dropdown
                    .block_signal(&step_widgets.policy_handler);
                step_widgets
                    .policy_dropdown
                    .set_selected(u32::try_from(selected_position).unwrap_or_default());
                step_widgets
                    .policy_dropdown
                    .unblock_signal(&step_widgets.policy_handler);
                step_widgets.policy_dropdown.set_sensitive(sensitive);
                step_widgets.policy_dropdown.set_visible(true);
            }
        }

        step_widgets
            .property_rows
            .drain(..)
            .for_each(|previous_row| expander_row.remove(&previous_row));
        let forget_rows = step_presentation
            .offers_forget
            .then(|| forget_device_row(sender).upcast());
        step_widgets.property_rows = step_presentation
            .property_rows
            .iter()
            .map(build_property_row)
            .chain(forget_rows)
            .collect();
        step_widgets
            .property_rows
            .iter()
            .for_each(|property_row| expander_row.add_row(property_row));
    }
}

impl FactoryComponent for PairingStepRow {
    type ParentWidget = adw::PreferencesGroup;
    type CommandOutput = ();
    type Input = PairingStepRowInput;
    type Output = TimelineAction;
    type Init = StepPresentation;
    type Root = adw::ExpanderRow;
    type Widgets = (adw::ExpanderRow, PairingStepRowWidgets);
    type Index = DynamicIndex;

    fn init_model(
        presentation: Self::Init,
        _index: &DynamicIndex,
        _sender: FactorySender<Self>,
    ) -> Self {
        Self {
            presentation,
            previously_needed_attention: false,
            presentation_changed: true,
        }
    }

    fn init_root(&self) -> Self::Root {
        adw::ExpanderRow::builder()
            .title(self.presentation.title)
            .subtitle_lines(0)
            .build()
    }

    fn init_widgets(
        &mut self,
        _index: &DynamicIndex,
        expander_row: Self::Root,
        _returned_widget: &<Self::ParentWidget as FactoryView>::ReturnedWidget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        let status_icon = gtk::Image::new();
        let status_spinner = gtk::Spinner::builder().visible(false).build();
        expander_row.add_prefix(&status_icon);
        expander_row.add_prefix(&status_spinner);
        expander_row.add_suffix(&protocol_info_button(
            self.presentation.protocol_term,
            self.presentation.protocol_details,
        ));

        let action_button = gtk::Button::builder()
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        let button_sender = sender.clone();
        action_button.connect_clicked(move |_clicked_button| {
            button_sender.input(PairingStepRowInput::ActionButtonClicked);
        });
        expander_row.add_suffix(&action_button);

        let policy_labels: Vec<_> = POLICY_CHOICES
            .iter()
            .map(|policy_choice| policy_choice.label)
            .collect();
        let policy_dropdown = gtk::DropDown::from_strings(&policy_labels);
        policy_dropdown.set_valign(gtk::Align::Center);
        policy_dropdown.set_visible(false);
        let policy_sender = sender.clone();
        let policy_handler = policy_dropdown.connect_selected_notify(move |changed_dropdown| {
            policy_sender.input(PairingStepRowInput::PolicyChosen(
                changed_dropdown.selected(),
            ));
        });
        expander_row.add_suffix(&policy_dropdown);

        let mut step_widgets = PairingStepRowWidgets {
            status_icon,
            status_spinner,
            action_button,
            policy_dropdown,
            policy_handler,
            property_rows: Vec::new(),
        };
        self.render_step(&expander_row, &mut step_widgets, &sender);
        self.presentation_changed = false;
        (expander_row, step_widgets)
    }

    fn update(&mut self, input: Self::Input, sender: FactorySender<Self>) {
        self.presentation_changed = false;
        let timeline_action = match input {
            PairingStepRowInput::Present(presentation) => {
                self.presentation_changed = presentation != self.presentation;
                if self.presentation_changed {
                    self.previously_needed_attention = self.presentation.needs_attention;
                    self.presentation = presentation;
                }
                return;
            }
            PairingStepRowInput::ActionButtonClicked => match &self.presentation.button {
                StepButtonSlot::Shown(step_button) => TimelineAction::Step(step_button.action),
                StepButtonSlot::Hidden => return,
            },
            PairingStepRowInput::PolicyChosen(selected_position) => {
                let Some(policy_choice) = usize::try_from(selected_position)
                    .ok()
                    .and_then(|position| POLICY_CHOICES.get(position))
                else {
                    return;
                };
                TimelineAction::SetConnectionPolicy(policy_choice.policy)
            }
            PairingStepRowInput::ForgetClicked => TimelineAction::ForgetDevice,
        };
        let _timeline_gone = sender.output(timeline_action);
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: FactorySender<Self>) {
        if self.presentation_changed {
            let (expander_row, step_widgets) = widgets;
            self.render_step(expander_row, step_widgets, &sender);
        }
    }
}
