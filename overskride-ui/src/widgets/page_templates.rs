use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::{RelmWidgetExt, WidgetTemplate};

#[relm4::widget_template(pub)]
impl WidgetTemplate for ClampedPage {
    view! {
        gtk::ScrolledWindow {
            set_propagate_natural_height: true,
            set_vexpand: true,

            adw::Clamp {
                set_maximum_size: 500,
                set_margin_all: 32,

                #[name = "page_content"]
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 18,
                },
            },
        }
    }
}

#[relm4::widget_template(pub)]
impl WidgetTemplate for PageHeader {
    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 20,

            #[name = "header_icon"]
            gtk::Image {
                set_pixel_size: 80,
            },

            #[name = "header_title"]
            gtk::Label {
                set_wrap: true,
                set_justify: gtk::Justification::Center,
                add_css_class: "title-1",
            },
        }
    }
}

#[relm4::widget_template(pub)]
impl WidgetTemplate for PropertyRow {
    view! {
        adw::ActionRow {
            set_subtitle_selectable: true,
            set_subtitle_lines: 0,
            add_css_class: "property",
        }
    }
}
