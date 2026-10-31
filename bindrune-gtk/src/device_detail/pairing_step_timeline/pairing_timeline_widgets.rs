use relm4::{
    ComponentParts, ComponentSender, SimpleComponent, adw, adw::prelude::*,
    factory::FactoryVecDeque, gtk, gtk::glib,
};

use crate::{
    device_detail::pairing_step_timeline::{
        adapter_bond_row::AdapterBondRow,
        pairing_step_row::{PairingStepRow, PairingStepRowInput},
        pairing_timeline_view_model::{TimelineAction, TimelinePresentation},
    },
    icon_names,
};

pub struct PairingTimeline {
    presentation: TimelinePresentation,
    step_rows: FactoryVecDeque<PairingStepRow>,
    adapter_bond_rows: FactoryVecDeque<AdapterBondRow>,
}

#[derive(Debug)]
pub enum PairingTimelineInput {
    Present(TimelinePresentation),
    RowActionChosen(TimelineAction),
}

pub struct PairingTimelineWidgets {
    preferences_group: adw::PreferencesGroup,
    radio_expander: adw::ExpanderRow,
    radio_info_icon: gtk::Image,
}

fn render_timeline_header(widgets: &PairingTimelineWidgets, presentation: &TimelinePresentation) {
    let radios = &presentation.radios;
    widgets
        .preferences_group
        .set_description(Some(&glib::markup_escape_text(&presentation.summary)));
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
}

impl PairingTimeline {
    fn replace_adapter_bond_rows(&mut self) {
        let mut adapter_bond_rows = self.adapter_bond_rows.guard();
        adapter_bond_rows.clear();
        self.presentation
            .radios
            .rows
            .iter()
            .cloned()
            .for_each(|radio_row| {
                adapter_bond_rows.push_back(radio_row);
            });
    }
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

        let adapter_bond_rows = FactoryVecDeque::builder()
            .launch(radio_expander.clone())
            .forward(sender.input_sender(), PairingTimelineInput::RowActionChosen);
        let mut step_rows = FactoryVecDeque::builder()
            .launch(root.clone())
            .forward(sender.input_sender(), PairingTimelineInput::RowActionChosen);
        presentation
            .steps
            .iter()
            .cloned()
            .for_each(|step_presentation| {
                step_rows.guard().push_back(step_presentation);
            });

        let mut model = Self {
            presentation,
            step_rows,
            adapter_bond_rows,
        };
        model.replace_adapter_bond_rows();
        let widgets = PairingTimelineWidgets {
            preferences_group: root,
            radio_expander,
            radio_info_icon,
        };
        render_timeline_header(&widgets, &model.presentation);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>) {
        match input {
            PairingTimelineInput::Present(presentation) => {
                let radios_changed = presentation.radios != self.presentation.radios;
                presentation
                    .steps
                    .iter()
                    .take(self.step_rows.len())
                    .cloned()
                    .enumerate()
                    .for_each(|(step_index, step_presentation)| {
                        self.step_rows
                            .send(step_index, PairingStepRowInput::Present(step_presentation));
                    });
                self.presentation = presentation;
                if radios_changed {
                    self.replace_adapter_bond_rows();
                }
            }
            PairingTimelineInput::RowActionChosen(timeline_action) => {
                let _device_page_gone = sender.output(timeline_action);
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        render_timeline_header(widgets, &self.presentation);
    }
}
