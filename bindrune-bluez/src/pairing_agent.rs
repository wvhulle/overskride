use std::time::Duration;

use bluer::agent::{Agent, ReqError, ReqResult};
use futures::{FutureExt, Stream};
use tokio::sync::{mpsc, oneshot};

use crate::{
    bluetooth_error::BluetoothError,
    session::{BluetoothSession, DeviceLocation},
};

pub const SERVICE_AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentPolicy {
    pub trust_after_pairing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceAuthorizationAnswer {
    Deny,
    AllowOnce,
    AlwaysAllow,
}

#[derive(Debug)]
pub struct PinCodeRequest {
    pub requester: DeviceLocation,
    reply_channel: oneshot::Sender<String>,
}

impl PinCodeRequest {
    pub fn answer(self, pin_code: String) {
        let _receiver_gone = self.reply_channel.send(pin_code);
    }
}

#[derive(Debug)]
pub struct PasskeyRequest {
    pub requester: DeviceLocation,
    reply_channel: oneshot::Sender<u32>,
}

impl PasskeyRequest {
    pub fn answer(self, passkey: u32) {
        let _receiver_gone = self.reply_channel.send(passkey);
    }
}

#[derive(Debug)]
pub struct ApprovalRequest {
    pub requester: DeviceLocation,
    reply_channel: oneshot::Sender<()>,
}

impl ApprovalRequest {
    pub fn approve(self) {
        let _receiver_gone = self.reply_channel.send(());
    }
}

#[derive(Debug)]
pub struct ServiceAuthorizationRequest {
    pub requester: DeviceLocation,
    pub service: bluer::Uuid,
    reply_channel: oneshot::Sender<ServiceAuthorizationAnswer>,
}

impl ServiceAuthorizationRequest {
    pub fn answer(self, authorization_answer: ServiceAuthorizationAnswer) {
        let _receiver_gone = self.reply_channel.send(authorization_answer);
    }
}

#[derive(Debug)]
pub enum PairingRequest {
    PinCode(PinCodeRequest),
    DisplayPinCode {
        requester: DeviceLocation,
        pin_code: String,
    },
    Passkey(PasskeyRequest),
    DisplayPasskey {
        requester: DeviceLocation,
        passkey: u32,
        entered_digits: u16,
    },
    Confirmation {
        approval: ApprovalRequest,
        passkey: u32,
    },
    Authorization(ApprovalRequest),
    ServiceAuthorization(ServiceAuthorizationRequest),
}

impl PairingRequest {
    pub fn requester(&self) -> &DeviceLocation {
        match self {
            Self::PinCode(PinCodeRequest { requester, .. })
            | Self::DisplayPinCode { requester, .. }
            | Self::Passkey(PasskeyRequest { requester, .. })
            | Self::DisplayPasskey { requester, .. }
            | Self::Confirmation {
                approval: ApprovalRequest { requester, .. },
                ..
            }
            | Self::Authorization(ApprovalRequest { requester, .. })
            | Self::ServiceAuthorization(ServiceAuthorizationRequest { requester, .. }) => {
                requester
            }
        }
    }
}

#[derive(Clone)]
struct RequestForwarder {
    session: BluetoothSession,
    request_sender: mpsc::UnboundedSender<PairingRequest>,
    policy: AgentPolicy,
}

fn requester_location(adapter_name: &str, address: bluer::Address) -> DeviceLocation {
    DeviceLocation {
        adapter_name: adapter_name.to_string(),
        address,
    }
}

impl RequestForwarder {
    fn forward(&self, pairing_request: PairingRequest) -> ReqResult<()> {
        self.request_sender
            .send(pairing_request)
            .map_err(|_| ReqError::Canceled)
    }

    async fn await_reply<ReplyType>(
        &self,
        reply_receiver: oneshot::Receiver<ReplyType>,
    ) -> ReqResult<ReplyType> {
        reply_receiver.await.map_err(|_| ReqError::Rejected)
    }

    async fn trust_device(&self, requester: &DeviceLocation) {
        let Ok(device) = self.session.device(requester) else {
            return;
        };
        let _trust_failure_is_not_fatal = device.set_trusted(true).await;
    }

    async fn trust_when_policy_asks(&self, requester: &DeviceLocation) {
        if self.policy.trust_after_pairing {
            self.trust_device(requester).await;
        }
    }

    async fn request_pin_code(self, request: bluer::agent::RequestPinCode) -> ReqResult<String> {
        let (reply_channel, reply_receiver) = oneshot::channel();
        self.forward(PairingRequest::PinCode(PinCodeRequest {
            requester: requester_location(&request.adapter, request.device),
            reply_channel,
        }))?;
        let pin_code = self.await_reply(reply_receiver).await?;
        if pin_code.is_empty() {
            return Err(ReqError::Rejected);
        }
        Ok(pin_code)
    }

    async fn request_passkey(self, request: bluer::agent::RequestPasskey) -> ReqResult<u32> {
        let (reply_channel, reply_receiver) = oneshot::channel();
        self.forward(PairingRequest::Passkey(PasskeyRequest {
            requester: requester_location(&request.adapter, request.device),
            reply_channel,
        }))?;
        self.await_reply(reply_receiver).await
    }

    fn display_pin_code(&self, request: bluer::agent::DisplayPinCode) -> ReqResult<()> {
        self.forward(PairingRequest::DisplayPinCode {
            requester: requester_location(&request.adapter, request.device),
            pin_code: request.pincode,
        })
    }

    fn display_passkey(&self, request: &bluer::agent::DisplayPasskey) -> ReqResult<()> {
        self.forward(PairingRequest::DisplayPasskey {
            requester: requester_location(&request.adapter, request.device),
            passkey: request.passkey,
            entered_digits: request.entered,
        })
    }

    async fn request_approval(
        self,
        requester: DeviceLocation,
        wrap_approval: impl FnOnce(ApprovalRequest) -> PairingRequest,
    ) -> ReqResult<()> {
        let (reply_channel, reply_receiver) = oneshot::channel();
        self.forward(wrap_approval(ApprovalRequest {
            requester: requester.clone(),
            reply_channel,
        }))?;
        self.await_reply(reply_receiver).await?;
        self.trust_when_policy_asks(&requester).await;
        Ok(())
    }

    async fn authorize_service(self, request: bluer::agent::AuthorizeService) -> ReqResult<()> {
        let requester = requester_location(&request.adapter, request.device);
        let (reply_channel, reply_receiver) = oneshot::channel();
        self.forward(PairingRequest::ServiceAuthorization(
            ServiceAuthorizationRequest {
                requester: requester.clone(),
                service: request.service,
                reply_channel,
            },
        ))?;
        let Ok(reply) = tokio::time::timeout(SERVICE_AUTHORIZATION_TIMEOUT, reply_receiver).await
        else {
            return Err(ReqError::Rejected);
        };
        match reply.map_err(|_| ReqError::Rejected)? {
            ServiceAuthorizationAnswer::Deny => Err(ReqError::Rejected),
            ServiceAuthorizationAnswer::AllowOnce => Ok(()),
            ServiceAuthorizationAnswer::AlwaysAllow => {
                self.trust_device(&requester).await;
                Ok(())
            }
        }
    }
}

fn build_agent(request_forwarder: &RequestForwarder) -> Agent {
    let pin_code_forwarder = request_forwarder.clone();
    let display_pin_forwarder = request_forwarder.clone();
    let passkey_forwarder = request_forwarder.clone();
    let display_passkey_forwarder = request_forwarder.clone();
    let confirmation_forwarder = request_forwarder.clone();
    let authorization_forwarder = request_forwarder.clone();
    let service_forwarder = request_forwarder.clone();
    Agent {
        request_default: true,
        request_pin_code: Some(Box::new(move |request| {
            pin_code_forwarder.clone().request_pin_code(request).boxed()
        })),
        display_pin_code: Some(Box::new(move |request| {
            futures::future::ready(display_pin_forwarder.display_pin_code(request)).boxed()
        })),
        request_passkey: Some(Box::new(move |request| {
            passkey_forwarder.clone().request_passkey(request).boxed()
        })),
        display_passkey: Some(Box::new(move |request| {
            futures::future::ready(display_passkey_forwarder.display_passkey(&request)).boxed()
        })),
        request_confirmation: Some(Box::new(move |request| {
            let passkey = request.passkey;
            confirmation_forwarder
                .clone()
                .request_approval(
                    requester_location(&request.adapter, request.device),
                    move |approval| PairingRequest::Confirmation { approval, passkey },
                )
                .boxed()
        })),
        request_authorization: Some(Box::new(move |request| {
            authorization_forwarder
                .clone()
                .request_approval(
                    requester_location(&request.adapter, request.device),
                    PairingRequest::Authorization,
                )
                .boxed()
        })),
        authorize_service: Some(Box::new(move |request| {
            service_forwarder.clone().authorize_service(request).boxed()
        })),
        ..Default::default()
    }
}

pub async fn pairing_requests(
    session: &BluetoothSession,
    policy: AgentPolicy,
) -> Result<impl Stream<Item = PairingRequest> + Send + 'static, BluetoothError> {
    let (request_sender, mut request_receiver) = mpsc::unbounded_channel();
    let request_forwarder = RequestForwarder {
        session: session.clone(),
        request_sender,
        policy,
    };
    let agent_handle = session
        .bluez_session()
        .register_agent(build_agent(&request_forwarder))
        .await?;
    Ok(async_stream::stream! {
        let _registered_agent = agent_handle;
        while let Some(pairing_request) = request_receiver.recv().await {
            yield pairing_request;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_requester() -> DeviceLocation {
        requester_location("hci0", bluer::Address::new([1, 2, 3, 4, 5, 6]))
    }

    #[tokio::test]
    async fn dropping_a_pin_request_rejects_it() {
        let (reply_channel, reply_receiver) = oneshot::channel::<String>();
        drop(PinCodeRequest {
            requester: test_requester(),
            reply_channel,
        });
        assert!(reply_receiver.await.is_err());
    }

    #[tokio::test]
    async fn overlapping_requests_receive_their_own_answers() {
        let (first_channel, first_receiver) = oneshot::channel();
        let (second_channel, second_receiver) = oneshot::channel();
        let first_request = PasskeyRequest {
            requester: test_requester(),
            reply_channel: first_channel,
        };
        let second_request = PasskeyRequest {
            requester: test_requester(),
            reply_channel: second_channel,
        };
        second_request.answer(222_222);
        first_request.answer(111_111);
        assert_eq!(first_receiver.await, Ok(111_111));
        assert_eq!(second_receiver.await, Ok(222_222));
    }
}
