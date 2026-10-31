use relm4::{
    adw,
    adw::prelude::*,
    factory::{DynamicIndex, FactoryComponent, FactorySender, FactoryView},
    gtk,
    gtk::glib,
};

use crate::device_detail::pairing_step_timeline::pairing_timeline_view_model::{
    RadioControl, RadioRowPresentation, TimelineAction,
};

#[derive(Debug)]
pub struct AdapterBondRow {
    presentation: RadioRowPresentation,
}

#[derive(Debug)]
pub enum AdapterBondRowInput {
    ControlClicked,
}

fn control_button_label(radio_control: &RadioControl) -> Option<&'static str> {
    match radio_control {
        RadioControl::Shown => None,
        RadioControl::Show(_) => Some("Show"),
        RadioControl::PairVia(_) => Some("Pair via This Radio"),
    }
}

impl FactoryComponent for AdapterBondRow {
    type ParentWidget = adw::ExpanderRow;
    type CommandOutput = ();
    type Input = AdapterBondRowInput;
    type Output = TimelineAction;
    type Init = RadioRowPresentation;
    type Root = adw::ActionRow;
    type Widgets = ();
    type Index = DynamicIndex;

    fn init_model(
        presentation: Self::Init,
        _index: &DynamicIndex,
        _sender: FactorySender<Self>,
    ) -> Self {
        Self { presentation }
    }

    fn init_root(&self) -> Self::Root {
        adw::ActionRow::builder()
            .title(glib::markup_escape_text(&self.presentation.radio_label).as_str())
            .subtitle(self.presentation.state_text)
            .subtitle_lines(0)
            .activatable(false)
            .build()
    }

    fn init_widgets(
        &mut self,
        _index: &DynamicIndex,
        radio_action_row: Self::Root,
        _returned_widget: &<Self::ParentWidget as FactoryView>::ReturnedWidget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        if self.presentation.may_be_stale {
            radio_action_row.add_suffix(
                &gtk::Image::builder()
                    .icon_name("dialog-warning-symbolic")
                    .css_classes(["warning"])
                    .tooltip_text("May be stale: the device is connected through another radio")
                    .build(),
            );
        }
        let Some(button_label) = control_button_label(&self.presentation.control) else {
            radio_action_row.add_suffix(
                &gtk::Label::builder()
                    .label("Shown")
                    .css_classes(["dim-label"])
                    .build(),
            );
            return;
        };
        let radio_button = gtk::Button::builder()
            .label(button_label)
            .valign(gtk::Align::Center)
            .build();
        radio_button.connect_clicked(move |clicked_button| {
            clicked_button.set_sensitive(false);
            sender.input(AdapterBondRowInput::ControlClicked);
        });
        radio_action_row.add_suffix(&radio_button);
    }

    fn update(&mut self, input: Self::Input, sender: FactorySender<Self>) {
        let timeline_action = match input {
            AdapterBondRowInput::ControlClicked => match &self.presentation.control {
                RadioControl::Show(adapter_name) => TimelineAction::ShowRadio(adapter_name.clone()),
                RadioControl::PairVia(adapter_name) => {
                    TimelineAction::PairViaRadio(adapter_name.clone())
                }
                RadioControl::Shown => return,
            },
        };
        let _timeline_gone = sender.output(timeline_action);
    }
}
