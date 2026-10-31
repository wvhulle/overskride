use std::cmp::Ordering;

use futures::StreamExt;
use overskride_bluetooth::bluetooth_error::BluetoothError;
use overskride_bluetooth::device_changes::{DeviceChange, device_changes};
use overskride_bluetooth::device_ordering::compare_devices;
use overskride_bluetooth::device_snapshot::DeviceSnapshot;
use overskride_bluetooth::session::BluetoothSession;
use relm4::adw;
use relm4::gtk;
use relm4::gtk::prelude::*;
use relm4::gtk::{gio, glib};
use relm4::typed_view::list::{RelmListItem, TypedListView};
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, prelude::*};

use crate::device_status::{pairing_status_badge, signal_icon_name, symbolic_device_icon_name};

const NAMED_ONLY_FILTER_INDEX: usize = 0;
const NAMELESS_SECTION_TITLE: &str = "Without a Name";
const NAMELESS_SECTION_EXPLANATION: &str = "These devices don't broadcast a name, often to protect the privacy of their owner.\n\nThe shown type is a guess based on what they do broadcast: the manufacturer ID and its payload, the advertised services, and the appearance value. Open a device to see what the guess was based on.\n\nDevices with a random address change it every few minutes, so one device can show up more than once.";

pub struct DeviceRowItem {
    snapshot: DeviceSnapshot,
}

impl PartialEq for DeviceRowItem {
    fn eq(&self, other_row: &Self) -> bool {
        self.cmp(other_row) == Ordering::Equal
    }
}

impl Eq for DeviceRowItem {}

impl PartialOrd for DeviceRowItem {
    fn partial_cmp(&self, other_row: &Self) -> Option<Ordering> {
        Some(self.cmp(other_row))
    }
}

impl Ord for DeviceRowItem {
    fn cmp(&self, other_row: &Self) -> Ordering {
        compare_devices(&self.snapshot, &other_row.snapshot)
    }
}

pub struct DeviceRowWidgets {
    device_image: gtk::Image,
    name_label: gtk::Label,
    address_tail_label: gtk::Label,
    status_image: gtk::Image,
    signal_image: gtk::Image,
}

impl RelmListItem for DeviceRowItem {
    type Root = gtk::Box;
    type Widgets = DeviceRowWidgets;

    fn setup(_list_item: &gtk::ListItem) -> (gtk::Box, DeviceRowWidgets) {
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
            DeviceRowWidgets {
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

fn row_has_meaningful_name(list_object: &glib::Object) -> bool {
    list_object
        .downcast_ref::<glib::BoxedAnyObject>()
        .and_then(|boxed_row| boxed_row.try_borrow::<DeviceRowItem>().ok())
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

fn install_name_sections(device_list_view: &gtk::ListView) {
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

fn describe_hidden_unnamed(unnamed_device_count: usize) -> String {
    if unnamed_device_count == 1 {
        "1 device without a name is hidden".to_string()
    } else {
        format!("{unnamed_device_count} devices without a name are hidden")
    }
}

pub struct DeviceListSource {
    pub session: BluetoothSession,
    pub adapter_name: String,
    pub show_unnamed_devices: bool,
}

pub struct DeviceList {
    device_rows: TypedListView<DeviceRowItem, gtk::NoSelection>,
    show_unnamed_devices: bool,
    unnamed_device_count: usize,
}

#[derive(Debug)]
pub enum DeviceListInput {
    ShowUnnamedDevices(bool),
    ShowUnnamedRequested,
    Activated(u32),
}

#[derive(Debug)]
pub enum DeviceListOutput {
    Opened(DeviceSnapshot),
    ShowUnnamedRequested,
    Changed(DeviceChange),
    Unavailable(BluetoothError),
}

#[derive(Debug)]
pub enum DeviceStreamEvent {
    Change(DeviceChange),
    Unavailable(BluetoothError),
}

impl DeviceList {
    fn position_of(&self, address: overskride_bluetooth::Address) -> Option<u32> {
        self.device_rows
            .find(|row| row.snapshot.address() == address)
    }

    fn put_snapshot(&mut self, snapshot: DeviceSnapshot) {
        if let Some(existing_position) = self.position_of(snapshot.address()) {
            self.device_rows.remove(existing_position);
        }
        self.device_rows.append(DeviceRowItem { snapshot });
    }

    fn count_unnamed_devices(&mut self) {
        self.unnamed_device_count = self
            .device_rows
            .iter()
            .filter(|device_row| !device_row.borrow().snapshot.has_meaningful_name())
            .count();
    }

    fn hides_unnamed_devices(&self) -> bool {
        !self.show_unnamed_devices && self.unnamed_device_count > 0
    }

    fn apply_change(&mut self, device_change: &DeviceChange) {
        match device_change {
            DeviceChange::Appeared(snapshot) | DeviceChange::Updated(snapshot) => {
                self.put_snapshot(snapshot.clone());
            }
            DeviceChange::Disappeared(address) => {
                if let Some(existing_position) = self.position_of(*address) {
                    self.device_rows.remove(existing_position);
                }
            }
        }
        self.count_unnamed_devices();
    }
}

#[relm4::component(pub)]
impl Component for DeviceList {
    type Init = DeviceListSource;
    type Input = DeviceListInput;
    type Output = DeviceListOutput;
    type CommandOutput = DeviceStreamEvent;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,

            adw::StatusPage {
                set_icon_name: Some("no-bluetooth-symbolic"),
                set_title: "No Known Devices",
                set_description: Some("Press Scan to find new ones"),
                add_css_class: "compact",
                #[watch]
                set_visible: model.device_rows.is_empty(),
            },

            #[local_ref]
            device_list_view -> gtk::ListView {
                add_css_class: "navigation-sidebar",
                set_single_click_activate: true,
                #[watch]
                set_visible: !model.device_rows.is_empty(),
                connect_activate[sender] => move |_list_view, position| {
                    sender.input(DeviceListInput::Activated(position));
                },
            },

            gtk::Box {
                set_spacing: 6,
                set_margin_vertical: 8,
                set_margin_horizontal: 12,
                #[watch]
                set_visible: model.hides_unnamed_devices(),

                gtk::Label {
                    set_xalign: 0.0,
                    set_hexpand: true,
                    set_wrap: true,
                    set_css_classes: &["dim-label", "caption"],
                    #[watch]
                    set_label: &describe_hidden_unnamed(model.unnamed_device_count),
                },

                gtk::Button {
                    set_label: "Show",
                    set_valign: gtk::Align::Center,
                    set_css_classes: &["flat", "caption"],
                    connect_clicked => DeviceListInput::ShowUnnamedRequested,
                },
            },
        }
    }

    fn init(
        device_source: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut device_rows = TypedListView::with_sorting();
        device_rows.add_filter(|row: &DeviceRowItem| row.snapshot.has_meaningful_name());
        device_rows.set_filter_status(NAMED_ONLY_FILTER_INDEX, !device_source.show_unnamed_devices);
        let model = Self {
            device_rows,
            show_unnamed_devices: device_source.show_unnamed_devices,
            unnamed_device_count: 0,
        };
        let device_list_view = &model.device_rows.view;
        install_name_sections(device_list_view);
        let widgets = view_output!();

        let DeviceListSource {
            session,
            adapter_name,
            ..
        } = device_source;
        sender.command(move |command_output, shutdown| {
            shutdown
                .register(async move {
                    match device_changes(&session, &adapter_name).await {
                        Ok(change_stream) => {
                            change_stream
                                .for_each(|device_change| {
                                    command_output.emit(DeviceStreamEvent::Change(device_change));
                                    futures::future::ready(())
                                })
                                .await;
                        }
                        Err(subscription_error) => {
                            command_output.emit(DeviceStreamEvent::Unavailable(subscription_error));
                        }
                    }
                })
                .drop_on_shutdown()
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match input {
            DeviceListInput::ShowUnnamedDevices(show_unnamed_devices) => {
                self.show_unnamed_devices = show_unnamed_devices;
                self.device_rows
                    .set_filter_status(NAMED_ONLY_FILTER_INDEX, !show_unnamed_devices);
            }
            DeviceListInput::ShowUnnamedRequested => {
                let _app_gone = sender.output(DeviceListOutput::ShowUnnamedRequested);
            }
            DeviceListInput::Activated(visible_position) => {
                let Some(activated_row) = self.device_rows.get_visible(visible_position) else {
                    return;
                };
                let snapshot = activated_row.borrow().snapshot.clone();
                let _app_gone = sender.output(DeviceListOutput::Opened(snapshot));
            }
        }
    }

    fn update_cmd(
        &mut self,
        stream_event: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        let output = match stream_event {
            DeviceStreamEvent::Change(device_change) => {
                self.apply_change(&device_change);
                DeviceListOutput::Changed(device_change)
            }
            DeviceStreamEvent::Unavailable(subscription_error) => {
                DeviceListOutput::Unavailable(subscription_error)
            }
        };
        let _app_gone = sender.output(output);
    }
}
