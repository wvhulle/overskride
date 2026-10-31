use std::collections::BTreeMap;

use bindrune_bluez::{
    device_pairing::pairing_agent_requests::{PairingRequest, SERVICE_AUTHORIZATION_TIMEOUT},
    obex_file_transfer::obex_incoming_push_agent::IncomingTransfer,
};
use relm4::{adw, adw::prelude::*, gtk, prelude::*};

use crate::{
    header_transfer_popover::file_transfer_popover::{SenderAssessment, TransferDecision},
    incoming_request_prompts::{
        incoming_request_prompt_drafts::{
            PromptAnswer, PromptDraft, PromptExpiry, draft_pairing_prompt, draft_transfer_prompt,
            service_authorization_answer,
        },
        prompt_dialog_and_notification_delivery::{
            AlertChoice, build_alert, notify_about_prompt, window_needs_attention,
            withdraw_prompt_notification,
        },
    },
};

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

struct PendingPrompt {
    dialog: adw::AlertDialog,
    answer: PromptAnswer,
}

pub struct PromptCenter {
    pending_prompts: BTreeMap<u64, PendingPrompt>,
    next_prompt_identifier: u64,
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
