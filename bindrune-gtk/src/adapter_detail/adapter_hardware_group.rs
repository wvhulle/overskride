use bindrune_bluez::bluetooth_adapter::adapter_snapshot_and_discovery::AdapterSnapshot;
use relm4::{adw, adw::prelude::*, prelude::*};

pub struct AdapterHardwareGroup {
    adapter: AdapterSnapshot,
}

#[derive(Debug)]
pub enum AdapterHardwareGroupInput {
    AdapterChanged(AdapterSnapshot),
}

#[relm4::component(pub)]
impl SimpleComponent for AdapterHardwareGroup {
    type Init = AdapterSnapshot;
    type Input = AdapterHardwareGroupInput;
    type Output = ();

    view! {
        adw::PreferencesGroup {
            set_title: "Hardware",

            adw::ActionRow {
                set_title: "Connection",
                add_css_class: "property",
                #[watch]
                set_subtitle: model.adapter.hardware.attachment.describe_attachment(),
            },

            adw::ActionRow {
                set_title: "Vendor",
                set_subtitle_selectable: true,
                add_css_class: "property",
                #[watch]
                set_visible: model.adapter.hardware.vendor_name.is_some(),
                #[watch]
                set_subtitle: model.adapter.hardware.vendor_name.as_deref().unwrap_or_default(),
            },

            adw::ActionRow {
                set_title: "Model",
                set_subtitle_selectable: true,
                add_css_class: "property",
                #[watch]
                set_visible: model.adapter.hardware.model_name.is_some(),
                #[watch]
                set_subtitle: model.adapter.hardware.model_name.as_deref().unwrap_or_default(),
            },

            adw::ActionRow {
                set_title: "Kernel Driver",
                set_subtitle_selectable: true,
                add_css_class: "property",
                #[watch]
                set_visible: model.adapter.hardware.kernel_driver.is_some(),
                #[watch]
                set_subtitle: model.adapter.hardware.kernel_driver.as_deref().unwrap_or_default(),
            },

            adw::ActionRow {
                set_title: "Hardware ID",
                set_subtitle_selectable: true,
                add_css_class: "property",
                #[watch]
                set_visible: model.adapter.hardware.hardware_id.is_some(),
                #[watch]
                set_subtitle: model.adapter.hardware.hardware_id.as_deref().unwrap_or_default(),
            },

            adw::ActionRow {
                set_title: "Kernel Name",
                set_subtitle_selectable: true,
                add_css_class: "property",
                #[watch]
                set_subtitle: &model.adapter.adapter_name,
            },
        }
    }

    fn init(
        adapter: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self { adapter };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, _sender: ComponentSender<Self>) {
        match input {
            AdapterHardwareGroupInput::AdapterChanged(adapter_snapshot) => {
                self.adapter = adapter_snapshot;
            }
        }
    }
}
