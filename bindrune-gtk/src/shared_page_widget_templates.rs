use relm4::{RelmWidgetExt, WidgetTemplate, adw, adw::prelude::*, gtk};

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

pub fn protocol_info_button(protocol_term: &str, protocol_details: &str) -> gtk::MenuButton {
    let popover_content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    let term_label = gtk::Label::builder()
        .label(protocol_term)
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    let details_label = gtk::Label::builder()
        .label(protocol_details)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(42)
        .build();
    popover_content.append(&term_label);
    popover_content.append(&details_label);
    gtk::MenuButton::builder()
        .icon_name("help-about-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(protocol_term)
        .css_classes(["flat"])
        .popover(&gtk::Popover::builder().child(&popover_content).build())
        .build()
}
