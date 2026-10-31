use std::collections::BTreeMap;

use overskride_bluetooth::obex_transfers::IncomingTransfer;
use overskride_bluetooth::pairing_agent::{
    ApprovalRequest, PairingRequest, PasskeyRequest, PinCodeRequest, SERVICE_AUTHORIZATION_TIMEOUT,
    ServiceAuthorizationAnswer, ServiceAuthorizationRequest,
};
use overskride_bluetooth::service_names::describe_service;
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::prelude::*;

use crate::attention_notifications::{
    notify_about_prompt, window_needs_attention, withdraw_prompt_notification,
};
use crate::transfers_popover::{SenderAssessment, TransferDecision};
use crate::widgets::alert_dialogs::{
    AlertChoice, CANCEL_CHOICE, answer_entry, build_alert, plain_choice, suggested_choice,
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

#[derive(Clone, Debug)]
pub struct RequesterDescription {
    pub device_name: String,
    pub radio_label: String,
}

#[derive(Debug)]
pub enum PromptRequest {
    Pairing {
        pairing_request: PairingRequest,
        requester: RequesterDescription,
    },
    Transfer(SenderAssessment),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseSource {
    Dialog,
    Notification,
    Timeout,
}

#[derive(Debug)]
pub enum PromptCenterInput {
    Requested(PromptRequest),
    Responded {
        prompt_identifier: u64,
        response_id: String,
        response_source: ResponseSource,
    },
}

#[derive(Debug)]
pub enum PromptCenterOutput {
    TransferDecided {
        incoming_transfer: IncomingTransfer,
        transfer_decision: TransferDecision,
    },
}

#[derive(Debug)]
pub enum PromptCenterCommand {
    Expired(u64),
}

enum PromptAnswer {
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
    fn typed_entry(&self) -> Option<&gtk::Entry> {
        match self {
            Self::PinCode { pin_entry, .. } => Some(pin_entry),
            Self::Passkey { passkey_entry, .. } => Some(passkey_entry),
            Self::Approval { .. }
            | Self::ServiceAuthorization(_)
            | Self::Transfer(_)
            | Self::Information => None,
        }
    }

    fn answerable_from_notification(&self) -> bool {
        matches!(
            self,
            Self::Approval { .. } | Self::ServiceAuthorization(_) | Self::Transfer(_)
        )
    }
}

struct PromptDraft {
    heading: &'static str,
    body: String,
    choices: &'static [AlertChoice],
    answer: PromptAnswer,
    expiry: PromptExpiry,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptExpiry {
    Never,
    AfterServiceTimeout,
}

struct PendingPrompt {
    dialog: adw::AlertDialog,
    answer: PromptAnswer,
}

pub struct PromptCenter {
    pending_prompts: BTreeMap<u64, PendingPrompt>,
    next_prompt_identifier: u64,
}

fn draft_pairing_prompt(
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
                "{device_name} wants to pair on {radio_label}. Enter the passkey shown on the device."
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
                "Type {passkey:06} on {device_name} to pair it on {radio_label}. {entered_digits} digits entered so far."
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
                    "{device_name} wants to connect: {service_name}, via {radio_label}. Answer within {} seconds, or the request is denied.",
                    SERVICE_AUTHORIZATION_TIMEOUT.as_secs()
                ),
                choices: &SERVICE_CHOICES,
                answer: PromptAnswer::ServiceAuthorization(service_request),
                expiry: PromptExpiry::AfterServiceTimeout,
            }
        }
    }
}

fn draft_transfer_prompt(assessment: SenderAssessment) -> PromptDraft {
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

fn service_authorization_answer(response_id: &str) -> ServiceAuthorizationAnswer {
    match response_id {
        "once" => ServiceAuthorizationAnswer::AllowOnce,
        "always" => ServiceAuthorizationAnswer::AlwaysAllow,
        _ => ServiceAuthorizationAnswer::Deny,
    }
}

impl PromptCenter {
    fn present_prompt(&mut self, prompt_draft: PromptDraft, sender: &ComponentSender<Self>) {
        let prompt_identifier = self.next_prompt_identifier;
        self.next_prompt_identifier += 1;
        let dialog = build_alert(
            prompt_draft.heading,
            &prompt_draft.body,
            prompt_draft.choices,
        );
        if let Some(typed_entry) = prompt_draft.answer.typed_entry() {
            dialog.set_extra_child(Some(typed_entry));
        }
        let response_sender = sender.input_sender().clone();
        dialog.connect_response(None, move |_answered_dialog, response_id| {
            response_sender.emit(PromptCenterInput::Responded {
                prompt_identifier,
                response_id: response_id.to_string(),
                response_source: ResponseSource::Dialog,
            });
        });
        dialog.present(relm4::main_application().active_window().as_ref());
        if window_needs_attention() {
            let notification_choices: &[AlertChoice] =
                if prompt_draft.answer.answerable_from_notification() {
                    prompt_draft.choices
                } else {
                    &[]
                };
            notify_about_prompt(
                prompt_identifier,
                prompt_draft.heading,
                &prompt_draft.body,
                notification_choices,
            );
        }
        if prompt_draft.expiry == PromptExpiry::AfterServiceTimeout {
            sender.oneshot_command(async move {
                tokio::time::sleep(SERVICE_AUTHORIZATION_TIMEOUT).await;
                PromptCenterCommand::Expired(prompt_identifier)
            });
        }
        self.pending_prompts.insert(
            prompt_identifier,
            PendingPrompt {
                dialog,
                answer: prompt_draft.answer,
            },
        );
    }

    fn dismiss_prompt(
        &mut self,
        prompt_identifier: u64,
        response_source: ResponseSource,
    ) -> Option<PromptAnswer> {
        let pending_prompt = self.pending_prompts.remove(&prompt_identifier)?;
        withdraw_prompt_notification(prompt_identifier);
        if response_source != ResponseSource::Dialog {
            pending_prompt.dialog.force_close();
        }
        Some(pending_prompt.answer)
    }

    fn answer_prompt(
        prompt_answer: PromptAnswer,
        response_id: &str,
        sender: &ComponentSender<Self>,
    ) {
        match prompt_answer {
            PromptAnswer::PinCode {
                pin_code_request,
                pin_entry,
            } if response_id == "confirm" => {
                pin_code_request.answer(pin_entry.text().to_string());
            }
            PromptAnswer::Passkey {
                passkey_request,
                passkey_entry,
            } if response_id == "confirm" => {
                if let Ok(passkey) = passkey_entry.text().trim().parse() {
                    passkey_request.answer(passkey);
                }
            }
            PromptAnswer::Approval {
                approval_request,
                approving_response,
            } if response_id == approving_response => approval_request.approve(),
            PromptAnswer::ServiceAuthorization(service_request) => {
                service_request.answer(service_authorization_answer(response_id));
            }
            PromptAnswer::Transfer(incoming_transfer) => {
                let _main_view_gone = sender.output(PromptCenterOutput::TransferDecided {
                    incoming_transfer,
                    transfer_decision: TransferDecision::from_response(response_id),
                });
            }
            PromptAnswer::PinCode { .. }
            | PromptAnswer::Passkey { .. }
            | PromptAnswer::Approval { .. }
            | PromptAnswer::Information => {}
        }
    }
}

impl Component for PromptCenter {
    type Init = ();
    type Input = PromptCenterInput;
    type Output = PromptCenterOutput;
    type CommandOutput = PromptCenterCommand;
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::default()
    }

    fn init(
        (): Self::Init,
        _root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        ComponentParts {
            model: Self {
                pending_prompts: BTreeMap::new(),
                next_prompt_identifier: 1,
            },
            widgets: (),
        }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match input {
            PromptCenterInput::Requested(PromptRequest::Pairing {
                pairing_request,
                requester,
            }) => self.present_prompt(draft_pairing_prompt(pairing_request, &requester), &sender),
            PromptCenterInput::Requested(PromptRequest::Transfer(assessment)) => {
                self.present_prompt(draft_transfer_prompt(assessment), &sender);
            }
            PromptCenterInput::Responded {
                prompt_identifier,
                response_id,
                response_source,
            } => {
                if let Some(prompt_answer) = self.dismiss_prompt(prompt_identifier, response_source)
                {
                    Self::answer_prompt(prompt_answer, &response_id, &sender);
                }
            }
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match command_output {
            PromptCenterCommand::Expired(prompt_identifier) => {
                let _expired_answer =
                    self.dismiss_prompt(prompt_identifier, ResponseSource::Timeout);
            }
        }
    }
}

pub fn answer_from_notification_target(
    prompt_center_sender: &relm4::Sender<PromptCenterInput>,
    (prompt_identifier, response_id): (u64, String),
) {
    prompt_center_sender.emit(PromptCenterInput::Responded {
        prompt_identifier,
        response_id,
        response_source: ResponseSource::Notification,
    });
}
