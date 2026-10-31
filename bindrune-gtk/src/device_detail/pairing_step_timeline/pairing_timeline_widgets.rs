use std::time::{Duration, Instant};

use bindrune_bluez::{
    bluetooth_adapter::device_bond_per_adapter::DeviceBondPerAdapter,
    bluez_dbus_session::BluezDbusSession,
    bluez_operation_error::BluetoothError,
    device_pairing::{
        pairing_step_failure::{AuthorizationFailure, ConnectionStepFailure, PairingFailure},
        pairing_step_progress::PairingProgress,
    },
    link_quality::{
        disconnect_reason_history::ConnectionHistory,
        hidraw_report_timing::{ReportTimingSample, report_timing},
    },
    remote_device::{
        linux_kernel_input_devices::KernelInputBinding, remote_device_snapshot::DeviceSnapshot,
    },
};
use futures::StreamExt;
use relm4::{
    Component, ComponentParts, ComponentSender, adw, adw::prelude::*, factory::FactoryVecDeque,
    gtk, gtk::glib,
};

use crate::{
    device_detail::pairing_step_timeline::{
        adapter_bond_row::AdapterBondRow,
        link_quality_presentation::{LinkMeasurements, ReportTimingState},
        pairing_device_commands::pairing_mode_hint,
        pairing_step_row::{PairingStepRow, PairingStepRowInput},
        pairing_timeline_view_model::{
            ActivePairing, TimelineAction, TimelineInputs, TimelinePresentation,
        },
    },
    icon_names,
};

const LINK_TICK: Duration = Duration::from_secs(1);

pub struct PairingStepTimelineInit {
    pub session: BluezDbusSession,
    pub snapshot: DeviceSnapshot,
    pub connection_history: ConnectionHistory,
}

pub struct PairingStepTimeline {
    pub(super) session: BluezDbusSession,
    pub(super) snapshot: DeviceSnapshot,
    radio_bonds: Vec<DeviceBondPerAdapter>,
    pub(super) active_pairing: ActivePairing,
    link_measurements: LinkMeasurements,
    report_generation: u64,
    presentation: TimelinePresentation,
    step_rows: FactoryVecDeque<PairingStepRow>,
    adapter_bond_rows: FactoryVecDeque<AdapterBondRow>,
}

#[derive(Debug)]
pub enum PairingStepTimelineInput {
    DeviceChanged(DeviceSnapshot),
    ConnectionHistoryChanged(ConnectionHistory),
    RowActionChosen(TimelineAction),
    ForgetConfirmed,
}

#[derive(Debug)]
pub enum PairingStepTimelineOutput {
    Failed(BluetoothError),
    Forgotten { hint: String, rescan: bool },
    ShowRadio(String),
    PairViaRadio(String),
}

#[derive(Debug)]
pub enum PairingStepTimelineCommand {
    RadioBondsRead(Result<Vec<DeviceBondPerAdapter>, BluetoothError>),
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

pub struct PairingStepTimelineWidgets {
    preferences_group: adw::PreferencesGroup,
    radio_expander: adw::ExpanderRow,
    radio_info_icon: gtk::Image,
}

fn render_timeline_header(
    widgets: &PairingStepTimelineWidgets,
    presentation: &TimelinePresentation,
) {
    let radios = &presentation.radios;
    widgets
        .preferences_group
        .set_description(Some(&glib::markup_escape_text(&presentation.summary)));
    widgets.radio_expander.set_visible(radios.visible);
    widgets
        .radio_expander
        .set_subtitle(&glib::markup_escape_text(&radios.shown_radio_label));
    widgets
        .radio_info_icon
        .set_visible(!radios.also_bonded_via.is_empty());
    widgets
        .radio_info_icon
        .set_tooltip_text(Some(&format!("Also bonded via {}", radios.also_bonded_via)));
}

impl PairingStepTimeline {
    fn current_presentation(&self) -> TimelinePresentation {
        TimelineInputs {
            snapshot: &self.snapshot,
            radio_bonds: &self.radio_bonds,
            active_pairing: &self.active_pairing,
            link_measurements: &self.link_measurements,
        }
        .present()
    }

    pub(super) fn present_timeline(&mut self) {
        self.link_measurements.measured_at = Instant::now();
        let presentation = self.current_presentation();
        let radios_changed = presentation.radios != self.presentation.radios;
        presentation
            .steps
            .iter()
            .take(self.step_rows.len())
            .cloned()
            .enumerate()
            .for_each(|(step_index, step_presentation)| {
                self.step_rows
                    .send(step_index, PairingStepRowInput::Present(step_presentation));
            });
        self.presentation = presentation;
        if radios_changed {
            self.replace_adapter_bond_rows();
        }
    }

    fn replace_adapter_bond_rows(&mut self) {
        let mut adapter_bond_rows = self.adapter_bond_rows.guard();
        adapter_bond_rows.clear();
        self.presentation
            .radios
            .rows
            .iter()
            .cloned()
            .for_each(|radio_row| {
                adapter_bond_rows.push_back(radio_row);
            });
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
                            command_output.emit(PairingStepTimelineCommand::ReportTimingSampled {
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
}

impl Component for PairingStepTimeline {
    type Init = PairingStepTimelineInit;
    type Input = PairingStepTimelineInput;
    type Output = PairingStepTimelineOutput;
    type CommandOutput = PairingStepTimelineCommand;
    type Root = adw::PreferencesGroup;
    type Widgets = PairingStepTimelineWidgets;

    fn init_root() -> Self::Root {
        adw::PreferencesGroup::builder().title("Pairing").build()
    }

    fn init(
        timeline_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let radio_expander = adw::ExpanderRow::builder().title("Radio").build();
        radio_expander.add_prefix(&gtk::Image::from_icon_name(icon_names::shipped::BLUETOOTH));
        let radio_info_icon = gtk::Image::builder()
            .icon_name("dialog-information-symbolic")
            .visible(false)
            .build();
        radio_expander.add_suffix(&radio_info_icon);
        root.add(&radio_expander);

        let adapter_bond_rows = FactoryVecDeque::builder()
            .launch(radio_expander.clone())
            .forward(
                sender.input_sender(),
                PairingStepTimelineInput::RowActionChosen,
            );
        let step_rows = FactoryVecDeque::builder().launch(root.clone()).forward(
            sender.input_sender(),
            PairingStepTimelineInput::RowActionChosen,
        );
        let link_measurements = LinkMeasurements::unmeasured(timeline_init.connection_history);
        let initial_presentation = TimelineInputs {
            snapshot: &timeline_init.snapshot,
            radio_bonds: &[],
            active_pairing: &ActivePairing::Idle,
            link_measurements: &link_measurements,
        }
        .present();
        let mut model = Self {
            session: timeline_init.session,
            snapshot: timeline_init.snapshot,
            radio_bonds: Vec::new(),
            active_pairing: ActivePairing::Idle,
            link_measurements,
            report_generation: 0,
            presentation: initial_presentation,
            step_rows,
            adapter_bond_rows,
        };
        {
            let mut step_rows = model.step_rows.guard();
            model
                .presentation
                .steps
                .iter()
                .cloned()
                .for_each(|step_presentation| {
                    step_rows.push_back(step_presentation);
                });
        }
        model.replace_adapter_bond_rows();
        let widgets = PairingStepTimelineWidgets {
            preferences_group: root,
            radio_expander,
            radio_info_icon,
        };
        render_timeline_header(&widgets, &model.presentation);

        model.read_radio_bonds(&sender);
        model.follow_report_timing(&sender);
        sender.command(|command_output, shutdown| {
            shutdown
                .register(async move {
                    let mut link_ticks = tokio::time::interval(LINK_TICK);
                    loop {
                        link_ticks.tick().await;
                        command_output.emit(PairingStepTimelineCommand::LinkTick);
                    }
                })
                .drop_on_shutdown()
        });
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match input {
            PairingStepTimelineInput::DeviceChanged(snapshot) => {
                let connection_changed = snapshot.connected != self.snapshot.connected;
                self.snapshot = snapshot;
                if connection_changed {
                    self.read_radio_bonds(&sender);
                }
                self.follow_report_timing(&sender);
                self.present_timeline();
            }
            PairingStepTimelineInput::ConnectionHistoryChanged(connection_history) => {
                self.link_measurements.connection_history = connection_history;
                self.present_timeline();
            }
            PairingStepTimelineInput::RowActionChosen(timeline_action) => {
                self.handle_timeline_action(timeline_action, &sender);
            }
            PairingStepTimelineInput::ForgetConfirmed => self.forget(false, &sender),
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match command_output {
            PairingStepTimelineCommand::LinkTick => {
                if self.snapshot.connected {
                    self.present_timeline();
                }
            }
            PairingStepTimelineCommand::ReportTimingSampled { generation, sample } => {
                self.record_report_timing(generation, sample);
            }
            PairingStepTimelineCommand::RadioBondsRead(Ok(radio_bonds)) => {
                self.radio_bonds = radio_bonds;
                self.present_timeline();
            }
            PairingStepTimelineCommand::RadioBondsRead(Err(_unreadable_radios)) => {}
            PairingStepTimelineCommand::Pairing(PairingProgress::Running(running_step)) => {
                self.active_pairing = ActivePairing::Running(running_step);
                self.present_timeline();
            }
            PairingStepTimelineCommand::Pairing(PairingProgress::Failed(pairing_failure)) => {
                self.active_pairing = ActivePairing::Failed(pairing_failure);
                self.present_timeline();
            }
            PairingStepTimelineCommand::Pairing(PairingProgress::Finished) => {
                self.active_pairing = ActivePairing::Idle;
                self.read_radio_bonds(&sender);
                self.present_timeline();
            }
            PairingStepTimelineCommand::ConnectionChanged(Ok(())) => {
                self.active_pairing = ActivePairing::Idle;
                self.present_timeline();
            }
            PairingStepTimelineCommand::ConnectionChanged(Err(connection_failure)) => {
                self.active_pairing = ActivePairing::Failed(connection_failure.into());
                self.present_timeline();
            }
            PairingStepTimelineCommand::PolicyApplied(Ok(())) => {
                if matches!(
                    &self.active_pairing,
                    ActivePairing::Failed(PairingFailure::Authorization(_))
                ) {
                    self.active_pairing = ActivePairing::Idle;
                }
                self.present_timeline();
            }
            PairingStepTimelineCommand::PolicyApplied(Err(authorization_failure)) => {
                self.active_pairing = ActivePairing::Failed(authorization_failure.into());
                self.present_timeline();
            }
            PairingStepTimelineCommand::Forgotten {
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
                let _device_page_gone =
                    sender.output(PairingStepTimelineOutput::Forgotten { hint, rescan });
            }
            PairingStepTimelineCommand::Forgotten {
                outcome: Err(forget_error),
                ..
            } => Self::report_failure(&sender, forget_error),
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        render_timeline_header(widgets, &self.presentation);
    }
}
