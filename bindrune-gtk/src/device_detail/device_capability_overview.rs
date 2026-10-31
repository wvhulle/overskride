use std::collections::BTreeSet;

use bindrune_bluez::remote_device::advertised_service_uuids_and_capabilities::{
    CapabilityCategory, DeviceCapability,
};
use relm4::{
    RelmWidgetExt, adw,
    adw::prelude::*,
    factory::{DynamicIndex, FactoryComponent, FactorySender, FactoryVecDeque},
    gtk,
    prelude::*,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilitySources {
    pub advertised_services: BTreeSet<bindrune_bluez::Uuid>,
    pub made_by_apple: bool,
}

#[derive(Debug)]
pub enum CapabilityOverviewInput {
    SourcesChanged(CapabilitySources),
}

#[derive(Clone, Debug)]
pub enum CapabilitySourcesInput {
    SourcesChanged(CapabilitySources),
}

pub struct CapabilityOverview {
    sources: CapabilitySources,
    sections: FactoryVecDeque<CapabilitySection>,
}

pub struct CapabilitySection {
    category: CapabilityCategory,
    offered_count: usize,
    indicators: FactoryVecDeque<CapabilityIndicator>,
}

pub struct CapabilityIndicator {
    capability: DeviceCapability,
    offered: bool,
    made_by_apple: bool,
}

pub struct CapabilitySectionInit {
    category: CapabilityCategory,
    sources: CapabilitySources,
}

pub struct CapabilityIndicatorInit {
    capability: DeviceCapability,
    sources: CapabilitySources,
}

fn count_offered(category: CapabilityCategory, sources: &CapabilitySources) -> usize {
    category
        .capabilities()
        .filter(|capability| capability.is_offered_by(&sources.advertised_services))
        .count()
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

fn status_icon_name(offered: bool) -> &'static str {
    if offered {
        "object-select-symbolic"
    } else {
        "window-close-symbolic"
    }
}

fn status_tooltip(offered: bool) -> &'static str {
    if offered {
        "Supported"
    } else {
        "Not supported"
    }
}

fn status_icon_classes(offered: bool) -> &'static [&'static str] {
    if offered {
        &["success"]
    } else {
        &["dim-label"]
    }
}

fn title_label_classes(offered: bool) -> &'static [&'static str] {
    if offered { &[] } else { &["dim-label"] }
}

#[relm4::factory(pub)]
impl FactoryComponent for CapabilityIndicator {
    type Init = CapabilityIndicatorInit;
    type Input = CapabilitySourcesInput;
    type Output = ();
    type CommandOutput = ();
    type ParentWidget = gtk::FlowBox;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Horizontal,
            set_spacing: 8,

            gtk::Image {
                #[watch]
                set_icon_name: Some(status_icon_name(self.offered)),
                #[watch]
                set_tooltip_text: Some(status_tooltip(self.offered)),
                #[watch]
                set_css_classes: status_icon_classes(self.offered),
            },

            gtk::Label {
                set_wrap: true,
                set_xalign: 0.0,
                #[watch]
                set_label: &indicator_title(self.capability, self.offered, self.made_by_apple),
                #[watch]
                set_css_classes: title_label_classes(self.offered),
            },
        }
    }

    fn init_model(
        indicator_init: Self::Init,
        _index: &DynamicIndex,
        _sender: FactorySender<Self>,
    ) -> Self {
        let CapabilityIndicatorInit {
            capability,
            sources,
        } = indicator_init;
        Self {
            capability,
            offered: capability.is_offered_by(&sources.advertised_services),
            made_by_apple: sources.made_by_apple,
        }
    }

    fn update(&mut self, input: Self::Input, _sender: FactorySender<Self>) {
        match input {
            CapabilitySourcesInput::SourcesChanged(sources) => {
                self.offered = self.capability.is_offered_by(&sources.advertised_services);
                self.made_by_apple = sources.made_by_apple;
            }
        }
    }
}

#[relm4::factory(pub)]
impl FactoryComponent for CapabilitySection {
    type Init = CapabilitySectionInit;
    type Input = CapabilitySourcesInput;
    type Output = ();
    type CommandOutput = ();
    type ParentWidget = adw::PreferencesGroup;

    view! {
        adw::PreferencesRow {
            set_activatable: false,
            #[watch]
            set_visible: self.offered_count > 0 || self.category.shown_when_empty(),

            #[wrap(Some)]
            set_child = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 6,
                set_margin_all: 12,

                gtk::Label {
                    set_label: self.category.category_title(),
                    set_xalign: 0.0,
                    add_css_class: "heading",
                },

                append: self.indicators.widget(),
            },
        }
    }

    fn init_model(
        section_init: Self::Init,
        _index: &DynamicIndex,
        _sender: FactorySender<Self>,
    ) -> Self {
        let CapabilitySectionInit { category, sources } = section_init;
        let mut indicators = FactoryVecDeque::builder()
            .launch(
                gtk::FlowBox::builder()
                    .selection_mode(gtk::SelectionMode::None)
                    .homogeneous(true)
                    .min_children_per_line(1)
                    .max_children_per_line(2)
                    .column_spacing(12)
                    .row_spacing(6)
                    .build(),
            )
            .detach();
        {
            let mut indicator_guard = indicators.guard();
            category.capabilities().for_each(|capability| {
                indicator_guard.push_back(CapabilityIndicatorInit {
                    capability,
                    sources: sources.clone(),
                });
            });
        }
        Self {
            category,
            offered_count: count_offered(category, &sources),
            indicators,
        }
    }

    fn update(&mut self, input: Self::Input, _sender: FactorySender<Self>) {
        match input {
            CapabilitySourcesInput::SourcesChanged(sources) => {
                self.offered_count = count_offered(self.category, &sources);
                self.indicators
                    .broadcast(CapabilitySourcesInput::SourcesChanged(sources));
            }
        }
    }
}

#[relm4::component(pub)]
impl SimpleComponent for CapabilityOverview {
    type Init = CapabilitySources;
    type Input = CapabilityOverviewInput;
    type Output = ();

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,

            #[local_ref]
            section_group -> adw::PreferencesGroup {
                set_title: "Capabilities",
                set_description: Some("What this device advertises it can do with this computer."),
            },
        }
    }

    fn init(
        sources: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut sections = FactoryVecDeque::builder()
            .launch(adw::PreferencesGroup::default())
            .detach();
        {
            let mut section_guard = sections.guard();
            CapabilityCategory::ALL.into_iter().for_each(|category| {
                section_guard.push_back(CapabilitySectionInit {
                    category,
                    sources: sources.clone(),
                });
            });
        }
        let model = Self { sources, sections };
        let section_group = model.sections.widget();
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, _sender: ComponentSender<Self>) {
        match input {
            CapabilityOverviewInput::SourcesChanged(sources) => {
                if sources != self.sources {
                    self.sections
                        .broadcast(CapabilitySourcesInput::SourcesChanged(sources.clone()));
                    self.sources = sources;
                }
            }
        }
    }
}
