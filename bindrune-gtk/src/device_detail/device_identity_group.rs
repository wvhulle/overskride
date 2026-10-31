use bindrune_bluez::remote_device::remote_device_snapshot::{DeviceSnapshot, Manufacturer};
use relm4::{adw, adw::prelude::*, gtk::glib, prelude::*};

use crate::{
    shared_device_appearance::{describe_address_type, describe_device_type, type_guess_evidence},
    shared_page_widget_templates::PropertyRow,
};

pub struct DeviceIdentityGroup {
    snapshot: DeviceSnapshot,
}

#[derive(Debug)]
pub enum DeviceIdentityGroupInput {
    DeviceChanged(DeviceSnapshot),
}

fn describe_manufacturer(manufacturer: &Manufacturer) -> &str {
    match manufacturer {
        Manufacturer::Unknown => "Unknown",
        Manufacturer::Identified(manufacturer_name) => manufacturer_name,
    }
}

#[relm4::component(pub)]
impl SimpleComponent for DeviceIdentityGroup {
    type Init = DeviceSnapshot;
    type Input = DeviceIdentityGroupInput;
    type Output = ();

    view! {
        adw::PreferencesGroup {
            set_title: "Details",

            #[template]
            PropertyRow {
                set_title: "Address",
                #[watch]
                set_subtitle: &model.snapshot.address().to_string(),
            },
            #[template]
            PropertyRow {
                set_title: "Address Type",
                #[watch]
                set_subtitle: describe_address_type(&model.snapshot),
            },
            #[template]
            PropertyRow {
                set_title: "Manufacturer",
                #[watch]
                set_subtitle: &glib::markup_escape_text(describe_manufacturer(&model.snapshot.manufacturer)),
            },
            #[template]
            PropertyRow {
                set_title: "Type",
                #[watch]
                set_subtitle: &glib::markup_escape_text(describe_device_type(&model.snapshot)),
            },
            #[template]
            PropertyRow {
                set_title: "Type Guessed From",
                #[watch]
                set_visible: type_guess_evidence(&model.snapshot).is_some(),
                #[watch]
                set_subtitle: &glib::markup_escape_text(type_guess_evidence(&model.snapshot).unwrap_or_default()),
            },
            #[template]
            PropertyRow {
                set_title: "Advertised Profiles",
                #[watch]
                set_subtitle: &glib::markup_escape_text(&model.snapshot.service_names().join("\n")),
            },
        }
    }

    fn init(
        snapshot: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self { snapshot };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, _sender: ComponentSender<Self>) {
        match input {
            DeviceIdentityGroupInput::DeviceChanged(snapshot) => self.snapshot = snapshot,
        }
    }
}
