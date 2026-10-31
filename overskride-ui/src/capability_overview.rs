use std::collections::BTreeSet;

use overskride_bluetooth::device_capabilities::{CapabilityCategory, DeviceCapability};
use relm4::RelmWidgetExt;
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilitySources {
    pub advertised_services: BTreeSet<overskride_bluetooth::Uuid>,
    pub made_by_apple: bool,
}

#[derive(Debug)]
pub enum CapabilityOverviewInput {
    SourcesChanged(CapabilitySources),
}

struct CapabilityIndicator {
    capability: DeviceCapability,
    status_icon: gtk::Image,
    title_label: gtk::Label,
}

struct CategorySection {
    category: CapabilityCategory,
    section_row: adw::PreferencesRow,
    indicators: Vec<CapabilityIndicator>,
}

pub struct CapabilityOverview {
    sources: CapabilitySources,
    sections: Vec<CategorySection>,
}

fn build_indicator(capability: DeviceCapability, flow_box: &gtk::FlowBox) -> CapabilityIndicator {
    let status_icon = gtk::Image::new();
    let title_label = gtk::Label::builder()
        .label(capability.capability_title())
        .wrap(true)
        .xalign(0.0)
        .build();
    let indicator_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    indicator_box.append(&status_icon);
    indicator_box.append(&title_label);
    flow_box.append(&indicator_box);
    CapabilityIndicator {
        capability,
        status_icon,
        title_label,
    }
}

fn build_section(
    category: CapabilityCategory,
    overview_group: &adw::PreferencesGroup,
) -> CategorySection {
    let heading_label = gtk::Label::builder()
        .label(category.category_title())
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    let flow_box = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(2)
        .column_spacing(12)
        .row_spacing(6)
        .build();
    let section_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    section_box.set_margin_all(12);
    section_box.append(&heading_label);
    section_box.append(&flow_box);
    let section_row = adw::PreferencesRow::builder()
        .activatable(false)
        .child(&section_box)
        .build();
    overview_group.add(&section_row);
    let indicators = category
        .capabilities()
        .map(|capability| build_indicator(capability, &flow_box))
        .collect();
    CategorySection {
        category,
        section_row,
        indicators,
    }
}

fn indicator_title(capability: DeviceCapability, offered: bool, made_by_apple: bool) -> String {
    match (capability, offered, made_by_apple) {
        (DeviceCapability::ReceivesFiles, false, true) => {
            format!(
                "{} (Apple devices use AirDrop)",
                capability.capability_title()
            )
        }
        _ => capability.capability_title().to_string(),
    }
}

impl CapabilityIndicator {
    fn show_offered(&self, sources: &CapabilitySources) -> bool {
        let offered = self.capability.is_offered_by(&sources.advertised_services);
        let (icon_name, tooltip_text) = if offered {
            ("object-select-symbolic", "Supported")
        } else {
            ("window-close-symbolic", "Not supported")
        };
        self.status_icon.set_icon_name(Some(icon_name));
        self.status_icon.set_tooltip_text(Some(tooltip_text));
        self.status_icon.set_css_classes(if offered {
            &["success"]
        } else {
            &["dim-label"]
        });
        self.title_label
            .set_css_classes(if offered { &[] } else { &["dim-label"] });
        self.title_label.set_label(&indicator_title(
            self.capability,
            offered,
            sources.made_by_apple,
        ));
        offered
    }
}

impl CategorySection {
    fn show_sources(&self, sources: &CapabilitySources) {
        let offered_count = self
            .indicators
            .iter()
            .filter(|indicator| indicator.show_offered(sources))
            .count();
        self.section_row
            .set_visible(offered_count > 0 || self.category.shown_when_empty());
    }
}

impl CapabilityOverview {
    fn show_sources(&self) {
        self.sections
            .iter()
            .for_each(|section| section.show_sources(&self.sources));
    }
}

impl SimpleComponent for CapabilityOverview {
    type Init = CapabilitySources;
    type Input = CapabilityOverviewInput;
    type Output = ();
    type Root = adw::PreferencesGroup;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::PreferencesGroup::builder()
            .title("Capabilities")
            .description("What this device advertises it can do with this computer.")
            .build()
    }

    fn init(
        sources: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let sections = CapabilityCategory::ALL
            .into_iter()
            .map(|category| build_section(category, &root))
            .collect();
        let model = Self { sources, sections };
        model.show_sources();
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, input: Self::Input, _sender: ComponentSender<Self>) {
        match input {
            CapabilityOverviewInput::SourcesChanged(sources) => {
                if sources != self.sources {
                    self.sources = sources;
                    self.show_sources();
                }
            }
        }
    }
}
