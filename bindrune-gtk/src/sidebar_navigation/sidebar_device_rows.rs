use bindrune_bluez::{
    bluez_dbus_session::BluetoothSession,
    bluez_operation_error::BluetoothError,
    remote_device::{
        remote_device_change_stream::{DeviceChange, device_changes},
        remote_device_snapshot::DeviceSnapshot,
    },
};
use futures::StreamExt;
use relm4::{
    ComponentParts, ComponentSender, RelmWidgetExt, adw, gtk, gtk::prelude::*, prelude::*,
    typed_view::list::TypedListView,
};

use crate::{
    icon_names,
    sidebar_navigation::{
        sidebar_device_row_item::DeviceRowItem,
        unnamed_device_count_section::{
            NAMED_ONLY_FILTER_INDEX, describe_hidden_unnamed, install_name_sections,
        },
    },
};

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
    fn position_of(&self, address: bindrune_bluez::Address) -> Option<u32> {
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
                set_icon_name: Some(icon_names::custom::NO_BLUETOOTH_SYMBOLIC),
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
