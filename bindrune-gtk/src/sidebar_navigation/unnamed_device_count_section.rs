use relm4::{
    RelmWidgetExt, gtk,
    gtk::{gio, glib, prelude::*},
};

use crate::sidebar_navigation::sidebar_device_row_item::SidebarDeviceRowItem;

pub(super) const NAMED_ONLY_FILTER_INDEX: usize = 0;
const NAMELESS_SECTION_TITLE: &str = "Without a Name";
const NAMELESS_SECTION_EXPLANATION: &str =
    "These devices don't broadcast a name, often to protect the privacy of their owner.\n\nThe \
     shown type is a guess based on what they do broadcast: the manufacturer ID and its payload, \
     the advertised services, and the appearance value. Open a device to see what the guess was \
     based on.\n\nDevices with a random address change it every few minutes, so one device can \
     show up more than once.";

fn row_has_meaningful_name(list_object: &glib::Object) -> bool {
    list_object
        .downcast_ref::<glib::BoxedAnyObject>()
        .and_then(|boxed_row| boxed_row.try_borrow::<SidebarDeviceRowItem>().ok())
        .is_some_and(|device_row| device_row.snapshot.has_meaningful_name())
}

fn find_sort_model(list_model: Option<gio::ListModel>) -> Option<gtk::SortListModel> {
    let list_model = list_model?;
    if let Some(sort_model) = list_model.downcast_ref::<gtk::SortListModel>() {
        return Some(sort_model.clone());
    }
    if let Some(filter_model) = list_model.downcast_ref::<gtk::FilterListModel>() {
        return find_sort_model(filter_model.model());
    }
    if let Some(no_selection) = list_model.downcast_ref::<gtk::NoSelection>() {
        return find_sort_model(no_selection.model());
    }
    None
}

pub(super) fn install_name_sections(device_list_view: &gtk::ListView) {
    let name_section_sorter = gtk::CustomSorter::new(|first_row, second_row| {
        row_has_meaningful_name(second_row)
            .cmp(&row_has_meaningful_name(first_row))
            .into()
    });
    if let Some(sort_model) = find_sort_model(
        device_list_view
            .model()
            .map(|selection_model| selection_model.upcast()),
    ) {
        sort_model.set_section_sorter(Some(&name_section_sorter));
    }

    let header_factory = gtk::SignalListItemFactory::new();
    header_factory.connect_setup(|_factory, list_object| {
        let Some(list_header) = list_object.downcast_ref::<gtk::ListHeader>() else {
            return;
        };
        relm4::view! {
            header_box = gtk::Box {
                set_spacing: 6,
                set_margin_top: 12,
                set_margin_horizontal: 6,

                gtk::Label {
                    set_label: NAMELESS_SECTION_TITLE,
                    set_xalign: 0.0,
                    set_hexpand: true,
                    add_css_class: "heading",
                },

                gtk::MenuButton {
                    set_icon_name: "help-about-symbolic",
                    set_valign: gtk::Align::Center,
                    set_tooltip_text: Some("Why these devices have no name"),
                    set_css_classes: &["flat", "circular"],
                    #[wrap(Some)]
                    set_popover = &gtk::Popover {
                        gtk::Label {
                            set_label: NAMELESS_SECTION_EXPLANATION,
                            set_wrap: true,
                            set_max_width_chars: 40,
                            set_xalign: 0.0,
                            set_margin_all: 6,
                        },
                    },
                },
            }
        }
        list_header.set_child(Some(&header_box));
    });
    header_factory.connect_bind(|_factory, list_object| {
        let Some(list_header) = list_object.downcast_ref::<gtk::ListHeader>() else {
            return;
        };
        let nameless_section = list_header
            .item()
            .is_some_and(|first_row| !row_has_meaningful_name(&first_row));
        if let Some(header_box) = list_header.child() {
            header_box.set_visible(nameless_section);
        }
    });
    device_list_view.set_header_factory(Some(&header_factory));
}

pub(super) fn describe_hidden_unnamed(unnamed_device_count: usize) -> String {
    if unnamed_device_count == 1 {
        "1 device without a name is hidden".to_string()
    } else {
        format!("{unnamed_device_count} devices without a name are hidden")
    }
}
