use overskride_bluetooth::radios::RadioDescription;
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};
use relm4::gtk;

#[derive(Debug)]
pub struct AdapterRow {
    pub radio: RadioDescription,
    pub active: bool,
}

pub struct AdapterRowInit {
    pub radio: RadioDescription,
    pub active: bool,
}

#[relm4::factory(pub)]
impl FactoryComponent for AdapterRow {
    type Init = AdapterRowInit;
    type Input = ();
    type Output = String;
    type CommandOutput = ();
    type ParentWidget = adw::PreferencesGroup;

    view! {
        adw::ActionRow {
            set_title: &self.radio.radio_label,
            set_subtitle: &self.radio.adapter_name,
            set_activatable: true,
            connect_activated[sender, adapter_name = self.radio.adapter_name.clone()] => move |_activated_row| {
                let _main_view_gone = sender.output(adapter_name.clone());
            },

            add_prefix = &gtk::Image {
                set_icon_name: Some("bluetooth-symbolic"),
            },

            add_suffix = &gtk::Image {
                set_icon_name: Some("object-select-symbolic"),
                set_tooltip_text: Some("Devices below belong to this adapter"),
                #[watch]
                set_visible: self.active,
            },

            add_suffix = &gtk::Image {
                set_icon_name: Some("go-next-symbolic"),
            },
        }
    }

    fn init_model(
        row_init: Self::Init,
        _index: &DynamicIndex,
        _sender: FactorySender<Self>,
    ) -> Self {
        Self {
            radio: row_init.radio,
            active: row_init.active,
        }
    }
}
