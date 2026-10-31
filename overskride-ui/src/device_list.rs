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
use relm4::typed_view::list::{RelmListItem, TypedListView};
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, prelude::*};

use crate::device_status::{pairing_status_badge, signal_icon_name};

const HIDE_UNKNOWN_FILTER_INDEX: usize = 0;

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
    name_label: gtk::Label,
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

                #[name = "name_label"]
                gtk::Label {
                    set_xalign: 0.0,
                    set_hexpand: true,
                    set_ellipsize: gtk::pango::EllipsizeMode::End,
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
                name_label,
                status_image,
                signal_image,
            },
        )
    }

    fn bind(&mut self, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        let status_badge = pairing_status_badge(&self.snapshot);
        widgets.name_label.set_label(self.snapshot.display_name());
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

pub struct DeviceListSource {
    pub session: BluetoothSession,
    pub adapter_name: String,
    pub hide_unknown_devices: bool,
}

pub struct DeviceList {
    device_rows: TypedListView<DeviceRowItem, gtk::NoSelection>,
}

#[derive(Debug)]
pub enum DeviceListInput {
    HideUnknownDevices(bool),
    Activated(u32),
}

#[derive(Debug)]
pub enum DeviceListOutput {
    Opened(DeviceSnapshot),
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
        }
    }

    fn init(
        device_source: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut device_rows = TypedListView::with_sorting();
        device_rows.add_filter(|row: &DeviceRowItem| row.snapshot.has_meaningful_name());
        device_rows.set_filter_status(
            HIDE_UNKNOWN_FILTER_INDEX,
            device_source.hide_unknown_devices,
        );
        let model = Self { device_rows };
        let device_list_view = &model.device_rows.view;
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
            DeviceListInput::HideUnknownDevices(hide_unknown_devices) => {
                self.device_rows
                    .set_filter_status(HIDE_UNKNOWN_FILTER_INDEX, hide_unknown_devices);
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
