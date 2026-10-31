use bindrune_bluez::{
    device_pairing::pairing_agent_requests::{
        ApprovalRequest, PairingRequest, PasskeyRequest, PinCodeRequest,
        SERVICE_AUTHORIZATION_TIMEOUT, ServiceAuthorizationAnswer, ServiceAuthorizationRequest,
    },
    obex_file_transfer::obex_incoming_push_agent::IncomingTransfer,
    remote_device::advertised_service_uuids_and_capabilities::describe_service,
};
use relm4::gtk;

use crate::{
    header_transfer_popover::file_transfer_popover::SenderAssessment,
    incoming_request_prompts::{
        incoming_request_prompt_queue::RequesterDescription,
        prompt_dialog_and_notification_delivery::{
            AlertChoice, CANCEL_CHOICE, answer_entry, plain_choice, suggested_choice,
        },
    },
};

const CONFIRM_CHOICES: [AlertChoice; 2] = [CANCEL_CHOICE, suggested_choice("confirm", "Confirm")];
const ALLOW_CHOICES: [AlertChoice; 2] = [CANCEL_CHOICE, suggested_choice("allow", "Allow")];
const INFORMATION_CHOICES: [AlertChoice; 1] = [suggested_choice("ok", "OK")];
const SERVICE_CHOICES: [AlertChoice; 3] = [
    plain_choice("deny", "Deny"),
    plain_choice("once", "Allow Once"),
    suggested_choice("always", "Always Allow"),
];
const BONDED_SENDER_CHOICES: [AlertChoice; 3] = [
    plain_choice("reject", "Reject"),
    plain_choice("accept", "Accept"),
    suggested_choice("always", "Always Accept"),
];
const UNBONDED_SENDER_CHOICES: [AlertChoice; 2] = [
    plain_choice("reject", "Reject"),
    suggested_choice("accept", "Accept"),
];

pub(super) enum PromptAnswer {
    PinCode {
        pin_code_request: PinCodeRequest,
        pin_entry: gtk::Entry,
    },
    Passkey {
        passkey_request: PasskeyRequest,
        passkey_entry: gtk::Entry,
    },
    Approval {
        approval_request: ApprovalRequest,
        approving_response: &'static str,
    },
    ServiceAuthorization(ServiceAuthorizationRequest),
    Transfer(IncomingTransfer),
    Information,
}

impl PromptAnswer {
    pub(super) fn typed_entry(&self) -> Option<&gtk::Entry> {
        match self {
            Self::PinCode { pin_entry, .. } => Some(pin_entry),
            Self::Passkey { passkey_entry, .. } => Some(passkey_entry),
            Self::Approval { .. }
            | Self::ServiceAuthorization(_)
            | Self::Transfer(_)
            | Self::Information => None,
        }
    }

    pub(super) fn answerable_from_notification(&self) -> bool {
        matches!(
            self,
            Self::Approval { .. } | Self::ServiceAuthorization(_) | Self::Transfer(_)
        )
    }
}

pub(super) struct PromptDraft {
    pub(super) heading: &'static str,
    pub(super) body: String,
    pub(super) choices: &'static [AlertChoice],
    pub(super) answer: PromptAnswer,
    pub(super) expiry: PromptExpiry,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PromptExpiry {
    Never,
    AfterServiceTimeout,
}

pub(super) fn draft_pairing_prompt(
    pairing_request: PairingRequest,
    requester: &RequesterDescription,
) -> PromptDraft {
    let RequesterDescription {
        device_name,
        radio_label,
    } = requester;
    match pairing_request {
        PairingRequest::PinCode(pin_code_request) => PromptDraft {
            heading: "PIN Code Requested",
            body: format!(
                "{device_name} wants to pair on {radio_label}. Enter the PIN code for the device."
            ),
            choices: &CONFIRM_CHOICES,
            answer: PromptAnswer::PinCode {
                pin_code_request,
                pin_entry: answer_entry(gtk::InputPurpose::Pin),
            },
            expiry: PromptExpiry::Never,
        },
        PairingRequest::Passkey(passkey_request) => PromptDraft {
            heading: "Passkey Requested",
            body: format!(
                "{device_name} wants to pair on {radio_label}. Enter the passkey shown on the \
                 device."
            ),
            choices: &CONFIRM_CHOICES,
            answer: PromptAnswer::Passkey {
                passkey_request,
                passkey_entry: answer_entry(gtk::InputPurpose::Digits),
            },
            expiry: PromptExpiry::Never,
        },
        PairingRequest::DisplayPinCode { pin_code, .. } => PromptDraft {
            heading: "Pairing PIN Code",
            body: format!("Type {pin_code} on {device_name} to pair it on {radio_label}."),
            choices: &INFORMATION_CHOICES,
            answer: PromptAnswer::Information,
            expiry: PromptExpiry::Never,
        },
        PairingRequest::DisplayPasskey {
            passkey,
            entered_digits,
            ..
        } => PromptDraft {
            heading: "Pairing Passkey",
            body: format!(
                "Type {passkey:06} on {device_name} to pair it on {radio_label}. {entered_digits} \
                 digits entered so far."
            ),
            choices: &INFORMATION_CHOICES,
            answer: PromptAnswer::Information,
            expiry: PromptExpiry::Never,
        },
        PairingRequest::Confirmation { approval, passkey } => PromptDraft {
            heading: "Confirm Pairing",
            body: format!("Does {device_name} show the passkey {passkey:06}?"),
            choices: &CONFIRM_CHOICES,
            answer: PromptAnswer::Approval {
                approval_request: approval,
                approving_response: "confirm",
            },
            expiry: PromptExpiry::Never,
        },
        PairingRequest::Authorization(approval) => PromptDraft {
            heading: "Allow Pairing?",
            body: format!("{device_name} wants to pair on {radio_label}."),
            choices: &ALLOW_CHOICES,
            answer: PromptAnswer::Approval {
                approval_request: approval,
                approving_response: "allow",
            },
            expiry: PromptExpiry::Never,
        },
        PairingRequest::ServiceAuthorization(service_request) => {
            let service_name = describe_service(service_request.service)
                .unwrap_or_else(|| service_request.service.to_string());
            PromptDraft {
                heading: "Connection Request",
                body: format!(
                    "{device_name} wants to connect: {service_name}, via {radio_label}. Answer \
                     within {} seconds, or the request is denied.",
                    SERVICE_AUTHORIZATION_TIMEOUT.as_secs()
                ),
                choices: &SERVICE_CHOICES,
                answer: PromptAnswer::ServiceAuthorization(service_request),
                expiry: PromptExpiry::AfterServiceTimeout,
            }
        }
    }
}

pub(super) fn draft_transfer_prompt(assessment: SenderAssessment) -> PromptDraft {
    let SenderAssessment {
        incoming_transfer,
        sender_name,
        bonded,
    } = assessment;
    PromptDraft {
        heading: "File Transfer Incoming",
        body: format!(
            "Accept {} ({}) from {sender_name}?",
            incoming_transfer.file_name,
            incoming_transfer.total_size.display().si(),
        ),
        choices: if bonded {
            &BONDED_SENDER_CHOICES
        } else {
            &UNBONDED_SENDER_CHOICES
        },
        answer: PromptAnswer::Transfer(incoming_transfer),
        expiry: PromptExpiry::Never,
    }
}

pub(super) fn service_authorization_answer(response_id: &str) -> ServiceAuthorizationAnswer {
    match response_id {
        "once" => ServiceAuthorizationAnswer::AllowOnce,
        "always" => ServiceAuthorizationAnswer::AlwaysAllow,
        _ => ServiceAuthorizationAnswer::Deny,
    }
}
