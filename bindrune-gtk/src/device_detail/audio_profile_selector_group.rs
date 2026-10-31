use bindrune_bluez::{
    bluez_operation_error::BluetoothError,
    pulse_audio_profiles::{
        ActiveAudioProfile, AudioProfiles, read_audio_profiles, set_audio_profile, switch_audio_off,
    },
    remote_device::remote_device_snapshot::DeviceSnapshot,
};
use relm4::{adw, adw::prelude::*, gtk, prelude::*};

const AUDIO_OFF_LABEL: &str = "Off";

#[derive(Clone, Debug, PartialEq, Eq)]
enum AudioState {
    Unavailable,
    Available(AudioProfiles),
}

pub struct AudioProfileSelectorGroup {
    address: bindrune_bluez::Address,
    connected: bool,
    audio_state: AudioState,
    audio_choices_changed: bool,
}

#[derive(Debug)]
pub enum AudioProfileSelectorGroupInput {
    DeviceChanged(DeviceSnapshot),
    AudioChoiceSelected(u32),
}

#[derive(Debug)]
pub enum AudioProfileSelectorGroupCommand {
    AudioProfilesRead(Result<AudioProfiles, BluetoothError>),
    AudioProfileApplied(Result<(), BluetoothError>),
}

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

impl AudioProfileSelectorGroup {
    fn read_audio_profiles(&mut self, sender: &ComponentSender<Self>) {
        if !self.connected {
            self.show_audio_state(AudioState::Unavailable);
            return;
        }
        let address = self.address;
        sender.oneshot_command(async move {
            AudioProfileSelectorGroupCommand::AudioProfilesRead(read_audio_profiles(address).await)
        });
    }

    fn show_audio_state(&mut self, audio_state: AudioState) {
        self.audio_choices_changed = audio_state != self.audio_state;
        self.audio_state = audio_state;
    }
}

#[relm4::component(pub)]
impl Component for AudioProfileSelectorGroup {
    type Init = DeviceSnapshot;
    type Input = AudioProfileSelectorGroupInput;
    type Output = BluetoothError;
    type CommandOutput = AudioProfileSelectorGroupCommand;

    view! {
        adw::PreferencesGroup {
            set_title: "Audio",
            #[watch]
            set_visible: model.audio_state != AudioState::Unavailable,

            adw::ComboRow {
                set_title: "Audio Profile",
                #[track(model.audio_choices_changed)]
                #[block_signal(audio_choice_handler)]
                set_model: Some(&audio_choice_list(&model.audio_state)),
                #[watch]
                #[block_signal(audio_choice_handler)]
                set_selected: active_audio_position(&model.audio_state),
                connect_selected_notify[sender] => move |combo_row| {
                    sender.input(AudioProfileSelectorGroupInput::AudioChoiceSelected(combo_row.selected()));
                } @audio_choice_handler,
            },
        }
    }

    fn init(
        snapshot: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut model = Self {
            address: snapshot.address(),
            connected: snapshot.connected,
            audio_state: AudioState::Unavailable,
            audio_choices_changed: true,
        };
        let widgets = view_output!();
        model.read_audio_profiles(&sender);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        self.audio_choices_changed = false;
        match input {
            AudioProfileSelectorGroupInput::DeviceChanged(snapshot) => {
                let connection_changed = snapshot.connected != self.connected;
                self.address = snapshot.address();
                self.connected = snapshot.connected;
                if connection_changed {
                    self.read_audio_profiles(&sender);
                }
            }
            AudioProfileSelectorGroupInput::AudioChoiceSelected(selected_position) => {
                let AudioState::Available(audio_profiles) = &self.audio_state else {
                    return;
                };
                if selected_position == active_audio_position(&self.audio_state) {
                    return;
                }
                let address = self.address;
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
                    AudioProfileSelectorGroupCommand::AudioProfileApplied(outcome)
                });
            }
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        self.audio_choices_changed = false;
        match command_output {
            AudioProfileSelectorGroupCommand::AudioProfilesRead(Ok(audio_profiles))
                if !audio_profiles.available_profiles.is_empty() =>
            {
                self.show_audio_state(AudioState::Available(audio_profiles));
            }
            AudioProfileSelectorGroupCommand::AudioProfilesRead(_no_audio_profiles) => {
                self.show_audio_state(AudioState::Unavailable);
            }
            AudioProfileSelectorGroupCommand::AudioProfileApplied(outcome) => {
                if let Err(audio_error) = outcome {
                    let _device_page_gone = sender.output(audio_error);
                }
                self.read_audio_profiles(&sender);
            }
        }
    }
}
