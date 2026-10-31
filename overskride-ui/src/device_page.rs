use std::path::PathBuf;

use futures::StreamExt;
use overskride_bluetooth::audio_profiles::{
    ActiveAudioProfile, AudioProfiles, read_audio_profiles, set_audio_profile, switch_audio_off,
};
use overskride_bluetooth::bluetooth_error::BluetoothError;
use overskride_bluetooth::device_actions::{
    connect_device, disconnect_device, forget_device, pair_trust_and_connect, rename_device,
    set_connection_policy,
};
use overskride_bluetooth::device_snapshot::{DeviceSnapshot, DistanceEstimate, Manufacturer};
use overskride_bluetooth::obex_transfers::{ObexConnection, TransferHandle};
use overskride_bluetooth::pairing_steps::{PairingProgress, PairingStep};
use overskride_bluetooth::radio_bonds::{RadioBond, read_radio_bonds};
use overskride_bluetooth::session::BluetoothSession;
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::{gio, glib};
use relm4::prelude::*;

use crate::device_status::device_icon_name;
use crate::pairing_presentation::{
    ActivePairing, StepButtonAction, TimelineAction, TimelineInputs,
};
use crate::pairing_timeline::{PairingTimeline, PairingTimelineInput};
use crate::widgets::alert_dialogs::{
    CANCEL_CHOICE, build_alert, choose_response, destructive_choice,
};
use crate::widgets::page_templates::{ClampedPage, PageHeader, PropertyRow};

#[derive(Clone, Debug)]
pub enum ObexAvailability {
    Unavailable,
    Available(ObexConnection),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperationState {
    Idle,
    Busy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AudioState {
    Unavailable,
    Available(AudioProfiles),
}

pub struct DevicePageInit {
    pub session: BluetoothSession,
    pub obex: ObexAvailability,
    pub adapter_address: overskride_bluetooth::Address,
    pub snapshot: DeviceSnapshot,
}

pub struct DevicePage {
    session: BluetoothSession,
    obex: ObexAvailability,
    adapter_address: overskride_bluetooth::Address,
    snapshot: DeviceSnapshot,
    radio_bonds: Vec<RadioBond>,
    active_pairing: ActivePairing,
    rename_operation: OperationState,
    alias_changed: bool,
    audio_state: AudioState,
    audio_choices_changed: bool,
    timeline: Controller<PairingTimeline>,
}

#[derive(Debug)]
pub enum DevicePageInput {
    DeviceChanged(DeviceSnapshot),
    RenameRequested(String),
    Timeline(TimelineAction),
    ChooseFilesToSend,
    SendFiles(Vec<PathBuf>),
    AudioChoiceSelected(u32),
    ForgetConfirmed,
}

#[derive(Debug)]
pub enum DevicePageOutput {
    Notify(String),
    Forgotten { hint: String, rescan: bool },
    ShowRadio(String),
    PairViaRadio(String),
    TransfersStarted(Vec<TransferHandle>),
}

#[derive(Debug)]
pub enum DevicePageCommand {
    RadioBondsRead(Result<Vec<RadioBond>, BluetoothError>),
    Pairing(PairingProgress),
    ConnectionChanged(Result<(), BluetoothError>),
    PolicyApplied(Result<(), BluetoothError>),
    Renamed(Result<String, BluetoothError>),
    AudioProfilesRead(Result<AudioProfiles, BluetoothError>),
    AudioProfileApplied(Result<(), BluetoothError>),
    Forgotten {
        outcome: Result<(), BluetoothError>,
        rescan: bool,
    },
    FilesSending(Result<Vec<TransferHandle>, BluetoothError>),
}

fn describe_distance(distance_estimate: DistanceEstimate) -> String {
    match distance_estimate {
        DistanceEstimate::Unknown => "Unknown".to_string(),
        DistanceEstimate::Meters(distance_meters) => format!("≈ {distance_meters:.2} meters"),
    }
}

fn describe_manufacturer(manufacturer: &Manufacturer) -> &str {
    match manufacturer {
        Manufacturer::Unknown => "Unknown",
        Manufacturer::Identified(manufacturer_name) => manufacturer_name,
    }
}

const AUDIO_OFF_LABEL: &str = "Off";

fn audio_choice_list(audio_state: &AudioState) -> gtk::StringList {
    let profile_descriptions: Vec<&str> = match audio_state {
        AudioState::Unavailable => Vec::new(),
        AudioState::Available(audio_profiles) => audio_profiles
            .available_profiles
            .iter()
            .map(|audio_profile| audio_profile.description.as_str())
            .collect(),
    };
    let choice_labels: Vec<&str> = std::iter::once(AUDIO_OFF_LABEL)
        .chain(profile_descriptions)
        .collect();
    gtk::StringList::new(&choice_labels)
}

fn active_audio_position(audio_state: &AudioState) -> u32 {
    let AudioState::Available(audio_profiles) = audio_state else {
        return 0;
    };
    let ActiveAudioProfile::Selected(active_profile_name) = &audio_profiles.active_profile else {
        return 0;
    };
    audio_profiles
        .available_profiles
        .iter()
        .position(|audio_profile| &audio_profile.profile_name == active_profile_name)
        .and_then(|profile_position| u32::try_from(profile_position + 1).ok())
        .unwrap_or_default()
}

fn pairing_mode_hint(snapshot: &DeviceSnapshot) -> &'static str {
    if snapshot.is_gamepad() {
        "hold PS + Create until the light flashes"
    } else {
        "put it in pairing mode"
    }
}

impl DevicePage {
    pub fn shown_address(&self) -> overskride_bluetooth::Address {
        self.snapshot.address()
    }

    fn present_timeline(&self) {
        let presentation = TimelineInputs {
            snapshot: &self.snapshot,
            radio_bonds: &self.radio_bonds,
            active_pairing: &self.active_pairing,
        }
        .present();
        self.timeline
            .emit(PairingTimelineInput::Present(presentation));
    }

    fn read_radio_bonds(&self, sender: &ComponentSender<Self>) {
        let session = self.session.clone();
        let location = self.snapshot.location.clone();
        sender.oneshot_command(async move {
            DevicePageCommand::RadioBondsRead(read_radio_bonds(&session, &location).await)
        });
    }

    fn read_audio_profiles(&mut self, sender: &ComponentSender<Self>) {
        if !self.snapshot.connected {
            self.show_audio_state(AudioState::Unavailable);
            return;
        }
        let address = self.snapshot.address();
        sender.oneshot_command(async move {
            DevicePageCommand::AudioProfilesRead(read_audio_profiles(address).await)
        });
    }

    fn show_audio_state(&mut self, audio_state: AudioState) {
        self.audio_choices_changed = audio_state != self.audio_state;
        self.audio_state = audio_state;
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
                        Err(lookup_error) => command_output.emit(DevicePageCommand::Pairing(
                            PairingProgress::Failed(PairingStep::Pairing, lookup_error),
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

    fn notify(sender: &ComponentSender<Self>, message: String) {
        let _app_gone = sender.output(DevicePageOutput::Notify(message));
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
                #[template]
                PageHeader {
                    #[template_child]
                    header_icon {
                        #[watch]
                        set_icon_name: Some(device_icon_name(&model.snapshot)),
                    },
                    #[template_child]
                    header_title {
                        #[watch]
                        set_label: model.snapshot.display_name(),
                    },
                },

                model.timeline.widget().clone(),

                adw::PreferencesGroup {
                    set_title: "Audio and Files",
                    #[watch]
                    set_visible: model.audio_state != AudioState::Unavailable
                        || model.snapshot.offers_object_push(),

                    adw::ComboRow {
                        set_title: "Audio Profile",
                        #[watch]
                        set_visible: model.audio_state != AudioState::Unavailable,
                        #[track(model.audio_choices_changed)]
                        #[block_signal(audio_choice_handler)]
                        set_model: Some(&audio_choice_list(&model.audio_state)),
                        #[watch]
                        #[block_signal(audio_choice_handler)]
                        set_selected: active_audio_position(&model.audio_state),
                        connect_selected_notify[sender] => move |combo_row| {
                            sender.input(DevicePageInput::AudioChoiceSelected(combo_row.selected()));
                        } @audio_choice_handler,
                    },

                    adw::ActionRow {
                        set_title: "Send File To Device",
                        #[watch]
                        set_visible: model.snapshot.offers_object_push(),
                        #[watch]
                        set_sensitive: model.snapshot.connected && matches!(model.obex, ObexAvailability::Available(_)),

                        add_suffix = &gtk::Button {
                            set_label: "Choose File",
                            set_valign: gtk::Align::Center,
                            connect_clicked => DevicePageInput::ChooseFilesToSend,
                        },
                    },
                },

                adw::PreferencesGroup {
                    set_title: "Name",
                    set_description: Some("Stored on this computer for each radio, together with the bond."),

                    adw::EntryRow {
                        set_title: "Device Name",
                        set_show_apply_button: true,
                        #[track(model.alias_changed)]
                        set_text: &model.snapshot.alias,
                        #[watch]
                        set_editable: model.rename_operation == OperationState::Idle,
                        connect_apply[sender] => move |entry_row| {
                            sender.input(DevicePageInput::RenameRequested(entry_row.text().to_string()));
                        },

                        add_suffix = &gtk::Spinner {
                            set_tooltip_text: Some("Renaming…"),
                            #[watch]
                            set_spinning: model.rename_operation == OperationState::Busy,
                            #[watch]
                            set_visible: model.rename_operation == OperationState::Busy,
                        },
                    },
                },

                adw::PreferencesGroup {
                    set_title: "Details",

                    #[template]
                    PropertyRow {
                        set_title: "Address",
                        #[watch]
                        set_subtitle: &model.snapshot.address().to_string(),
                    },
                    #[template]
                    PropertyRow {
                        set_title: "Manufacturer",
                        #[watch]
                        set_subtitle: &glib::markup_escape_text(describe_manufacturer(&model.snapshot.manufacturer)),
                    },
                    #[template]
                    PropertyRow {
                        set_title: "Type",
                        #[watch]
                        set_subtitle: &glib::markup_escape_text(&model.snapshot.icon_name),
                    },
                    #[template]
                    PropertyRow {
                        set_title: "Distance",
                        set_tooltip_text: Some("Estimated from signal strength, only known while scanning"),
                        #[watch]
                        set_subtitle: &describe_distance(model.snapshot.distance_estimate()),
                    },
                    #[template]
                    PropertyRow {
                        set_title: "Services",
                        #[watch]
                        set_subtitle: &glib::markup_escape_text(&model.snapshot.service_names().join("\n")),
                    },
                },
            },
        }
    }

    fn init(
        page_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let initial_presentation = TimelineInputs {
            snapshot: &page_init.snapshot,
            radio_bonds: &[],
            active_pairing: &ActivePairing::Idle,
        }
        .present();
        let timeline = PairingTimeline::builder()
            .launch(initial_presentation)
            .forward(sender.input_sender(), DevicePageInput::Timeline);

        let mut model = Self {
            session: page_init.session,
            obex: page_init.obex,
            adapter_address: page_init.adapter_address,
            snapshot: page_init.snapshot,
            radio_bonds: Vec::new(),
            active_pairing: ActivePairing::Idle,
            rename_operation: OperationState::Idle,
            alias_changed: true,
            audio_state: AudioState::Unavailable,
            audio_choices_changed: true,
            timeline,
        };
        let widgets = view_output!();

        model.read_radio_bonds(&sender);
        model.read_audio_profiles(&sender);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        self.alias_changed = false;
        self.audio_choices_changed = false;
        match input {
            DevicePageInput::DeviceChanged(snapshot) => {
                let connection_changed = snapshot.connected != self.snapshot.connected;
                self.alias_changed = snapshot.alias != self.snapshot.alias;
                self.snapshot = snapshot;
                if connection_changed {
                    self.read_audio_profiles(&sender);
                    self.read_radio_bonds(&sender);
                }
                self.present_timeline();
            }
            DevicePageInput::RenameRequested(requested_alias) => {
                self.rename_operation = OperationState::Busy;
                let session = self.session.clone();
                let location = self.snapshot.location.clone();
                sender.oneshot_command(async move {
                    DevicePageCommand::Renamed(
                        rename_device(&session, &location, &requested_alias).await,
                    )
                });
            }
            DevicePageInput::Timeline(timeline_action) => {
                self.handle_timeline_action(timeline_action, &sender);
            }
            DevicePageInput::ChooseFilesToSend => {
                let file_dialog = gtk::FileDialog::builder()
                    .title("Choose Files to Send")
                    .modal(true)
                    .build();
                let input_sender = sender.input_sender().clone();
                relm4::spawn_local(async move {
                    let parent_window = relm4::main_application().active_window();
                    let Ok(chosen_files) = file_dialog
                        .open_multiple_future(parent_window.as_ref())
                        .await
                    else {
                        return;
                    };
                    let file_paths: Vec<_> = (0..chosen_files.n_items())
                        .filter_map(|file_position| chosen_files.item(file_position))
                        .filter_map(|file_object| file_object.downcast::<gio::File>().ok())
                        .filter_map(|chosen_file| chosen_file.path())
                        .collect();
                    if !file_paths.is_empty() {
                        input_sender.emit(DevicePageInput::SendFiles(file_paths));
                    }
                });
            }
            DevicePageInput::SendFiles(file_paths) => {
                let ObexAvailability::Available(obex_connection) = self.obex.clone() else {
                    return;
                };
                let source_address = self.adapter_address;
                let destination_address = self.snapshot.address();
                sender.oneshot_command(async move {
                    DevicePageCommand::FilesSending(
                        obex_connection
                            .send_files(source_address, destination_address, file_paths)
                            .await,
                    )
                });
            }
            DevicePageInput::AudioChoiceSelected(selected_position) => {
                let AudioState::Available(audio_profiles) = &self.audio_state else {
                    return;
                };
                if selected_position == active_audio_position(&self.audio_state) {
                    return;
                }
                let address = self.snapshot.address();
                let chosen_profile_name = usize::try_from(selected_position)
                    .ok()
                    .and_then(|position| position.checked_sub(1))
                    .and_then(|profile_position| {
                        audio_profiles.available_profiles.get(profile_position)
                    })
                    .map(|audio_profile| audio_profile.profile_name.clone());
                sender.oneshot_command(async move {
                    let outcome = match chosen_profile_name {
                        Some(profile_name) => set_audio_profile(address, profile_name).await,
                        None => switch_audio_off(address).await,
                    };
                    DevicePageCommand::AudioProfileApplied(outcome)
                });
            }
            DevicePageInput::ForgetConfirmed => self.forget(false, &sender),
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        self.alias_changed = false;
        self.audio_choices_changed = false;
        match command_output {
            DevicePageCommand::RadioBondsRead(Ok(radio_bonds)) => {
                self.radio_bonds = radio_bonds;
                self.present_timeline();
            }
            DevicePageCommand::RadioBondsRead(Err(_unreadable_radios)) => {}
            DevicePageCommand::Pairing(PairingProgress::Running(running_step)) => {
                self.active_pairing = ActivePairing::Running(running_step);
                self.present_timeline();
            }
            DevicePageCommand::Pairing(PairingProgress::Failed(failed_step, pairing_error)) => {
                let reason = pairing_error.to_string();
                Self::notify(&sender, reason.clone());
                self.active_pairing = ActivePairing::Failed {
                    step: failed_step,
                    reason,
                };
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
            DevicePageCommand::ConnectionChanged(Err(connection_error)) => {
                let reason = connection_error.to_string();
                Self::notify(&sender, reason.clone());
                self.active_pairing = ActivePairing::Failed {
                    step: PairingStep::Connection,
                    reason,
                };
                self.present_timeline();
            }
            DevicePageCommand::PolicyApplied(outcome) => {
                if let Err(policy_error) = outcome {
                    Self::notify(&sender, policy_error.to_string());
                }
                self.present_timeline();
            }
            DevicePageCommand::Renamed(outcome) => {
                self.rename_operation = OperationState::Idle;
                match outcome {
                    Ok(current_alias) => {
                        self.alias_changed = current_alias != self.snapshot.alias;
                        self.snapshot.alias = current_alias;
                    }
                    Err(rename_error) => {
                        self.alias_changed = true;
                        Self::notify(&sender, rename_error.to_string());
                    }
                }
            }
            DevicePageCommand::AudioProfilesRead(Ok(audio_profiles))
                if !audio_profiles.available_profiles.is_empty() =>
            {
                self.show_audio_state(AudioState::Available(audio_profiles));
            }
            DevicePageCommand::AudioProfilesRead(_no_audio_profiles) => {
                self.show_audio_state(AudioState::Unavailable);
            }
            DevicePageCommand::AudioProfileApplied(outcome) => {
                if let Err(audio_error) = outcome {
                    Self::notify(&sender, audio_error.to_string());
                }
                self.read_audio_profiles(&sender);
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
            } => Self::notify(&sender, forget_error.to_string()),
            DevicePageCommand::FilesSending(Ok(transfer_handles)) => {
                let _app_gone = sender.output(DevicePageOutput::TransfersStarted(transfer_handles));
            }
            DevicePageCommand::FilesSending(Err(sending_error)) => {
                Self::notify(&sender, sending_error.to_string());
            }
        }
    }
}
