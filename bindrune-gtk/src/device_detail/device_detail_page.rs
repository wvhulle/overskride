use std::path::PathBuf;

use bindrune_bluez::{
    bluez_dbus_session::BluetoothSession, bluez_operation_error::BluetoothError,
    link_quality::disconnect_reason_history::ConnectionHistory,
    obex_file_transfer::obex_transfer_progress::TransferHandle,
    remote_device::remote_device_snapshot::DeviceSnapshot,
};
use relm4::{gtk, prelude::*};

use crate::{
    device_detail::{
        audio_profile_selector_group::{AudioProfileSelectorGroup, AudioProfileSelectorGroupInput},
        device_capability_overview::{
            CapabilityOverview, CapabilityOverviewInput, CapabilitySources,
        },
        device_identity_group::{DeviceIdentityGroup, DeviceIdentityGroupInput},
        device_name_group::{DeviceNameGroup, DeviceNameGroupInit, DeviceNameGroupInput},
        file_transfer_group::{
            FileTransferGroup, FileTransferGroupInit, FileTransferGroupInput,
            FileTransferGroupOutput, ObexAvailability,
        },
        pairing_step_timeline::pairing_timeline_widgets::{
            PairingTimeline, PairingTimelineInit, PairingTimelineInput, PairingTimelineOutput,
        },
    },
    shared_device_appearance::device_icon_name,
    shared_page_widget_templates::ClampedPage,
};

pub struct DevicePageInit {
    pub session: BluetoothSession,
    pub obex: ObexAvailability,
    pub adapter_address: bindrune_bluez::Address,
    pub accepts_files: bool,
    pub received_files_folder: PathBuf,
    pub snapshot: DeviceSnapshot,
    pub connection_history: ConnectionHistory,
}

pub struct DevicePage {
    snapshot: DeviceSnapshot,
    name_group: Controller<DeviceNameGroup>,
    timeline: Controller<PairingTimeline>,
    audio_group: Controller<AudioProfileSelectorGroup>,
    file_transfer_group: Controller<FileTransferGroup>,
    capability_overview: Controller<CapabilityOverview>,
    identity_group: Controller<DeviceIdentityGroup>,
}

#[derive(Debug)]
pub enum DevicePageInput {
    DeviceChanged(DeviceSnapshot),
    ConnectionHistoryChanged(ConnectionHistory),
    AutoAcceptChanged(bool),
    GroupFailed(BluetoothError),
    TimelineReported(PairingTimelineOutput),
    FileTransferReported(FileTransferGroupOutput),
}

#[derive(Debug)]
pub enum DevicePageOutput {
    Failed(BluetoothError),
    Forgotten {
        hint: String,
        rescan: bool,
    },
    AutoAcceptChosen {
        address: bindrune_bluez::Address,
        accepts_files: bool,
    },
    ShowRadio(String),
    PairViaRadio(String),
    TransfersStarted(Vec<TransferHandle>),
}

fn capability_sources(snapshot: &DeviceSnapshot) -> CapabilitySources {
    CapabilitySources {
        advertised_services: snapshot.advertised_services(),
        made_by_apple: snapshot.made_by_apple(),
    }
}

impl DevicePage {
    pub fn shown_address(&self) -> bindrune_bluez::Address {
        self.snapshot.address()
    }

    fn forward_device_change(&self) {
        self.name_group
            .emit(DeviceNameGroupInput::DeviceChanged(self.snapshot.clone()));
        self.timeline
            .emit(PairingTimelineInput::DeviceChanged(self.snapshot.clone()));
        self.audio_group
            .emit(AudioProfileSelectorGroupInput::DeviceChanged(
                self.snapshot.clone(),
            ));
        self.file_transfer_group
            .emit(FileTransferGroupInput::DeviceChanged(self.snapshot.clone()));
        self.capability_overview
            .emit(CapabilityOverviewInput::SourcesChanged(capability_sources(
                &self.snapshot,
            )));
        self.identity_group
            .emit(DeviceIdentityGroupInput::DeviceChanged(
                self.snapshot.clone(),
            ));
    }
}

fn page_output_from_timeline(timeline_output: PairingTimelineOutput) -> DevicePageOutput {
    match timeline_output {
        PairingTimelineOutput::Failed(timeline_failure) => {
            DevicePageOutput::Failed(timeline_failure)
        }
        PairingTimelineOutput::Forgotten { hint, rescan } => {
            DevicePageOutput::Forgotten { hint, rescan }
        }
        PairingTimelineOutput::ShowRadio(adapter_name) => DevicePageOutput::ShowRadio(adapter_name),
        PairingTimelineOutput::PairViaRadio(adapter_name) => {
            DevicePageOutput::PairViaRadio(adapter_name)
        }
    }
}

fn page_output_from_file_transfer(
    file_transfer_output: FileTransferGroupOutput,
) -> DevicePageOutput {
    match file_transfer_output {
        FileTransferGroupOutput::Failed(sending_error) => DevicePageOutput::Failed(sending_error),
        FileTransferGroupOutput::AutoAcceptChosen {
            address,
            accepts_files,
        } => DevicePageOutput::AutoAcceptChosen {
            address,
            accepts_files,
        },
        FileTransferGroupOutput::TransfersStarted(transfer_handles) => {
            DevicePageOutput::TransfersStarted(transfer_handles)
        }
    }
}

#[relm4::component(pub)]
impl SimpleComponent for DevicePage {
    type Init = DevicePageInit;
    type Input = DevicePageInput;
    type Output = DevicePageOutput;

    view! {
        #[template]
        ClampedPage {
            #[template_child]
            page_content {
                gtk::Image {
                    set_pixel_size: 80,
                    #[watch]
                    set_icon_name: Some(device_icon_name(&model.snapshot)),
                },

                model.name_group.widget().clone(),

                model.timeline.widget().clone(),

                model.audio_group.widget().clone(),

                model.file_transfer_group.widget().clone(),

                model.capability_overview.widget().clone(),

                model.identity_group.widget().clone(),
            },
        }
    }

    fn init(
        page_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let name_group = DeviceNameGroup::builder()
            .launch(DeviceNameGroupInit {
                session: page_init.session.clone(),
                snapshot: page_init.snapshot.clone(),
            })
            .forward(sender.input_sender(), DevicePageInput::GroupFailed);
        let timeline = PairingTimeline::builder()
            .launch(PairingTimelineInit {
                session: page_init.session,
                snapshot: page_init.snapshot.clone(),
                connection_history: page_init.connection_history,
            })
            .forward(sender.input_sender(), DevicePageInput::TimelineReported);
        let audio_group = AudioProfileSelectorGroup::builder()
            .launch(page_init.snapshot.clone())
            .forward(sender.input_sender(), DevicePageInput::GroupFailed);
        let file_transfer_group = FileTransferGroup::builder()
            .launch(FileTransferGroupInit {
                obex: page_init.obex,
                adapter_address: page_init.adapter_address,
                accepts_files: page_init.accepts_files,
                received_files_folder: page_init.received_files_folder,
                snapshot: page_init.snapshot.clone(),
            })
            .forward(sender.input_sender(), DevicePageInput::FileTransferReported);
        let capability_overview = CapabilityOverview::builder()
            .launch(capability_sources(&page_init.snapshot))
            .detach();
        let identity_group = DeviceIdentityGroup::builder()
            .launch(page_init.snapshot.clone())
            .detach();

        let model = Self {
            snapshot: page_init.snapshot,
            name_group,
            timeline,
            audio_group,
            file_transfer_group,
            capability_overview,
            identity_group,
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>) {
        let page_output = match input {
            DevicePageInput::DeviceChanged(snapshot) => {
                self.snapshot = snapshot;
                self.forward_device_change();
                return;
            }
            DevicePageInput::ConnectionHistoryChanged(connection_history) => {
                self.timeline
                    .emit(PairingTimelineInput::ConnectionHistoryChanged(
                        connection_history,
                    ));
                return;
            }
            DevicePageInput::AutoAcceptChanged(accepts_files) => {
                self.file_transfer_group
                    .emit(FileTransferGroupInput::AutoAcceptChanged(accepts_files));
                return;
            }
            DevicePageInput::GroupFailed(group_failure) => DevicePageOutput::Failed(group_failure),
            DevicePageInput::TimelineReported(timeline_output) => {
                page_output_from_timeline(timeline_output)
            }
            DevicePageInput::FileTransferReported(file_transfer_output) => {
                page_output_from_file_transfer(file_transfer_output)
            }
        };
        let _app_gone = sender.output(page_output);
    }
}
