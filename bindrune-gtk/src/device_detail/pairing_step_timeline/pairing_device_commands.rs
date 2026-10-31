use bindrune_bluez::{
    bluetooth_adapter::device_bond_per_adapter::read_radio_bonds,
    bluez_operation_error::BluetoothError,
    device_pairing::pairing_step_progress::{PairingProgress, PairingStep},
    remote_device::{
        remote_device_commands::{
            connect_device, disconnect_device, forget_device, pair_trust_and_connect,
            set_connection_policy,
        },
        remote_device_snapshot::DeviceSnapshot,
    },
};
use futures::StreamExt;
use relm4::ComponentSender;

use crate::{
    device_detail::pairing_step_timeline::{
        pairing_timeline_view_model::{ActivePairing, StepButtonAction, TimelineAction},
        pairing_timeline_widgets::{
            PairingStepTimeline, PairingStepTimelineCommand, PairingStepTimelineInput,
            PairingStepTimelineOutput,
        },
    },
    incoming_request_prompts::prompt_dialog_and_notification_delivery::{
        CANCEL_CHOICE, build_alert, choose_response, destructive_choice,
    },
};

pub(super) fn pairing_mode_hint(snapshot: &DeviceSnapshot) -> &'static str {
    if snapshot.is_gamepad() {
        "hold PS + Create until the light flashes"
    } else {
        "put it in pairing mode"
    }
}

impl PairingStepTimeline {
    pub(super) fn read_radio_bonds(&self, sender: &ComponentSender<Self>) {
        let session = self.session.clone();
        let location = self.snapshot.location.clone();
        sender.oneshot_command(async move {
            PairingStepTimelineCommand::RadioBondsRead(read_radio_bonds(&session, &location).await)
        });
    }

    pub(super) fn start_pairing(&mut self, sender: &ComponentSender<Self>) {
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
                                    command_output
                                        .emit(PairingStepTimelineCommand::Pairing(progress));
                                    futures::future::ready(())
                                })
                                .await;
                        }
                        Err(lookup_failure) => {
                            command_output.emit(PairingStepTimelineCommand::Pairing(
                                PairingProgress::Failed(lookup_failure.into()),
                            ))
                        }
                    }
                })
                .drop_on_shutdown()
        });
    }

    pub(super) fn change_connection(
        &mut self,
        connect_requested: bool,
        sender: &ComponentSender<Self>,
    ) {
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
            PairingStepTimelineCommand::ConnectionChanged(outcome)
        });
    }

    pub(super) fn forget(&self, rescan: bool, sender: &ComponentSender<Self>) {
        let session = self.session.clone();
        let location = self.snapshot.location.clone();
        sender.oneshot_command(async move {
            PairingStepTimelineCommand::Forgotten {
                outcome: forget_device(&session, &location).await,
                rescan,
            }
        });
    }

    pub(super) fn confirm_forget(&self, sender: &ComponentSender<Self>) {
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
                input_sender.emit(PairingStepTimelineInput::ForgetConfirmed);
            }
        });
    }

    pub(super) fn handle_timeline_action(
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
                    PairingStepTimelineCommand::PolicyApplied(
                        set_connection_policy(&session, &location, connection_policy).await,
                    )
                });
            }
            TimelineAction::ShowRadio(adapter_name) => {
                let _device_page_gone =
                    sender.output(PairingStepTimelineOutput::ShowRadio(adapter_name));
            }
            TimelineAction::PairViaRadio(adapter_name) => {
                let _device_page_gone =
                    sender.output(PairingStepTimelineOutput::PairViaRadio(adapter_name));
            }
        }
    }

    pub(super) fn report_failure(sender: &ComponentSender<Self>, failure: BluetoothError) {
        let _device_page_gone = sender.output(PairingStepTimelineOutput::Failed(failure));
    }
}
