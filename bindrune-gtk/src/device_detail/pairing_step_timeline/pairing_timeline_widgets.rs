use bindrune_bluez::device_pairing::pairing_step_progress::ConnectionPolicy;
use relm4::{
    ComponentParts, ComponentSender, SimpleComponent, adw, adw::prelude::*, gtk, gtk::glib,
};

use crate::{
    device_detail::pairing_step_timeline::pairing_timeline_view_model::{
        PolicySelector, PropertyHint, PropertyRow, RadioControl, RadioRowPresentation,
        StepButtonSlot, StepNodeState, StepPresentation, TimelineAction, TimelinePresentation,
    },
    icon_names,
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

pub struct PairingTimeline {
    presentation: TimelinePresentation,
}

#[derive(Debug)]
pub enum PairingTimelineInput {
    Present(TimelinePresentation),
    StepButtonClicked(usize),
    PolicyChosen(u32),
    ForgetClicked,
    RadioControlClicked(RadioControl),
}

struct StepWidgets {
    expander_row: adw::ExpanderRow,
    status_icon: gtk::Image,
    status_spinner: gtk::Spinner,
    action_button: gtk::Button,
    policy_dropdown: gtk::DropDown,
    policy_handler: glib::SignalHandlerId,
    property_rows: Vec<adw::PreferencesRow>,
}

pub struct PairingTimelineWidgets {
    preferences_group: adw::PreferencesGroup,
    radio_expander: adw::ExpanderRow,
    radio_info_icon: gtk::Image,
    radio_rows: Vec<adw::ActionRow>,
    step_widgets: Vec<StepWidgets>,
    rendered_presentation: TimelinePresentation,
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

fn build_step_widgets(
    step_index: usize,
    step_presentation: &StepPresentation,
    sender: &ComponentSender<PairingTimeline>,
) -> StepWidgets {
    let expander_row = adw::ExpanderRow::builder()
        .title(step_presentation.title)
        .subtitle_lines(0)
        .build();
    let status_icon = gtk::Image::new();
    let status_spinner = gtk::Spinner::builder().visible(false).build();
    expander_row.add_prefix(&status_icon);
    expander_row.add_prefix(&status_spinner);
    expander_row.add_suffix(&protocol_info_button(
        step_presentation.protocol_term,
        step_presentation.protocol_details,
    ));

    let action_button = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .visible(false)
        .build();
    let button_sender = sender.clone();
    action_button.connect_clicked(move |_clicked_button| {
        button_sender.input(PairingTimelineInput::StepButtonClicked(step_index));
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
        policy_sender.input(PairingTimelineInput::PolicyChosen(
            changed_dropdown.selected(),
        ));
    });
    expander_row.add_suffix(&policy_dropdown);

    StepWidgets {
        expander_row,
        status_icon,
        status_spinner,
        action_button,
        policy_dropdown,
        policy_handler,
        property_rows: Vec::new(),
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

fn forget_device_row(sender: &ComponentSender<PairingTimeline>) -> adw::ActionRow {
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
        forget_sender.input(PairingTimelineInput::ForgetClicked);
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

fn render_step(
    step_widgets: &mut StepWidgets,
    step_presentation: &StepPresentation,
    previous_presentation: Option<&StepPresentation>,
    sender: &ComponentSender<PairingTimeline>,
) {
    let expander_row = &step_widgets.expander_row;
    let previously_needed_attention =
        previous_presentation.is_some_and(|previous_step| previous_step.needs_attention);
    if step_presentation.needs_attention && !previously_needed_attention {
        expander_row.set_expanded(true);
    }
    expander_row.set_visible(step_presentation.visible);
    expander_row.set_sensitive(step_presentation.sensitive);
    expander_row.set_subtitle(&glib::markup_escape_text(&step_presentation.status_text));

    let (icon_name, style_class) = node_icon(step_presentation.node_state);
    let is_running = step_presentation.node_state == StepNodeState::Running;
    NODE_STYLE_CLASSES
        .iter()
        .for_each(|node_style_class| step_widgets.status_icon.remove_css_class(node_style_class));
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

fn build_radio_row(
    radio_row: &RadioRowPresentation,
    sender: &ComponentSender<PairingTimeline>,
) -> adw::ActionRow {
    let radio_action_row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&radio_row.radio_label).as_str())
        .subtitle(radio_row.state_text)
        .subtitle_lines(0)
        .activatable(false)
        .build();
    if radio_row.may_be_stale {
        radio_action_row.add_suffix(
            &gtk::Image::builder()
                .icon_name("dialog-warning-symbolic")
                .css_classes(["warning"])
                .tooltip_text("May be stale: the device is connected through another radio")
                .build(),
        );
    }
    let button_label = match &radio_row.control {
        RadioControl::Shown => {
            radio_action_row.add_suffix(
                &gtk::Label::builder()
                    .label("Shown")
                    .css_classes(["dim-label"])
                    .build(),
            );
            return radio_action_row;
        }
        RadioControl::Show(_) => "Show",
        RadioControl::PairVia(_) => "Pair via This Radio",
    };
    let radio_button = gtk::Button::builder()
        .label(button_label)
        .valign(gtk::Align::Center)
        .build();
    let radio_sender = sender.clone();
    let radio_control = radio_row.control.clone();
    radio_button.connect_clicked(move |clicked_button| {
        clicked_button.set_sensitive(false);
        radio_sender.input(PairingTimelineInput::RadioControlClicked(
            radio_control.clone(),
        ));
    });
    radio_action_row.add_suffix(&radio_button);
    radio_action_row
}

fn render_timeline(
    widgets: &mut PairingTimelineWidgets,
    presentation: &TimelinePresentation,
    sender: &ComponentSender<PairingTimeline>,
) {
    widgets
        .preferences_group
        .set_description(Some(&glib::markup_escape_text(&presentation.summary)));

    let radios = &presentation.radios;
    if radios != &widgets.rendered_presentation.radios {
        widgets.radio_expander.set_visible(radios.visible);
        widgets
            .radio_expander
            .set_subtitle(&glib::markup_escape_text(&radios.shown_radio_label));
        widgets
            .radio_info_icon
            .set_visible(!radios.also_bonded_via.is_empty());
        widgets
            .radio_info_icon
            .set_tooltip_text(Some(&format!("Also bonded via {}", radios.also_bonded_via)));
        widgets
            .radio_rows
            .drain(..)
            .for_each(|previous_row| widgets.radio_expander.remove(&previous_row));
        widgets.radio_rows = radios
            .rows
            .iter()
            .map(|radio_row| build_radio_row(radio_row, sender))
            .collect();
        widgets
            .radio_rows
            .iter()
            .for_each(|radio_row| widgets.radio_expander.add_row(radio_row));
    }

    let rendered_steps = &widgets.rendered_presentation.steps;
    widgets
        .step_widgets
        .iter_mut()
        .zip(presentation.steps.iter())
        .enumerate()
        .filter(|(step_index, (_step_widgets, step_presentation))| {
            rendered_steps.get(*step_index) != Some(*step_presentation)
        })
        .for_each(|(step_index, (step_widgets, step_presentation))| {
            render_step(
                step_widgets,
                step_presentation,
                rendered_steps.get(step_index),
                sender,
            );
        });
    widgets.rendered_presentation = presentation.clone();
}

impl SimpleComponent for PairingTimeline {
    type Init = TimelinePresentation;
    type Input = PairingTimelineInput;
    type Output = TimelineAction;
    type Root = adw::PreferencesGroup;
    type Widgets = PairingTimelineWidgets;

    fn init_root() -> Self::Root {
        adw::PreferencesGroup::builder().title("Pairing").build()
    }

    fn init(
        presentation: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let radio_expander = adw::ExpanderRow::builder().title("Radio").build();
        radio_expander.add_prefix(&gtk::Image::from_icon_name(icon_names::shipped::BLUETOOTH));
        let radio_info_icon = gtk::Image::builder()
            .icon_name("dialog-information-symbolic")
            .visible(false)
            .build();
        radio_expander.add_suffix(&radio_info_icon);
        root.add(&radio_expander);

        let step_widgets: Vec<_> = presentation
            .steps
            .iter()
            .enumerate()
            .map(|(step_index, step_presentation)| {
                build_step_widgets(step_index, step_presentation, &sender)
            })
            .collect();
        step_widgets
            .iter()
            .for_each(|built_step| root.add(&built_step.expander_row));

        let mut widgets = PairingTimelineWidgets {
            preferences_group: root.clone(),
            radio_expander,
            radio_info_icon,
            radio_rows: Vec::new(),
            step_widgets,
            rendered_presentation: TimelinePresentation {
                steps: Vec::new(),
                ..presentation.clone()
            },
        };
        widgets.rendered_presentation.radios.visible = !presentation.radios.visible;
        render_timeline(&mut widgets, &presentation, &sender);
        ComponentParts {
            model: Self { presentation },
            widgets,
        }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>) {
        let timeline_action = match input {
            PairingTimelineInput::Present(presentation) => {
                self.presentation = presentation;
                return;
            }
            PairingTimelineInput::StepButtonClicked(step_index) => {
                match self.presentation.steps.get(step_index) {
                    Some(StepPresentation {
                        button: StepButtonSlot::Shown(step_button),
                        ..
                    }) => TimelineAction::Step(step_button.action),
                    _ => return,
                }
            }
            PairingTimelineInput::PolicyChosen(selected_position) => {
                let Some(policy_choice) = usize::try_from(selected_position)
                    .ok()
                    .and_then(|position| POLICY_CHOICES.get(position))
                else {
                    return;
                };
                TimelineAction::SetConnectionPolicy(policy_choice.policy)
            }
            PairingTimelineInput::ForgetClicked => TimelineAction::ForgetDevice,
            PairingTimelineInput::RadioControlClicked(RadioControl::Show(adapter_name)) => {
                TimelineAction::ShowRadio(adapter_name)
            }
            PairingTimelineInput::RadioControlClicked(RadioControl::PairVia(adapter_name)) => {
                TimelineAction::PairViaRadio(adapter_name)
            }
            PairingTimelineInput::RadioControlClicked(RadioControl::Shown) => return,
        };
        let _device_page_gone = sender.output(timeline_action);
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        if widgets.rendered_presentation != self.presentation {
            render_timeline(widgets, &self.presentation, &sender);
        }
    }
}
