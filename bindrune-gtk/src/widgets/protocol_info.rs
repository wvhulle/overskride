use relm4::{gtk, gtk::prelude::*};

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
