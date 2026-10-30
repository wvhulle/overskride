use async_channel::Sender;
use bluer::{AdapterEvent, AdapterProperty, DeviceEvent, DeviceProperty};
use futures::{pin_mut, stream, stream::SelectAll, Stream, StreamExt};
use std::pin::Pin;
use std::sync::OnceLock;
use tokio_util::sync::CancellationToken;
use uuid::uuid;

use crate::{message::Message, window::{DEVICES_LUT}, agent::wait_for_dialog_exit, audio_profiles::AudioProfiles, battery::CANCEL_BATTERY_CHECK, services};
use crate::window::OVERSKRIDE_PROPS;

use crate::pairing_stages::{read_pairing_stages, send_current_pairing_stages, send_pairing_stages, ConnectionPolicy, PairingStep};
use crate::radios::list_radios;

static mut CANCELLATION_TOKEN: Option<CancellationToken> = None;

const DEVICE_SCAN_DURATION: std::time::Duration = std::time::Duration::from_secs(60);

type DeviceScanChannel = (async_channel::Sender<bool>, async_channel::Receiver<bool>);
type DiscoveryEventStream = Pin<Box<dyn Stream<Item = AdapterEvent> + Send>>;

fn device_scan_channel() -> &'static DeviceScanChannel {
    static DEVICE_SCAN_REQUESTS: OnceLock<DeviceScanChannel> = OnceLock::new();
    DEVICE_SCAN_REQUESTS.get_or_init(async_channel::unbounded)
}

pub fn request_device_scan(scan_enabled: bool) {
    let _ = device_scan_channel().0.try_send(scan_enabled);
}

async fn next_discovery_event(discovery_events: &mut Option<DiscoveryEventStream>) -> Option<AdapterEvent> {
    match discovery_events {
        Some(discovery_stream) => discovery_stream.next().await,
        None => std::future::pending().await,
    }
}

async fn pair_trust_and_connect(device: &bluer::Device, sender: &Sender<Message>) -> bluer::Result<()> {
    let stages_before_pairing = read_pairing_stages(device).await?;
    send_pairing_stages(device, stages_before_pairing.clone().with_running_step(PairingStep::Pairing), sender).await;

    if let Err(pairing_error) = device.pair().await {
        let stages_after_failure = read_pairing_stages(device).await.unwrap_or(stages_before_pairing);
        send_pairing_stages(device, stages_after_failure.with_failed_step(PairingStep::Pairing, pairing_error.message.clone()), sender).await;
        return Err(pairing_error);
    }
    request_device_scan(false);

    let stages_after_pairing = read_pairing_stages(device).await?;
    if !stages_after_pairing.bonded {
        let missing_bond_reason = "Pairing finished, but BlueZ did not store a link key. Put the device in pairing mode and press Pair Again.".to_string();
        send_pairing_stages(device, stages_after_pairing.with_failed_step(PairingStep::Bonding, missing_bond_reason.clone()), sender).await;
        return Err(bluer::Error { kind: bluer::ErrorKind::Failed, message: missing_bond_reason });
    }

    send_pairing_stages(device, stages_after_pairing.with_running_step(PairingStep::Authorization), sender).await;
    device.set_trusted(true).await?;

    send_pairing_stages(device, read_pairing_stages(device).await?.with_running_step(PairingStep::Connection), sender).await;
    if device.connect().await.is_err() {
        if let Err(connection_error) = device.connect().await {
            send_pairing_stages(device, read_pairing_stages(device).await?.with_failed_step(PairingStep::Connection, connection_error.message.clone()), sender).await;
            return Err(connection_error);
        }
    }

    Ok(())
}

/// Set the associated with `address` device's state, between connected and not
/// connected depending on what was already the case.
/// A little funky and needs fixing but works for now.
pub async fn set_device_active(address: bluer::Address, sender: Sender<Message>, adapter_name: String) -> bluer::Result<()> {
    let address_string = address.clone().to_string();
    let adapter_string = adapter_name.clone();

    let adapter = bluer::Session::new().await?.adapter(adapter_name.as_str())?;
	let device = adapter.device(address)?;

	sender.send(Message::SwitchActiveSpinner(true)).await.expect("cannot set spinner to show.");

    let state = device.is_connected().await?;

    if state {
        device.disconnect().await?;
    }
    else if !device.is_paired().await? {
        pair_trust_and_connect(&device, &sender).await?;
   	}
   	else {
        device.connect().await?;
   	}

       let updated_state = device.is_connected().await?;


    println!("set state {} for device {}\n", updated_state, device.address());
    send_current_pairing_stages(&device, &sender).await?;
	sender.send(Message::SwitchActiveSpinner(false)).await.expect("cannot set spinner to show.");
    sender.send(Message::SwitchActive(updated_state, address, true)).await.expect("cannot send message");
	sender.send(Message::InvalidateSort()).await.expect("cannot set device name.");

	// sender.send(Message::SwitchActiveSpinner(false)).expect("cannot set spinner to show.");
    // connected_switch_row.set_active(!connected_switch_row.active());

    let sender_clone = sender.clone();
    std::thread::spawn(move || {
        let clone = sender_clone.clone();
        unsafe {
            CANCEL_BATTERY_CHECK = true;
        }
        crate::battery::get_battery_for_device(address_string, adapter_string, clone);
    });

    sender.send(Message::SwitchAudioProfileExpanded(false)).await.expect("cannot send message");
    sender.send(Message::SwitchAudioProfilesList(false)).await.expect("cannot send message");

    if let Ok(profiles) = AudioProfiles::new(address.to_string()) {
        let active = profiles.active_profile;
        let profiles_map = profiles.profiles;

        if !profiles_map.is_empty() {
            sender.send(Message::PopulateAudioProfilesList(profiles_map)).await.expect("cannot send message");
            sender.send(Message::SwitchAudioProfilesList(true)).await.expect("cannot send message");
            sender.send(Message::SetActiveAudioProfile(active)).await.expect("cannot send message");
        }
        else {
            sender.send(Message::SwitchAudioProfilesList(false)).await.expect("cannot send message");
            sender.send(Message::SwitchAudioProfileExpanded(false)).await.expect("cannot send message");
        }
    }
    else {
        sender.send(Message::SwitchAudioProfilesList(false)).await.expect("cannot send message");
        sender.send(Message::SwitchAudioProfileExpanded(false)).await.expect("cannot send message");
    }

	if let Ok(()) = has_service(uuid!("00001105-0000-1000-8000-00805f9b34fb"), device).await {
    	sender.send(Message::SwitchHasObexService(true)).await.expect("cannot send message");
        sender.send(Message::SwitchSendFileActive(updated_state)).await.expect("cannot send message");
    }
    else {
      	sender.send(Message::SwitchHasObexService(false)).await.expect("cannot send message");
        sender.send(Message::SwitchSendFileActive(false)).await.expect("cannot send message");
    }



    Ok(())
}

pub async fn set_connection_policy(address: bluer::Address, connection_policy: ConnectionPolicy, sender: Sender<Message>, adapter_name: String) -> bluer::Result<()> {
    let adapter = bluer::Session::new().await?.adapter(adapter_name.as_str())?;
    let device = adapter.device(address)?;

    let policy_result = match connection_policy {
        ConnectionPolicy::Blocked => device.set_blocked(true).await,
        ConnectionPolicy::AskEachTime => {
            device.set_blocked(false).await?;
            device.set_trusted(false).await
        }
        ConnectionPolicy::Trusted if !read_pairing_stages(&device).await?.bonded => Err(bluer::Error {
            kind: bluer::ErrorKind::Failed,
            message: "Trust can't be saved: this device is paired but not bonded (no stored link key). Use Pair Again.".to_string(),
        }),
        ConnectionPolicy::Trusted => {
            device.set_blocked(false).await?;
            device.set_trusted(true).await
        }
    };

    send_current_pairing_stages(&device, &sender).await?;
    policy_result
}

pub async fn pair_device_again(address: bluer::Address, sender: Sender<Message>, adapter_name: String) -> bluer::Result<()> {
    let adapter = bluer::Session::new().await?.adapter(adapter_name.as_str())?;
    let device = adapter.device(address)?;

    let device_name = device.alias().await?;
    let is_gamepad = read_pairing_stages(&device).await?.is_gamepad;

    adapter.remove_device(address).await?;
    unsafe {
        let mut devices_lut = DEVICES_LUT.clone().unwrap();
        devices_lut.remove(&address);
        DEVICES_LUT = Some(devices_lut);
    }
    sender.send(Message::RemoveDevice(device_name.clone(), address)).await.expect("can't send message");
    sender.send(Message::UpdateListBoxImage()).await.expect("can't send message");

    request_device_scan(true);

    let pairing_mode_hint = if is_gamepad {
        "hold PS + Create until the light flashes"
    } else {
        "put it in pairing mode"
    };
    let pair_again_hint = format!("Forgot {}. Now {}; it reappears in the list, then press Pair.", device_name, pairing_mode_hint);
    sender.send(Message::PopupError(pair_again_hint, adw::ToastPriority::High)).await.expect("can't send message");

    Ok(())
}

/// Sets the currently selected device's name, updating the entry and listboxrow accordingly.
pub async fn set_device_name(address: bluer::Address, name: String, sender: Sender<Message>, adapter_name: String) -> bluer::Result<()> {
	let adapter = bluer::Session::new().await?.adapter(adapter_name.as_str())?;
	let device = adapter.device(address)?;
    let mut lut = unsafe {
    	DEVICES_LUT.clone().unwrap()
    };

    let set_name = name.trim().to_string();

    for key in lut.keys() {
		if let Some(pair) = lut.get_key_value(key) {
			if pair.1.trim() == set_name && pair.0 != &address {
                sender.send(Message::SetNameValid(false)).await.expect("cannot send message");
				return Err(bluer::Error { kind: bluer::ErrorKind::AlreadyExists, message: "device-name-exists".to_string() });
			}
		}
	}


    device.set_alias(set_name).await?;
    let current_alias = device.alias().await?;

    unsafe {
        lut.remove(&address);
        lut.insert(address, current_alias.clone());
        DEVICES_LUT = Some(lut);
    }

	sender.send(Message::SwitchName(current_alias, None, address)).await.expect("cannot set device name.");
 	sender.send(Message::SetNameValid(true)).await.expect("cannot send message");

	tokio::time::sleep(std::time::Duration::from_millis(500)).await;
	sender.send(Message::InvalidateSort()).await.expect("cannot set device name.");
    Ok(())
}

/// Gets the device associates with `address`, and then retrieves the properties of that device.
pub async fn get_device_properties(address: bluer::Address, sender: Sender<Message>, adapter_name: String) -> bluer::Result<()> {
    let adapter_string = adapter_name.clone();
    let address_string = address.clone().to_string();

    let adapter = bluer::Session::new().await?.adapter(&adapter_name)?;
	let device = adapter.device(address)?;

    let is_active = device.is_connected().await?;
    let alias = device.alias().await?;
    let icon_name = match device.icon().await? {
        Some(icon) => {
            icon
        },
        None => {
            "image-missing-symbolic".to_string()
        },
    };

    sender.send(Message::SwitchPage(Some(alias), Some(icon_name))).await.expect("cannot set device alias and icon in page.");
    sender.send(Message::SwitchActive(is_active, address, true)).await.expect("cannot set device active in page.");
    
        send_current_pairing_stages(&device, &sender).await?;
    if let Ok((_, device_address, manufacturer, device_type, distance, services_list)) = get_more_info(address, adapter_name.clone()).await {
        sender.send(Message::SwitchDeviceDetails(address, device_address, manufacturer, device_type, distance, services_list)).await.expect("cannot send message");
    }
   	sender.send(Message::SetNameValid(true)).await.expect("cannot send message");
    sender.send(Message::SwitchAudioProfileExpanded(false)).await.expect("cannot send message");
    sender.send(Message::SwitchAudioProfilesList(false)).await.expect("cannot send message");

    let sender_clone = sender.clone();
    std::thread::spawn(move || {
        let clone = sender_clone.clone();
        unsafe {
            CANCEL_BATTERY_CHECK = true;
        }
        crate::battery::get_battery_for_device(address_string, adapter_string, clone);
    });


    if let Ok(profiles) = AudioProfiles::new(address.to_string()) {
        let active = profiles.active_profile;
        let profiles_map = profiles.profiles;

        if !profiles_map.is_empty() {
            sender.send(Message::PopulateAudioProfilesList(profiles_map)).await.expect("cannot send message");
            sender.send(Message::SwitchAudioProfilesList(true)).await.expect("cannot send message");
            sender.send(Message::SetActiveAudioProfile(active)).await.expect("cannot send message");
        }
        else {
            sender.send(Message::SwitchAudioProfilesList(false)).await.expect("cannot send message");
            sender.send(Message::SwitchAudioProfileExpanded(false)).await.expect("cannot send message");
        }
    }
    else {
        sender.send(Message::SwitchAudioProfilesList(false)).await.expect("cannot send message");
        sender.send(Message::SwitchAudioProfileExpanded(false)).await.expect("cannot send message");
    }

	if let Ok(()) = has_service(uuid!("00001105-0000-1000-8000-00805f9b34fb"), device).await {
        sender.send(Message::SwitchHasObexService(true)).await.expect("cannot send message");
        sender.send(Message::SwitchSendFileActive(is_active)).await.expect("cannot send message");
    }
    else {
        sender.send(Message::SwitchHasObexService(false)).await.expect("cannot send message");
        sender.send(Message::SwitchSendFileActive(false)).await.expect("cannot send message");
    }

    // println!("the devices properties have been gotten with state: {}", is_active);

    Ok(())
}

pub async fn remove_device(address: bluer::Address, sender: Sender<Message>, adapter_name: String) -> bluer::Result<()> {
	let adapter = bluer::Session::new().await?.adapter(adapter_name.as_str())?;
	let device = adapter.device(address)?;

    let title = "Remove Device?".to_string();
    let subtitle = "Are you sure you want to remove <span font_weight='bold' color='#78aeed'>`".to_string() + &device.alias().await? + "`</span>?";
    let confirm = "Remove".to_string();


    OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;

    sender.send(Message::RequestYesNo(title, subtitle, confirm, adw::ResponseAppearance::Destructive)).await.expect("can't send message");

    wait_for_dialog_exit().await;

    let confirmed = OVERSKRIDE_PROPS.lock().unwrap().confirm_authorization;

    if confirmed {
        println!("removing device...");
        let name = device.alias().await?;
        adapter.remove_device(address).await?;
        unsafe {
            let mut devices_lut = DEVICES_LUT.clone().unwrap();
            if devices_lut.contains_key(&address) {
                devices_lut.remove(&address);
                DEVICES_LUT = Some(devices_lut);
            }
        }

        sender.send(Message::RemoveDevice(name.clone(), address)).await.expect("can't send message");
        sender.send(Message::UpdateListBoxImage()).await.expect("can't send message");

        let removed_device_hint = format!("Removed {}. To pair it again, put it in pairing mode and press Scan.", name);
        sender.send(Message::PopupError(removed_device_hint, adw::ToastPriority::Normal)).await.expect("can't send message");
    }

    Ok(())
}

pub async fn has_service(service: bluer::Uuid, device: bluer::Device) -> bluer::Result<()> {
    if device.uuids().await?.unwrap_or_default().contains(&service) {
        return Ok(());
    }

    Err(bluer::Error { kind: bluer::ErrorKind::DoesNotExist, message: "wanted service doesn't exist.".to_string()})
}

pub async fn stop_searching() {
    unsafe {
        if let Some(token) = CANCELLATION_TOKEN.clone() {
            token.cancel();
        }
    }
}

pub async fn get_devices_continuous(sender: Sender<Message>, adapter_name: String) -> bluer::Result<()> {
	let session = bluer::Session::new().await?;
	let adapter = &session.adapter(adapter_name.as_str())?;

	let filter = bluer::DiscoveryFilter {
        transport: bluer::DiscoveryTransport::Auto,
        ..Default::default()
    };
    adapter.set_discovery_filter(filter).await?;

    let known_device_events = stream::iter(
        adapter.device_addresses().await?.into_iter().map(AdapterEvent::DeviceAdded),
    );
    let device_events = known_device_events.chain(adapter.events().await?);
    pin_mut!(device_events);

    let scan_requests = device_scan_channel().1.clone();
    let mut discovery_events: Option<DiscoveryEventStream> = None;
    let scan_deadline = tokio::time::sleep(DEVICE_SCAN_DURATION);
    pin_mut!(scan_deadline);
    sender.send(Message::SwitchScanning(false)).await.expect("cannot send message");
    sender.send(Message::PopulateRadios(list_radios(&session).await?)).await.expect("cannot send message");

    let mut all_change_events = SelectAll::new();

	let sender_clone = sender.clone();

    let cancellation_token = CancellationToken::new();
    unsafe {
        CANCELLATION_TOKEN = Some(cancellation_token.clone());
    }

    while adapter.is_powered().await? {
        tokio::select! {
            Ok(scan_enabled) = scan_requests.recv() => {
                if scan_enabled && discovery_events.is_none() {
                    discovery_events = Some(Box::pin(adapter.discover_devices().await?));
                    scan_deadline.as_mut().reset(tokio::time::Instant::now() + DEVICE_SCAN_DURATION);
                }
                else if !scan_enabled {
                    discovery_events = None;
                }
                sender_clone.send(Message::SwitchScanning(discovery_events.is_some())).await.expect("cannot send message");
            }
            Some(_) = next_discovery_event(&mut discovery_events) => {}
            _ = &mut scan_deadline, if discovery_events.is_some() => {
                discovery_events = None;
                sender_clone.send(Message::SwitchScanning(false)).await.expect("cannot send message");
            }
            Some(device_event) = device_events.next() => {
                match device_event {
                    AdapterEvent::DeviceAdded(addr) => {
		                if adapter.is_powered().await? {
	                        let supposed_device = adapter.device(addr);

                            let devices_lut = unsafe {
                                DEVICES_LUT.clone().unwrap()
                            };

                            if !devices_lut.contains_key(&addr) {
                                if let Ok(added_device) = supposed_device {
	                                sender.send(Message::AddRow(added_device)).await.expect("cannot send message {}");
	                                sender.send(Message::UpdateListBoxImage()).await.expect("cannot send message {}");
	                                //println!("supposedly sent");

	                                let device = adapter.device(addr)?;
	                                let change_events = device.events().await?.map(move |evt| (addr, evt));
	                                all_change_events.push(change_events);
                                }
                                else {
                                	println!("device isn't present, something went wrong.");
                                }
                            }
                            else {
                                println!("device already exists, not adding again.");
                            }
		                }
                    }
                    AdapterEvent::DeviceRemoved(addr) => {
   		                if adapter.is_powered().await? {
                            let mut devices_lut = unsafe {
                                DEVICES_LUT.clone().unwrap()
                            };

                            let device_name = if devices_lut.contains_key(&addr) {
                                let lut = devices_lut.get(&addr).unwrap().clone();
                                unsafe {
                                    devices_lut.remove(&addr);
                                    DEVICES_LUT = Some(devices_lut);
                                }

                                lut
                            }
                            else {
                                String::new()
                            };

                            sender_clone.send(Message::RemoveDevice(device_name.clone(), addr)).await.expect("cannot send message");
                            sender_clone.send(Message::UpdateListBoxImage()).await.expect("cannot send message");
                            println!("Device removed: {:?} {}\n", addr, device_name.clone());
						}
                    },
                    AdapterEvent::PropertyChanged(AdapterProperty::Powered(powered)) => {
                        tokio::time::sleep(std::time::Duration::from_secs_f32(0.5)).await;
                        sender_clone.send(Message::SwitchAdapterPowered(powered)).await.expect("cannot send message {}");
                        println!("powered switch to {}", powered);
                    },
                    AdapterEvent::PropertyChanged(AdapterProperty::Discoverable(discoverable)) => {
                        tokio::time::sleep(std::time::Duration::from_secs_f32(0.5)).await;
                        sender_clone.send(Message::SwitchAdapterDiscoverable(discoverable)).await.expect("cannot send message {}");
                        println!("discoverable switch to {}", discoverable);
                    },
                    AdapterEvent::PropertyChanged(AdapterProperty::Alias(alias)) => {
                    	tokio::time::sleep(std::time::Duration::from_secs_f32(0.5)).await;
                    	sender_clone.send(Message::SwitchAdapterName(alias.clone(), alias.clone())).await.expect("cannot send message {}");
                    },
                    event => {
                        println!("unhandled adapter event: {:?}", event);
                    }
                }
            }
            Some((addr, DeviceEvent::PropertyChanged(property))) = all_change_events.next() => {
                let current_address = OVERSKRIDE_PROPS.lock().unwrap().address;
                match property {
                    DeviceProperty::Connected(connected) => {
                        tokio::time::sleep(std::time::Duration::from_secs_f32(0.5)).await;
                        sender_clone.send(Message::SwitchActive(connected, addr, addr == current_address)).await.expect("cannot send message");
                        if let Ok(changed_device) = adapter.device(addr) {
                            let _ = send_current_pairing_stages(&changed_device, &sender_clone).await;
                        }
                    },
                    DeviceProperty::Paired(_) | DeviceProperty::Bonded(_) | DeviceProperty::Trusted(_) | DeviceProperty::Blocked(_) => {
                        if let Ok(changed_device) = adapter.device(addr) {
                            let _ = send_current_pairing_stages(&changed_device, &sender_clone).await;
                        }
                    },
                    
                    DeviceProperty::Alias(name) => {
                        let current_address = OVERSKRIDE_PROPS.lock().unwrap().address;

                        if addr == current_address {
                            tokio::time::sleep(std::time::Duration::from_secs_f32(0.01)).await;
                            sender_clone.send(Message::SwitchName(name.clone(), None, addr)).await.expect("cannot send message");
                            sender_clone.send(Message::SwitchPage(Some(name.clone()), None)).await.expect("cannot send message");
                        }
                        else {
                            let hashmap = unsafe {
                            	DEVICES_LUT.clone().unwrap()
                            };

                            let empty = String::new();
                            let old_alias = hashmap.get(&addr).unwrap_or(&empty);

                            sender_clone.send(Message::SwitchName(name.clone(), Some(old_alias.to_string()), addr)).await.expect("cannot send message");
                        }
                    },
                    DeviceProperty::Icon(icon) => {
                        let current_address = OVERSKRIDE_PROPS.lock().unwrap().address;

                       	if addr == current_address {
                            tokio::time::sleep(std::time::Duration::from_secs_f32(0.5)).await;
                            sender_clone.send(Message::SwitchPage(None, Some(icon))).await.expect("cannot send message");
                        }
                    },
                    DeviceProperty::Rssi(rssi) => {
                       	let device = unsafe {
                            DEVICES_LUT.clone().unwrap().get(&addr).unwrap_or(&"Unknown Device".to_string()).to_string()
                        };
                        sender_clone.send(Message::SwitchRssi(device, rssi as i32)).await.expect("cannot send message");
                        sender_clone.send(Message::InvalidateSort()).await.expect("cannot send message");
                    },
                    event => {
                        println!("unhandled device event: {:?}", event);
                    },
                }
            }
            _ = cancellation_token.cancelled() => {
                // println!("exited loop from refresh");
                break;
            }
            else => break
        }

        // if cancellation_token.is_cancelled() {
        //     break;
        // }
    }

    println!("exited loop");
    // drop(agent);
    if cancellation_token.is_cancelled() {
        Ok(())
    }
    else {
        Err(bluer::Error { kind: bluer::ErrorKind::Failed, message: "Stopped searching for devices".to_string() })
    }
}

pub async fn get_more_info(address: bluer::Address, adapter_name: String) -> bluer::Result<(String, String, String, String, String, Vec<String>)> {
    let session = bluer::Session::new().await?;
	let adapter = &session.adapter(&adapter_name)?;

    let device = adapter.device(address)?;

    let name = device.alias().await?;
    let device_type = device.icon().await?.unwrap_or("Unknown".to_string());
    let mut services_list = vec![];

    let distance = async {
        /* factor for if indoors outside etc., between 2 and 4 */
    	let n = 3;
    	let measured = device.tx_power().await?;
		let rssi  = device.rssi().await?;

		println!("{:?} {:?}", measured, rssi);

		if rssi.is_none() {
			return bluer::Result::from(Ok("Unknown".to_string()));
		}

		// the -59 is an average fallback case (closest to current device)
		let ratio = (measured.unwrap_or(-59) - rssi.unwrap()) as f32;

		// basically reverse the logarithmic way or calculate TX power to get the distance
		// it is absolute fuckery and I have no idea how the hell anyone would come up with this, but it works fairly well
		let dist = 10f32.powf(ratio / (10.0 * n as f32));

		Ok(format!("≈ {:.1$} meters", dist, 2))


		// needs testing but this may be more accurate????
		// var ratio = rssi*1.0/txPower;
		// if (ratio < 1.0) {
		// 	return Math.pow(ratio,10);
		// }
		// else {
		// 	var distance =	(0.89976)*Math.pow(ratio,7.7095) + 0.111;
		// 	return distance;
		// }
    }.await?;

        for uuid in device.uuids().await?.unwrap_or_default() {
        let service = services::get_name_from_service(uuid).unwrap_or("".to_string());

        if !service.is_empty() {
        	services_list.push(service);
        }
    }

    let mut manufacturer = String::from("Unknown");

    if let Some(info) = device.manufacturer_data().await? {
    	println!("{:?}", info);

		for key in info.keys() {
			manufacturer = match bluer::id::Manufacturer::try_from(*key) {
				Ok(val) => {
					val.to_string()
				},
				Err(_) => {
					"Unknown".to_string()
				}
			};
		}
    }

    Ok((name, address.to_string(), manufacturer, device_type, distance, services_list))
}
