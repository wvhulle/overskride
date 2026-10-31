use futures::FutureExt;
use overskride_bluetooth::pairing_agent::{
    PairingRequest, SERVICE_AUTHORIZATION_TIMEOUT, ServiceAuthorizationAnswer,
};
use overskride_bluetooth::service_names::describe_service;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::glib;

use crate::attention_dialogs::choose_with_notification;
use crate::widgets::alert_dialogs::{
    CANCEL_CHOICE, answer_entry, build_alert, plain_choice, suggested_choice,
};

pub struct RequesterDescription {
    pub device_name: String,
    pub radio_label: String,
}

fn show_information(heading: &str, body: &str) {
    let information_dialog = build_alert(heading, body, &[suggested_choice("ok", "OK")]);
    relm4::spawn_local(async move {
        choose_with_notification(&information_dialog, &[]).await;
    });
}

pub fn present_pairing_request(pairing_request: PairingRequest, requester: &RequesterDescription) {
    let RequesterDescription {
        device_name,
        radio_label,
    } = requester;
    let confirm_choices = [CANCEL_CHOICE, suggested_choice("confirm", "Confirm")];
    match pairing_request {
        PairingRequest::PinCode(pin_code_request) => {
            let pin_entry = answer_entry(gtk::InputPurpose::Pin);
            let pin_dialog = build_alert(
                "PIN Code Requested",
                &format!(
                    "{device_name} wants to pair on {radio_label}. Enter the PIN code for the device."
                ),
                &confirm_choices,
            );
            pin_dialog.set_extra_child(Some(&pin_entry));
            relm4::spawn_local(async move {
                if choose_with_notification(&pin_dialog, &[]).await == "confirm" {
                    pin_code_request.answer(pin_entry.text().to_string());
                }
            });
        }
        PairingRequest::Passkey(passkey_request) => {
            let passkey_entry = answer_entry(gtk::InputPurpose::Digits);
            let passkey_dialog = build_alert(
                "Passkey Requested",
                &format!(
                    "{device_name} wants to pair on {radio_label}. Enter the passkey shown on the device."
                ),
                &confirm_choices,
            );
            passkey_dialog.set_extra_child(Some(&passkey_entry));
            relm4::spawn_local(async move {
                let confirmed = choose_with_notification(&passkey_dialog, &[]).await == "confirm";
                let entered_passkey = passkey_entry.text().trim().parse().ok();
                if let (true, Some(passkey)) = (confirmed, entered_passkey) {
                    passkey_request.answer(passkey);
                }
            });
        }
        PairingRequest::DisplayPinCode { pin_code, .. } => show_information(
            "Pairing PIN Code",
            &format!("Type {pin_code} on {device_name} to pair it on {radio_label}."),
        ),
        PairingRequest::DisplayPasskey {
            passkey,
            entered_digits,
            ..
        } => show_information(
            "Pairing Passkey",
            &format!(
                "Type {passkey:06} on {device_name} to pair it on {radio_label}. {entered_digits} digits entered so far."
            ),
        ),
        PairingRequest::Confirmation { approval, passkey } => {
            let confirmation_dialog = build_alert(
                "Confirm Pairing",
                &format!("Does {device_name} show the passkey {passkey:06}?"),
                &confirm_choices,
            );
            relm4::spawn_local(async move {
                if choose_with_notification(&confirmation_dialog, &confirm_choices).await
                    == "confirm"
                {
                    approval.approve();
                }
            });
        }
        PairingRequest::Authorization(approval) => {
            let authorization_choices = [CANCEL_CHOICE, suggested_choice("allow", "Allow")];
            let authorization_dialog = build_alert(
                "Allow Pairing?",
                &format!("{device_name} wants to pair on {radio_label}."),
                &authorization_choices,
            );
            relm4::spawn_local(async move {
                if choose_with_notification(&authorization_dialog, &authorization_choices).await
                    == "allow"
                {
                    approval.approve();
                }
            });
        }
        PairingRequest::ServiceAuthorization(service_request) => {
            let service_name = describe_service(service_request.service)
                .unwrap_or_else(|| service_request.service.to_string());
            let service_choices = [
                plain_choice("deny", "Deny"),
                plain_choice("once", "Allow Once"),
                suggested_choice("always", "Always Allow"),
            ];
            let service_dialog = build_alert(
                "Connection Request",
                &format!(
                    "{device_name} wants to connect: {service_name}, via {radio_label}. Answer within {} seconds, or the request is denied.",
                    SERVICE_AUTHORIZATION_TIMEOUT.as_secs()
                ),
                &service_choices,
            );
            relm4::spawn_local(async move {
                let response_future =
                    choose_with_notification(&service_dialog, &service_choices).fuse();
                let expiry_future = glib::timeout_future(SERVICE_AUTHORIZATION_TIMEOUT).fuse();
                futures::pin_mut!(response_future, expiry_future);
                let chosen_response = futures::select! {
                    chosen_response = response_future => chosen_response,
                    () = expiry_future => {
                        service_dialog.force_close();
                        return;
                    }
                };
                let authorization_answer = match chosen_response.as_str() {
                    "once" => ServiceAuthorizationAnswer::AllowOnce,
                    "always" => ServiceAuthorizationAnswer::AlwaysAllow,
                    _ => ServiceAuthorizationAnswer::Deny,
                };
                service_request.answer(authorization_answer);
            });
        }
    }
}
