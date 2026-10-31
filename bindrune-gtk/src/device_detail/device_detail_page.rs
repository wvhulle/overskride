use std::path::PathBuf;

use bindrune_bluez::{
    bluez_dbus_session::BluezDbusSession, bluez_operation_error::BluetoothError,
    link_quality::disconnect_reason_history::ConnectionHistory,
    obex_file_transfer::obex_transfer_progress::TransferHandle,
    remote_device::remote_device_snapshot::DeviceSnapshot,
};
use relm4::{gtk, prelude::*};

use crate::{
    device_detail::{
        audio_profile_selector_group::{AudioProfileSelectorGroup, AudioProfileSelectorGroupInput},
        device_capability_overview::{
            CapabilitySources, DeviceCapabilityOverview, DeviceCapabilityOverviewInput,
        },
        device_identity_group::{DeviceIdentityGroup, DeviceIdentityGroupInput},
        device_name_group::{DeviceNameGroup, DeviceNameGroupInit, DeviceNameGroupInput},
        file_transfer_group::{
            FileTransferGroup, FileTransferGroupInit, FileTransferGroupInput,
            FileTransferGroupOutput, ObexAvailability,
        },
        pairing_step_timeline::pairing_timeline_widgets::{
            PairingStepTimeline, PairingStepTimelineInit, PairingStepTimelineInput,
            PairingStepTimelineOutput,
        },
    },
    shared_device_appearance::device_icon_name,
    shared_page_widget_templates::ClampedPage,
};

pub struct DeviceDetailPageInit {
    pub session: BluezDbusSession,
    pub obex: ObexAvailability,
    pub adapter_address: bindrune_bluez::Address,
    pub accepts_files: bool,
    pub received_files_folder: PathBuf,
    pub snapshot: DeviceSnapshot,
    pub connection_history: ConnectionHistory,
}

pub struct DeviceDetailPage {
    snapshot: DeviceSnapshot,
    name_group: Controller<DeviceNameGroup>,
    timeline: Controller<PairingStepTimeline>,
    audio_group: Controller<AudioProfileSelectorGroup>,
    file_transfer_group: Controller<FileTransferGroup>,
    capability_overview: Controller<DeviceCapabilityOverview>,
    identity_group: Controller<DeviceIdentityGroup>,
}

#[derive(Debug)]
pub enum DeviceDetailPageInput {
    DeviceChanged(DeviceSnapshot),
    ConnectionHistoryChanged(ConnectionHistory),
    AutoAcceptChanged(bool),
    GroupFailed(BluetoothError),
    TimelineReported(PairingStepTimelineOutput),
    FileTransferReported(FileTransferGroupOutput),
}

#[derive(Debug)]
pub enum DeviceDetailPageOutput {
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

impl DeviceDetailPage {
    pub fn shown_address(&self) -> bindrune_bluez::Address {
        self.snapshot.address()
    }

    fn forward_device_change(&self) {
        self.name_group
            .emit(DeviceNameGroupInput::DeviceChanged(self.snapshot.clone()));
        self.timeline.emit(PairingStepTimelineInput::DeviceChanged(
            self.snapshot.clone(),
        ));
        self.audio_group
            .emit(AudioProfileSelectorGroupInput::DeviceChanged(
                self.snapshot.clone(),
            ));
        self.file_transfer_group
            .emit(FileTransferGroupInput::DeviceChanged(self.snapshot.clone()));
        self.capability_overview
            .emit(DeviceCapabilityOverviewInput::SourcesChanged(
                capability_sources(&self.snapshot),
            ));
        self.identity_group
            .emit(DeviceIdentityGroupInput::DeviceChanged(
                self.snapshot.clone(),
            ));
    }
}

fn page_output_from_timeline(timeline_output: PairingStepTimelineOutput) -> DeviceDetailPageOutput {
    match timeline_output {
        PairingStepTimelineOutput::Failed(timeline_failure) => {
            DeviceDetailPageOutput::Failed(timeline_failure)
        }
        PairingStepTimelineOutput::Forgotten { hint, rescan } => {
            DeviceDetailPageOutput::Forgotten { hint, rescan }
        }
        PairingStepTimelineOutput::ShowRadio(adapter_name) => {
            DeviceDetailPageOutput::ShowRadio(adapter_name)
        }
        PairingStepTimelineOutput::PairViaRadio(adapter_name) => {
            DeviceDetailPageOutput::PairViaRadio(adapter_name)
        }
    }
}

fn page_output_from_file_transfer(
    file_transfer_output: FileTransferGroupOutput,
) -> DeviceDetailPageOutput {
    match file_transfer_output {
        FileTransferGroupOutput::Failed(sending_error) => {
            DeviceDetailPageOutput::Failed(sending_error)
        }
        FileTransferGroupOutput::AutoAcceptChosen {
            address,
            accepts_files,
        } => DeviceDetailPageOutput::AutoAcceptChosen {
            address,
            accepts_files,
        },
        FileTransferGroupOutput::TransfersStarted(transfer_handles) => {
            DeviceDetailPageOutput::TransfersStarted(transfer_handles)
        }
    }
}

#[relm4::component(pub)]
impl SimpleComponent for DeviceDetailPage {
    type Init = DeviceDetailPageInit;
    type Input = DeviceDetailPageInput;
    type Output = DeviceDetailPageOutput;

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
            .forward(sender.input_sender(), DeviceDetailPageInput::GroupFailed);
        let timeline = PairingStepTimeline::builder()
            .launch(PairingStepTimelineInit {
                session: page_init.session,
                snapshot: page_init.snapshot.clone(),
                connection_history: page_init.connection_history,
            })
            .forward(
                sender.input_sender(),
                DeviceDetailPageInput::TimelineReported,
            );
        let audio_group = AudioProfileSelectorGroup::builder()
            .launch(page_init.snapshot.clone())
            .forward(sender.input_sender(), DeviceDetailPageInput::GroupFailed);
        let file_transfer_group = FileTransferGroup::builder()
            .launch(FileTransferGroupInit {
                obex: page_init.obex,
                adapter_address: page_init.adapter_address,
                accepts_files: page_init.accepts_files,
                received_files_folder: page_init.received_files_folder,
                snapshot: page_init.snapshot.clone(),
            })
            .forward(
                sender.input_sender(),
                DeviceDetailPageInput::FileTransferReported,
            );
        let capability_overview = DeviceCapabilityOverview::builder()
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
            DeviceDetailPageInput::DeviceChanged(snapshot) => {
                self.snapshot = snapshot;
                self.forward_device_change();
                return;
            }
            DeviceDetailPageInput::ConnectionHistoryChanged(connection_history) => {
                self.timeline
                    .emit(PairingStepTimelineInput::ConnectionHistoryChanged(
                        connection_history,
                    ));
                return;
            }
            DeviceDetailPageInput::AutoAcceptChanged(accepts_files) => {
                self.file_transfer_group
                    .emit(FileTransferGroupInput::AutoAcceptChanged(accepts_files));
                return;
            }
            DeviceDetailPageInput::GroupFailed(group_failure) => {
                DeviceDetailPageOutput::Failed(group_failure)
            }
            DeviceDetailPageInput::TimelineReported(timeline_output) => {
                page_output_from_timeline(timeline_output)
            }
            DeviceDetailPageInput::FileTransferReported(file_transfer_output) => {
                page_output_from_file_transfer(file_transfer_output)
            }
        };
        let _app_gone = sender.output(page_output);
    }
}
