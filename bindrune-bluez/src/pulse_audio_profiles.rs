use std::{cell::RefCell, rc::Rc};

use pulseaudio::{
    callbacks::ListResult,
    context::{Context, FlagSet as ContextFlagSet, State as ContextState},
    mainloop::standard::{IterateResult, Mainloop},
    operation::State as OperationState,
    proplist::{Proplist, properties},
};

use crate::bluez_operation_error::{AudioServerFailure, BluetoothError};

const OFF_PROFILE_NAME: &str = "off";
const APPLICATION_NAME: &str = "Bindrune";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioProfile {
    pub profile_name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActiveAudioProfile {
    Off,
    Selected(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioProfiles {
    pub active_profile: ActiveAudioProfile,
    pub available_profiles: Vec<AudioProfile>,
}

struct PulseConnection {
    mainloop: Mainloop,
    context: Context,
}

impl Drop for PulseConnection {
    fn drop(&mut self) {
        self.context.disconnect();
    }
}

fn card_name_for(address: bluer::Address) -> String {
    format!("bluez_card.{}", address.to_string().replace(':', "_"))
}

fn iterate_once(mainloop: &mut Mainloop) -> Result<(), AudioServerFailure> {
    match mainloop.iterate(true) {
        IterateResult::Success(_) => Ok(()),
        IterateResult::Quit(_) | IterateResult::Err(_) => {
            Err(AudioServerFailure::ContextUnavailable)
        }
    }
}

fn connect_pulse() -> Result<PulseConnection, AudioServerFailure> {
    let mut proplist = Proplist::new().ok_or(AudioServerFailure::ContextUnavailable)?;
    proplist
        .set_str(properties::APPLICATION_NAME, APPLICATION_NAME)
        .map_err(|()| AudioServerFailure::ContextUnavailable)?;
    let mut mainloop = Mainloop::new().ok_or(AudioServerFailure::ContextUnavailable)?;
    let mut context = Context::new_with_proplist(&mainloop, APPLICATION_NAME, &proplist)
        .ok_or(AudioServerFailure::ContextUnavailable)?;
    context
        .connect(None, ContextFlagSet::NOFLAGS, None)
        .map_err(|_| AudioServerFailure::ContextUnavailable)?;
    loop {
        iterate_once(&mut mainloop)?;
        match context.get_state() {
            ContextState::Ready => return Ok(PulseConnection { mainloop, context }),
            ContextState::Failed | ContextState::Terminated => {
                return Err(AudioServerFailure::ContextUnavailable);
            }
            _ => {}
        }
    }
}

fn wait_for_operation<ClosureType: ?Sized>(
    pulse_connection: &mut PulseConnection,
    operation: &pulseaudio::operation::Operation<ClosureType>,
) -> Result<(), AudioServerFailure> {
    while operation.get_state() == OperationState::Running {
        iterate_once(&mut pulse_connection.mainloop)?;
    }
    Ok(())
}

fn query_audio_profiles(address: bluer::Address) -> Result<AudioProfiles, AudioServerFailure> {
    let mut pulse_connection = connect_pulse()?;
    let collected_profiles: Rc<RefCell<Result<AudioProfiles, AudioServerFailure>>> =
        Rc::new(RefCell::new(Err(AudioServerFailure::CardNotFound)));
    let profile_sink = Rc::clone(&collected_profiles);
    let operation = pulse_connection.context.introspect().get_card_info_by_name(
        &card_name_for(address),
        move |card_result| {
            let ListResult::Item(card_info) = card_result else {
                return;
            };
            let available_profiles = card_info
                .profiles
                .iter()
                .filter_map(|card_profile| {
                    Some(AudioProfile {
                        profile_name: card_profile.name.as_ref()?.to_string(),
                        description: card_profile.description.as_ref()?.to_string(),
                    })
                })
                .filter(|audio_profile| audio_profile.profile_name != OFF_PROFILE_NAME)
                .collect();
            let active_profile = card_info
                .active_profile
                .as_ref()
                .and_then(|active_card_profile| active_card_profile.name.as_ref())
                .map(ToString::to_string)
                .filter(|profile_name| profile_name != OFF_PROFILE_NAME)
                .map_or(ActiveAudioProfile::Off, ActiveAudioProfile::Selected);
            *profile_sink.borrow_mut() = Ok(AudioProfiles {
                active_profile,
                available_profiles,
            });
        },
    );
    wait_for_operation(&mut pulse_connection, &operation)?;
    collected_profiles.replace(Err(AudioServerFailure::CardNotFound))
}

fn apply_audio_profile(
    address: bluer::Address,
    profile_name: &str,
) -> Result<(), AudioServerFailure> {
    let mut pulse_connection = connect_pulse()?;
    let profile_accepted = Rc::new(RefCell::new(false));
    let acceptance_sink = Rc::clone(&profile_accepted);
    let operation = pulse_connection
        .context
        .introspect()
        .set_card_profile_by_name(
            &card_name_for(address),
            profile_name,
            Some(Box::new(move |accepted| {
                *acceptance_sink.borrow_mut() = accepted;
            })),
        );
    wait_for_operation(&mut pulse_connection, &operation)?;
    if *profile_accepted.borrow() {
        Ok(())
    } else {
        Err(AudioServerFailure::ProfileRejected)
    }
}

pub async fn read_audio_profiles(address: bluer::Address) -> Result<AudioProfiles, BluetoothError> {
    Ok(tokio::task::spawn_blocking(move || query_audio_profiles(address)).await??)
}

pub async fn set_audio_profile(
    address: bluer::Address,
    profile_name: String,
) -> Result<(), BluetoothError> {
    Ok(tokio::task::spawn_blocking(move || apply_audio_profile(address, &profile_name)).await??)
}

pub async fn switch_audio_off(address: bluer::Address) -> Result<(), BluetoothError> {
    set_audio_profile(address, OFF_PROFILE_NAME.to_string()).await
}
