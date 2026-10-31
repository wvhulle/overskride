use std::cmp::Ordering;

use bindrune_bluez::remote_device::remote_device_snapshot::{DeviceSnapshot, compare_devices};
use relm4::{RelmWidgetExt, gtk, gtk::prelude::*, typed_view::list::RelmListItem};

use crate::shared_device_appearance::{
    pairing_status_badge, signal_icon_name, symbolic_device_icon_name,
};

pub struct SidebarDeviceRowItem {
    pub(super) snapshot: DeviceSnapshot,
}

impl PartialEq for SidebarDeviceRowItem {
    fn eq(&self, other_row: &Self) -> bool {
        self.cmp(other_row) == Ordering::Equal
    }
}

impl Eq for SidebarDeviceRowItem {}

impl PartialOrd for SidebarDeviceRowItem {
    fn partial_cmp(&self, other_row: &Self) -> Option<Ordering> {
        Some(self.cmp(other_row))
    }
}

impl Ord for SidebarDeviceRowItem {
    fn cmp(&self, other_row: &Self) -> Ordering {
        compare_devices(&self.snapshot, &other_row.snapshot)
    }
}

pub struct SidebarDeviceRowWidgets {
    device_image: gtk::Image,
    name_label: gtk::Label,
    address_tail_label: gtk::Label,
    status_image: gtk::Image,
    signal_image: gtk::Image,
}

impl RelmListItem for SidebarDeviceRowItem {
    type Root = gtk::Box;
    type Widgets = SidebarDeviceRowWidgets;

    fn setup(_list_item: &gtk::ListItem) -> (gtk::Box, SidebarDeviceRowWidgets) {
        relm4::view! {
            row_box = gtk::Box {
                set_spacing: 10,
                set_margin_vertical: 8,
                set_margin_horizontal: 6,

                #[name = "device_image"]
                gtk::Image,

                #[name = "name_label"]
                gtk::Label {
                    set_xalign: 0.0,
                    set_hexpand: true,
                    set_ellipsize: gtk::pango::EllipsizeMode::End,
                },

                #[name = "address_tail_label"]
                gtk::Label {
                    set_css_classes: &["dim-label", "caption", "monospace"],
                },

                #[name = "status_image"]
                gtk::Image,

                #[name = "signal_image"]
                gtk::Image,
            }
        }
        (
            row_box,
            SidebarDeviceRowWidgets {
                device_image,
                name_label,
                address_tail_label,
                status_image,
                signal_image,
            },
        )
    }

    fn bind(&mut self, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        let status_badge = pairing_status_badge(&self.snapshot);
        widgets
            .device_image
            .set_icon_name(Some(&symbolic_device_icon_name(&self.snapshot)));
        widgets.name_label.set_label(self.snapshot.display_name());
        widgets
            .address_tail_label
            .set_label(&self.snapshot.short_address());
        widgets
            .address_tail_label
            .set_visible(!self.snapshot.has_meaningful_name());
        widgets
            .status_image
            .set_icon_name(Some(status_badge.icon_name));
        widgets
            .status_image
            .set_css_classes(&[status_badge.style_class]);
        widgets
            .status_image
            .set_tooltip_text(Some(&status_badge.tooltip));
        widgets.status_image.set_visible(status_badge.visible);
        widgets
            .signal_image
            .set_icon_name(Some(signal_icon_name(self.snapshot.signal_strength)));
    }
}
