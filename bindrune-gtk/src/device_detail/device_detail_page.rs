use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use bindrune_bluez::{
    bluetooth_adapter::device_bond_per_adapter::{RadioBond, read_radio_bonds},
    bluez_dbus_session::BluetoothSession,
    bluez_operation_error::BluetoothError,
    device_pairing::{
        pairing_step_failure::{AuthorizationFailure, ConnectionStepFailure, PairingFailure},
        pairing_step_progress::{PairingProgress, PairingStep},
    },
    link_quality::{
        disconnect_reason_history::ConnectionHistory,
        hidraw_report_timing::{ReportTimingSample, report_timing},
    },
    obex_file_transfer::obex_transfer_progress::TransferHandle,
    remote_device::{
        linux_kernel_input_devices::KernelInputBinding,
        remote_device_commands::{
            connect_device, disconnect_device, forget_device, pair_trust_and_connect,
            set_connection_policy,
        },
        remote_device_snapshot::DeviceSnapshot,
    },
};
use futures::StreamExt;
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
        pairing_step_timeline::{
            link_quality_presentation::{LinkMeasurements, ReportTimingState},
            pairing_timeline_view_model::{
                ActivePairing, StepButtonAction, TimelineAction, TimelineInputs,
            },
            pairing_timeline_widgets::{PairingTimeline, PairingTimelineInput},
        },
    },
    incoming_request_prompts::prompt_dialog_and_notification_delivery::{
        CANCEL_CHOICE, build_alert, choose_response, destructive_choice,
    },
    shared_device_appearance::device_icon_name,
    shared_page_widget_templates::ClampedPage,
};

const LINK_TICK: Duration = Duration::from_secs(1);

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
    session: BluetoothSession,
    snapshot: DeviceSnapshot,
    radio_bonds: Vec<RadioBond>,
    active_pairing: ActivePairing,
    name_group: Controller<DeviceNameGroup>,
    timeline: Controller<PairingTimeline>,
    audio_group: Controller<AudioProfileSelectorGroup>,
    file_transfer_group: Controller<FileTransferGroup>,
    capability_overview: Controller<CapabilityOverview>,
    identity_group: Controller<DeviceIdentityGroup>,
    link_measurements: LinkMeasurements,
    report_generation: u64,
}

#[derive(Debug)]
pub enum DevicePageInput {
    DeviceChanged(DeviceSnapshot),
    ConnectionHistoryChanged(ConnectionHistory),
    Timeline(TimelineAction),
    AutoAcceptChanged(bool),
    ForgetConfirmed,
    GroupFailed(BluetoothError),
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

#[derive(Debug)]
pub enum DevicePageCommand {
    RadioBondsRead(Result<Vec<RadioBond>, BluetoothError>),
    Pairing(PairingProgress),
    ConnectionChanged(Result<(), ConnectionStepFailure>),
    PolicyApplied(Result<(), AuthorizationFailure>),
    Forgotten {
        outcome: Result<(), BluetoothError>,
        rescan: bool,
    },
    LinkTick,
    ReportTimingSampled {
        generation: u64,
        sample: ReportTimingSample,
    },
}

fn capability_sources(snapshot: &DeviceSnapshot) -> CapabilitySources {
    CapabilitySources {
        advertised_services: snapshot.advertised_services(),
        made_by_apple: snapshot.made_by_apple(),
    }
}

fn pairing_mode_hint(snapshot: &DeviceSnapshot) -> &'static str {
    if snapshot.is_gamepad() {
        "hold PS + Create until the light flashes"
    } else {
        "put it in pairing mode"
    }
}

impl DevicePage {
    pub fn shown_address(&self) -> bindrune_bluez::Address {
        self.snapshot.address()
    }

    fn present_timeline(&mut self) {
        self.link_measurements.measured_at = Instant::now();
        let presentation = TimelineInputs {
            snapshot: &self.snapshot,
            radio_bonds: &self.radio_bonds,
            active_pairing: &self.active_pairing,
            link_measurements: &self.link_measurements,
        }
        .present();
        self.timeline
            .emit(PairingTimelineInput::Present(presentation));
    }

    fn follow_report_timing(&mut self, sender: &ComponentSender<Self>) {
        let hidraw_node = match &self.snapshot.kernel_input {
            KernelInputBinding::Bound(kernel_input_devices) if self.snapshot.connected => {
                kernel_input_devices.hidraw_nodes.first().cloned()
            }
            KernelInputBinding::Bound(_) | KernelInputBinding::Absent => None,
        };
        match (hidraw_node, &self.link_measurements.report_timing) {
            (Some(hidraw_node), ReportTimingState::NotApplicable) => {
                self.report_generation += 1;
                let generation = self.report_generation;
                self.link_measurements.report_timing =
                    ReportTimingState::Waiting(hidraw_node.clone());
                sender.command(move |command_output, shutdown| {
                    shutdown
                        .register(report_timing(hidraw_node).for_each(move |sample| {
                            command_output.emit(DevicePageCommand::ReportTimingSampled {
                                generation,
                                sample,
                            });
                            futures::future::ready(())
                        }))
                        .drop_on_shutdown()
                });
            }
            (None, ReportTimingState::Waiting(_) | ReportTimingState::Sampled { .. }) => {
                self.report_generation += 1;
                self.link_measurements.report_timing = ReportTimingState::NotApplicable;
            }
            (Some(_), ReportTimingState::Waiting(_) | ReportTimingState::Sampled { .. })
            | (None, ReportTimingState::NotApplicable) => {}
        }
    }

    fn record_report_timing(&mut self, generation: u64, sample: ReportTimingSample) {
        if generation != self.report_generation {
            return;
        }
        let hidraw_node = match &self.link_measurements.report_timing {
            ReportTimingState::Waiting(hidraw_node)
            | ReportTimingState::Sampled { hidraw_node, .. } => hidraw_node.clone(),
            ReportTimingState::NotApplicable => return,
        };
        self.link_measurements.report_timing = ReportTimingState::Sampled {
            hidraw_node,
            sample,
        };
        self.present_timeline();
    }

    fn read_radio_bonds(&self, sender: &ComponentSender<Self>) {
        let session = self.session.clone();
        let location = self.snapshot.location.clone();
        sender.oneshot_command(async move {
            DevicePageCommand::RadioBondsRead(read_radio_bonds(&session, &location).await)
        });
    }

    fn start_pairing(&mut self, sender: &ComponentSender<Self>) {
        self.active_pairing = ActivePairing::Running(PairingStep::Pairing);
        let session = self.session.clone();
        let location = self.snapshot.location.clone();
        sender.command(move |command_output, shutdown| {
            shutdown
                .register(async move {
                    match pair_trust_and_connect(&session, &location) {
                        Ok(pairing_progress) => {
                            pairing_progress
                                .for_each(|progress| {
                                    command_output.emit(DevicePageCommand::Pairing(progress));
                                    futures::future::ready(())
                                })
                                .await;
                        }
                        Err(lookup_failure) => command_output.emit(DevicePageCommand::Pairing(
                            PairingProgress::Failed(lookup_failure.into()),
                        )),
                    }
                })
                .drop_on_shutdown()
        });
    }

    fn change_connection(&mut self, connect_requested: bool, sender: &ComponentSender<Self>) {
        if connect_requested == self.snapshot.connected
            || matches!(self.active_pairing, ActivePairing::Running(_))
        {
            return;
        }
        if connect_requested && !self.snapshot.paired {
            self.start_pairing(sender);
            return;
        }
        self.active_pairing = ActivePairing::Running(PairingStep::Connection);
        self.present_timeline();
        let session = self.session.clone();
        let location = self.snapshot.location.clone();
        sender.oneshot_command(async move {
            let outcome = if connect_requested {
                connect_device(&session, &location).await
            } else {
                disconnect_device(&session, &location).await
            };
            DevicePageCommand::ConnectionChanged(outcome)
        });
    }

    fn forget(&self, rescan: bool, sender: &ComponentSender<Self>) {
        let session = self.session.clone();
        let location = self.snapshot.location.clone();
        sender.oneshot_command(async move {
            DevicePageCommand::Forgotten {
                outcome: forget_device(&session, &location).await,
                rescan,
            }
        });
    }

    fn confirm_forget(&self, sender: &ComponentSender<Self>) {
        let confirmation_dialog = build_alert(
            "Remove Device?",
            &format!(
                "{} has to pair again before it can connect.",
                self.snapshot.display_name()
            ),
            &[CANCEL_CHOICE, destructive_choice("remove", "Remove")],
        );
        let input_sender = sender.input_sender().clone();
        relm4::spawn_local(async move {
            if choose_response(&confirmation_dialog).await == "remove" {
                input_sender.emit(DevicePageInput::ForgetConfirmed);
            }
        });
    }

    fn handle_timeline_action(
        &mut self,
        timeline_action: TimelineAction,
        sender: &ComponentSender<Self>,
    ) {
        match timeline_action {
            TimelineAction::Step(StepButtonAction::Pair) => self.start_pairing(sender),
            TimelineAction::Step(StepButtonAction::PairAgain) => self.forget(true, sender),
            TimelineAction::Step(StepButtonAction::Connect) => self.change_connection(true, sender),
            TimelineAction::Step(StepButtonAction::Disconnect) => {
                self.change_connection(false, sender);
            }
            TimelineAction::ForgetDevice => self.confirm_forget(sender),
            TimelineAction::SetConnectionPolicy(connection_policy) => {
                let session = self.session.clone();
                let location = self.snapshot.location.clone();
                sender.oneshot_command(async move {
                    DevicePageCommand::PolicyApplied(
                        set_connection_policy(&session, &location, connection_policy).await,
                    )
                });
            }
            TimelineAction::ShowRadio(adapter_name) => {
                let _app_gone = sender.output(DevicePageOutput::ShowRadio(adapter_name));
            }
            TimelineAction::PairViaRadio(adapter_name) => {
                let _app_gone = sender.output(DevicePageOutput::PairViaRadio(adapter_name));
            }
        }
    }

    fn report_failure(sender: &ComponentSender<Self>, failure: BluetoothError) {
        let _app_gone = sender.output(DevicePageOutput::Failed(failure));
    }
}

#[relm4::component(pub)]
impl Component for DevicePage {
    type Init = DevicePageInit;
    type Input = DevicePageInput;
    type Output = DevicePageOutput;
    type CommandOutput = DevicePageCommand;

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
        let link_measurements = LinkMeasurements::unmeasured(page_init.connection_history);
        let initial_presentation = TimelineInputs {
            snapshot: &page_init.snapshot,
            radio_bonds: &[],
            active_pairing: &ActivePairing::Idle,
            link_measurements: &link_measurements,
        }
        .present();
        let timeline = PairingTimeline::builder()
            .launch(initial_presentation)
            .forward(sender.input_sender(), DevicePageInput::Timeline);

        let capability_overview = CapabilityOverview::builder()
            .launch(capability_sources(&page_init.snapshot))
            .detach();
        let name_group = DeviceNameGroup::builder()
            .launch(DeviceNameGroupInit {
                session: page_init.session.clone(),
                snapshot: page_init.snapshot.clone(),
            })
            .forward(sender.input_sender(), DevicePageInput::GroupFailed);
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
        let identity_group = DeviceIdentityGroup::builder()
            .launch(page_init.snapshot.clone())
            .detach();

        let mut model = Self {
            session: page_init.session,
            snapshot: page_init.snapshot,
            radio_bonds: Vec::new(),
            active_pairing: ActivePairing::Idle,
            name_group,
            timeline,
            audio_group,
            file_transfer_group,
            capability_overview,
            identity_group,
            link_measurements,
            report_generation: 0,
        };
        let widgets = view_output!();

        model.read_radio_bonds(&sender);
        model.follow_report_timing(&sender);
        sender.command(|command_output, shutdown| {
            shutdown
                .register(async move {
                    let mut link_ticks = tokio::time::interval(LINK_TICK);
                    loop {
                        link_ticks.tick().await;
                        command_output.emit(DevicePageCommand::LinkTick);
                    }
                })
                .drop_on_shutdown()
        });
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match input {
            DevicePageInput::DeviceChanged(snapshot) => {
                let connection_changed = snapshot.connected != self.snapshot.connected;
                self.snapshot = snapshot;
                self.name_group
                    .emit(DeviceNameGroupInput::DeviceChanged(self.snapshot.clone()));
                self.audio_group
                    .emit(AudioProfileSelectorGroupInput::DeviceChanged(
                        self.snapshot.clone(),
                    ));
                self.file_transfer_group
                    .emit(FileTransferGroupInput::DeviceChanged(self.snapshot.clone()));
                self.identity_group
                    .emit(DeviceIdentityGroupInput::DeviceChanged(
                        self.snapshot.clone(),
                    ));
                self.capability_overview
                    .emit(CapabilityOverviewInput::SourcesChanged(capability_sources(
                        &self.snapshot,
                    )));
                if connection_changed {
                    self.read_radio_bonds(&sender);
                }
                self.follow_report_timing(&sender);
                self.present_timeline();
            }
            DevicePageInput::ConnectionHistoryChanged(connection_history) => {
                self.link_measurements.connection_history = connection_history;
                self.present_timeline();
            }
            DevicePageInput::Timeline(timeline_action) => {
                self.handle_timeline_action(timeline_action, &sender);
            }
            DevicePageInput::AutoAcceptChanged(accepts_files) => {
                self.file_transfer_group
                    .emit(FileTransferGroupInput::AutoAcceptChanged(accepts_files));
            }
            DevicePageInput::ForgetConfirmed => self.forget(false, &sender),
            DevicePageInput::GroupFailed(group_failure) => {
                Self::report_failure(&sender, group_failure);
            }
            DevicePageInput::FileTransferReported(FileTransferGroupOutput::Failed(
                sending_error,
            )) => Self::report_failure(&sender, sending_error),
            DevicePageInput::FileTransferReported(FileTransferGroupOutput::AutoAcceptChosen {
                address,
                accepts_files,
            }) => {
                let _app_gone = sender.output(DevicePageOutput::AutoAcceptChosen {
                    address,
                    accepts_files,
                });
            }
            DevicePageInput::FileTransferReported(FileTransferGroupOutput::TransfersStarted(
                transfer_handles,
            )) => {
                let _app_gone = sender.output(DevicePageOutput::TransfersStarted(transfer_handles));
            }
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match command_output {
            DevicePageCommand::LinkTick => {
                if self.snapshot.connected {
                    self.present_timeline();
                }
            }
            DevicePageCommand::ReportTimingSampled { generation, sample } => {
                self.record_report_timing(generation, sample);
            }
            DevicePageCommand::RadioBondsRead(Ok(radio_bonds)) => {
                self.radio_bonds = radio_bonds;
                self.present_timeline();
            }
            DevicePageCommand::RadioBondsRead(Err(_unreadable_radios)) => {}
            DevicePageCommand::Pairing(PairingProgress::Running(running_step)) => {
                self.active_pairing = ActivePairing::Running(running_step);
                self.present_timeline();
            }
            DevicePageCommand::Pairing(PairingProgress::Failed(pairing_failure)) => {
                self.active_pairing = ActivePairing::Failed(pairing_failure);
                self.present_timeline();
            }
            DevicePageCommand::Pairing(PairingProgress::Finished) => {
                self.active_pairing = ActivePairing::Idle;
                self.read_radio_bonds(&sender);
                self.present_timeline();
            }
            DevicePageCommand::ConnectionChanged(Ok(())) => {
                self.active_pairing = ActivePairing::Idle;
                self.present_timeline();
            }
            DevicePageCommand::ConnectionChanged(Err(connection_failure)) => {
                self.active_pairing = ActivePairing::Failed(connection_failure.into());
                self.present_timeline();
            }
            DevicePageCommand::PolicyApplied(Ok(())) => {
                if matches!(
                    &self.active_pairing,
                    ActivePairing::Failed(PairingFailure::Authorization(_))
                ) {
                    self.active_pairing = ActivePairing::Idle;
                }
                self.present_timeline();
            }
            DevicePageCommand::PolicyApplied(Err(authorization_failure)) => {
                self.active_pairing = ActivePairing::Failed(authorization_failure.into());
                self.present_timeline();
            }
            DevicePageCommand::Forgotten {
                outcome: Ok(()),
                rescan,
            } => {
                let hint = if rescan {
                    format!(
                        "Forgot {}. Now {}; it reappears in the list, then press Pair.",
                        self.snapshot.display_name(),
                        pairing_mode_hint(&self.snapshot)
                    )
                } else {
                    format!(
                        "Removed {}. To pair it again, put it in pairing mode and press Scan.",
                        self.snapshot.display_name()
                    )
                };
                let _app_gone = sender.output(DevicePageOutput::Forgotten { hint, rescan });
            }
            DevicePageCommand::Forgotten {
                outcome: Err(forget_error),
                ..
            } => Self::report_failure(&sender, forget_error),
        }
    }
}
