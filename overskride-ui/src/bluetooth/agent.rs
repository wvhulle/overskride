use async_channel::Sender;
use futures::FutureExt;

use crate::message::Message;
use crate::window::OVERSKRIDE_PROPS;

const SERVICE_AUTHORIZATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

async fn trust_agent_device(session: &bluer::Session, adapter_name: &str, address: bluer::Address) {
    let device = session.adapter(adapter_name).and_then(|adapter| adapter.device(address));
    if let Ok(device) = device {
        if let Err(trust_error) = device.set_trusted(true).await {
            println!("cannot trust device {}: {:?}", address, trust_error);
        }
    }
}

async fn request_pin_code(request: bluer::agent::RequestPinCode, sender: Sender<Message>) -> bluer::agent::ReqResult<String> {
    println!("request pincode incoming");
	let address = request.device;

    sender.send(Message::RequestPinCode(request)).await.expect("cannot send message");

    OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;
    
    wait_for_dialog_exit().await;

    let final_pin_code = OVERSKRIDE_PROPS.lock().unwrap().pin_code.clone();

    println!("pin code is: {:?}", final_pin_code);
    if final_pin_code.is_empty() {
      	Err(bluer::agent::ReqError::Rejected)
    }
    else {
        sender.send(Message::SwitchActive(true, address, true)).await.expect("cannot send message");
	    Ok(final_pin_code)
    }
}

async fn display_pin_code(request: bluer::agent::DisplayPinCode, sender: Sender<Message>) -> bluer::agent::ReqResult<()> {
    println!("display pincode incoming");

    sender.send(Message::DisplayPinCode(request)).await.expect("cannot send message");

    OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;

    wait_for_dialog_exit().await;

    println!("displaying pin code finished");
    Ok(())
}

async fn request_pass_key(request: bluer::agent::RequestPasskey, sender: Sender<Message>) -> bluer::agent::ReqResult<u32> {
    println!("request passkey incoming");
	let address = request.device;

    sender.send(Message::RequestPassKey(request)).await.expect("cannot send message");

    OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;

    wait_for_dialog_exit().await;

    let pass_key = OVERSKRIDE_PROPS.lock().unwrap().pass_key.clone();
    println!("pass key is: {}", pass_key);
    if pass_key == 0 {
    	Err(bluer::agent::ReqError::Rejected)
    }
    else {
        sender.send(Message::SwitchActive(true, address, true)).await.expect("cannot send message");
    	Ok(pass_key)
    }
}   

async fn display_pass_key(request: bluer::agent::DisplayPasskey, sender: Sender<Message>) -> bluer::agent::ReqResult<()> {
    println!("display passkey incoming");
    
    sender.send(Message::DisplayPassKey(request)).await.expect("cannot send message");

    OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;

    wait_for_dialog_exit().await;

    Ok(())
}

async fn request_confirmation(request: bluer::agent::RequestConfirmation, session: bluer::Session, set_trust: bool, sender: Sender<Message>) -> bluer::agent::ReqResult<()> {
    println!("pairing confirmation incoming");
	let address = request.device;
    let adapter_name = request.adapter.clone();
    
    sender.send(Message::RequestConfirmation(request)).await.expect("cannot send message");

    OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;

    wait_for_dialog_exit().await;
    
    let confirmed = OVERSKRIDE_PROPS.lock().unwrap().confirm_authorization;

    if confirmed {
        println!("allowed pairing with device");
        if set_trust {
            trust_agent_device(&session, &adapter_name, address).await;
        }
        sender.send(Message::SwitchActive(true, address, true)).await.expect("cannot send message");
        Ok(())
    }
    else {
        println!("rejected pairing with device");
        Err(bluer::agent::ReqError::Rejected)
    }
}

async fn request_authorization(request: bluer::agent::RequestAuthorization, session: bluer::Session, set_trust: bool, sender: Sender<Message>) -> bluer::agent::ReqResult<()> {
    println!("pairing authorization incoming");
	let address = request.device;
    let adapter_name = request.adapter.clone();

    sender.send(Message::RequestAuthorization(request)).await.expect("cannot send message");

    OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;

    wait_for_dialog_exit().await;

    let confirmed = OVERSKRIDE_PROPS.lock().unwrap().confirm_authorization;
    if confirmed {
        println!("allowed pairing with device");
        if set_trust {
            trust_agent_device(&session, &adapter_name, address).await;
        }
        sender.send(Message::SwitchActive(true, address, true)).await.expect("cannot send message");
        Ok(())
    }
    else {
        println!("rejected pairing with device");
        Err(bluer::agent::ReqError::Rejected)
    }

}

async fn authorize_service(request: bluer::agent::AuthorizeService, session: bluer::Session, sender: Sender<Message>) -> bluer::agent::ReqResult<()> {
    println!("service authorization incoming");
	let address = request.device;
    let adapter_name = request.adapter.clone();

    sender.send(Message::AuthorizeService(request)).await.expect("cannot send message");

    OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;

    let answered_in_time = tokio::time::timeout(SERVICE_AUTHORIZATION_TIMEOUT, wait_for_dialog_exit()).await.is_ok();
    if !answered_in_time {
        {
            let mut overskride_properties = OVERSKRIDE_PROPS.lock().unwrap();
            overskride_properties.displaying_dialog = false;
            overskride_properties.confirm_authorization = false;
            overskride_properties.trust_after_authorization = false;
        }
        sender.send(Message::ExpireServiceAuthorization(address)).await.expect("cannot send message");
        return Err(bluer::agent::ReqError::Rejected);
    }

    let confirmed = OVERSKRIDE_PROPS.lock().unwrap().confirm_authorization;
    let trust_after_authorization = std::mem::take(&mut OVERSKRIDE_PROPS.lock().unwrap().trust_after_authorization);

    if confirmed {
        println!("allowed pairing with device");
        if trust_after_authorization {
            trust_agent_device(&session, &adapter_name, address).await;
        }
        sender.send(Message::SwitchActive(true, address, true)).await.expect("cannot send message");
        Ok(())
    }
    else {
        println!("rejected pairing with device");
        Err(bluer::agent::ReqError::Rejected)
    }
}

pub async fn register_agent(session: &bluer::Session, request_default: bool, set_trust: bool, sender_to_be_sent: Sender<Message>) -> bluer::Result<bluer::agent::AgentHandle> {
    let session1 = session.clone();
    let session2 = session.clone();
    let session3 = session.clone();

    // IDK if this is the best way, but it's a way.
    let sender1 = sender_to_be_sent.clone();
    let sender2 = sender_to_be_sent.clone();
    let sender3 = sender_to_be_sent.clone();
    let sender4 = sender_to_be_sent.clone();
    let sender5 = sender_to_be_sent.clone();
    let sender6 = sender_to_be_sent.clone();
    let sender7 = sender_to_be_sent.clone();

    let agent = bluer::agent::Agent {
        request_default,
        request_pin_code: Some(Box::new(move |req| request_pin_code(req, sender1.clone()).boxed())),
        display_pin_code: Some(Box::new(move |req| display_pin_code(req, sender2.clone()).boxed())),
        request_passkey: Some(Box::new(move |req| request_pass_key(req, sender3.clone()).boxed())),
        display_passkey: Some(Box::new(move |req| display_pass_key(req, sender4.clone()).boxed())),
        request_confirmation: Some(Box::new(move |req| {
            request_confirmation(req, session1.clone(), set_trust, sender5.clone()).boxed()
        })),
        request_authorization: Some(Box::new(move |req| {
            request_authorization(req, session2.clone(), set_trust, sender6.clone()).boxed()
        })),
        authorize_service: Some(Box::new(move |req| authorize_service(req, session3.clone(), sender7.clone()).boxed())),
        ..Default::default()
    };

    let handle = session.register_agent(agent).await.expect("unable to register agent, fuck-");

    Ok(handle)
}

pub async fn wait_for_dialog_exit() {
    while OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[tokio::main]
pub async fn register_bluetooth_agent(sender: Sender<Message>) -> bluer::Result<()> {
	let session = bluer::Session::new().await?;
	let agent = register_agent(&session, true, true, sender.clone()).await?;	
   	println!("registered agent standalone {:?}", agent);

   	loop {
   		tokio::time::sleep(std::time::Duration::from_secs(1)).await;
   	}
} 
