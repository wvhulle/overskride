use std::collections::HashSet;
use std::time::Duration;

use futures::StreamExt;
use overskride_bluetooth::device_actions::read_device;
use overskride_bluetooth::obex_transfers::{
    IncomingTransfer, TransferHandle, TransferProgress, TransferStatus,
};
use overskride_bluetooth::session::{BluetoothSession, DeviceLocation};
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender, FactoryVecDeque};
use relm4::gtk;
use relm4::gtk::glib;
use relm4::prelude::*;

use crate::app_preferences::AppPreferences;
use crate::device_page::ObexAvailability;
use crate::widgets::alert_dialogs::{build_alert, choose_response, plain_choice, suggested_choice};

const FINISHED_TRANSFER_LINGER: Duration = Duration::from_secs(60);
const BYTES_PER_MEGABYTE: f64 = 1_000_000.0;

pub struct TransfersInit {
    pub session: BluetoothSession,
    pub obex: ObexAvailability,
    pub preferences: AppPreferences,
    pub adapter_name: String,
}

pub struct TransfersPopover {
    session: BluetoothSession,
    preferences: AppPreferences,
    adapter_name: String,
    accepted_senders: HashSet<overskride_bluetooth::Address>,
    transfer_rows: FactoryVecDeque<TransferRow>,
}

#[derive(Debug)]
pub enum TransfersInput {
    TransfersStarted(Vec<TransferHandle>),
    AdapterSwitched(String),
    RowFinished(DynamicIndex),
    Decided {
        incoming_transfer: IncomingTransfer,
        accepted: bool,
    },
}

#[derive(Debug)]
pub enum TransfersOutput {
    Notify(String),
}

#[derive(Debug)]
pub struct SenderAssessment {
    incoming_transfer: IncomingTransfer,
    sender_name: String,
    trusted: bool,
}

#[derive(Debug)]
pub enum TransfersCommand {
    Incoming(IncomingTransfer),
    SenderAssessed(SenderAssessment),
    RemoveFinishedRow(DynamicIndex),
}

fn megabytes(byte_count: u64) -> f64 {
    let whole_kilobytes = u32::try_from(byte_count / 1000).unwrap_or(u32::MAX);
    f64::from(whole_kilobytes) * 1000.0 / BYTES_PER_MEGABYTE
}

impl TransfersPopover {
    fn accept(&mut self, incoming_transfer: IncomingTransfer, sender: &ComponentSender<Self>) {
        let sender_address = incoming_transfer.sender_address;
        let transfer_handle = incoming_transfer.accept(self.preferences.received_files_folder());
        if self.preferences.keep_accepting_after_first_file() {
            self.accepted_senders.insert(sender_address);
        }
        self.transfer_rows.guard().push_back(transfer_handle);
        let _app_gone = sender.output(TransfersOutput::Notify("Receiving a file".to_string()));
    }

    fn assess_sender(&self, incoming_transfer: IncomingTransfer, sender: &ComponentSender<Self>) {
        let session = self.session.clone();
        let location = DeviceLocation {
            adapter_name: self.adapter_name.clone(),
            address: incoming_transfer.sender_address,
        };
        sender.oneshot_command(async move {
            let sender_snapshot = read_device(&session, &location).await;
            TransfersCommand::SenderAssessed(SenderAssessment {
                sender_name: sender_snapshot.as_ref().map_or_else(
                    |_unknown| location.address.to_string(),
                    |snapshot| snapshot.display_name().to_string(),
                ),
                trusted: sender_snapshot.is_ok_and(|snapshot| snapshot.trusted),
                incoming_transfer,
            })
        });
    }

    fn ask_user(assessment: SenderAssessment, sender: &ComponentSender<Self>) {
        let question_dialog = build_alert(
            "File Transfer Incoming",
            &format!(
                "Accept {} ({:.2} MB) from {}?",
                assessment.incoming_transfer.file_name,
                megabytes(assessment.incoming_transfer.total_bytes),
                assessment.sender_name
            ),
            &[
                plain_choice("reject", "Reject"),
                suggested_choice("accept", "Accept"),
            ],
        );
        let input_sender = sender.input_sender().clone();
        let incoming_transfer = assessment.incoming_transfer;
        relm4::spawn_local(async move {
            let accepted = choose_response(&question_dialog).await == "accept";
            input_sender.emit(TransfersInput::Decided {
                incoming_transfer,
                accepted,
            });
        });
    }
}

#[relm4::component(pub)]
impl Component for TransfersPopover {
    type Init = TransfersInit;
    type Input = TransfersInput;
    type Output = TransfersOutput;
    type CommandOutput = TransfersCommand;

    view! {
        gtk::MenuButton {
            set_icon_name: "folder-download-symbolic",
            set_tooltip_text: Some("File Transfers"),
            add_css_class: "flat",

            #[wrap(Some)]
            set_popover = &gtk::Popover {
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 6,
                    set_width_request: 320,

                    gtk::Label {
                        set_label: "No file transfers yet",
                        add_css_class: "dim-label",
                        set_margin_all: 12,
                        #[watch]
                        set_visible: model.transfer_rows.is_empty(),
                    },

                    #[local_ref]
                    transfer_list -> gtk::ListBox {
                        add_css_class: "boxed-list",
                        set_selection_mode: gtk::SelectionMode::None,
                        #[watch]
                        set_visible: !model.transfer_rows.is_empty(),
                    },
                },
            },
        }
    }

    fn init(
        transfers_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let transfer_rows = FactoryVecDeque::builder()
            .launch(gtk::ListBox::default())
            .forward(sender.input_sender(), TransfersInput::RowFinished);
        let model = Self {
            session: transfers_init.session,
            preferences: transfers_init.preferences,
            adapter_name: transfers_init.adapter_name,
            accepted_senders: HashSet::new(),
            transfer_rows,
        };
        let transfer_list = model.transfer_rows.widget();
        let widgets = view_output!();

        if let ObexAvailability::Available(obex_connection) = transfers_init.obex {
            sender.command(move |command_output, shutdown| {
                shutdown
                    .register(async move {
                        let Ok(incoming_transfers) = obex_connection.incoming_transfers().await
                        else {
                            return;
                        };
                        incoming_transfers
                            .for_each(|incoming_transfer| {
                                command_output.emit(TransfersCommand::Incoming(incoming_transfer));
                                futures::future::ready(())
                            })
                            .await;
                    })
                    .drop_on_shutdown()
            });
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match input {
            TransfersInput::TransfersStarted(transfer_handles) => {
                let mut transfer_rows = self.transfer_rows.guard();
                transfer_handles.into_iter().for_each(|transfer_handle| {
                    transfer_rows.push_back(transfer_handle);
                });
            }
            TransfersInput::AdapterSwitched(adapter_name) => self.adapter_name = adapter_name,
            TransfersInput::Decided {
                incoming_transfer,
                accepted: true,
            } => self.accept(incoming_transfer, &sender),
            TransfersInput::Decided {
                incoming_transfer,
                accepted: false,
            } => incoming_transfer.reject(),
            TransfersInput::RowFinished(finished_index) => {
                sender.oneshot_command(async move {
                    tokio::time::sleep(FINISHED_TRANSFER_LINGER).await;
                    TransfersCommand::RemoveFinishedRow(finished_index)
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
        match command_output {
            TransfersCommand::Incoming(incoming_transfer) => {
                if self
                    .accepted_senders
                    .contains(&incoming_transfer.sender_address)
                {
                    self.accept(incoming_transfer, &sender);
                } else {
                    self.assess_sender(incoming_transfer, &sender);
                }
            }
            TransfersCommand::SenderAssessed(assessment) => {
                if assessment.trusted && self.preferences.auto_accept_from_trusted() {
                    self.accept(assessment.incoming_transfer, &sender);
                } else {
                    Self::ask_user(assessment, &sender);
                }
            }
            TransfersCommand::RemoveFinishedRow(finished_index) => {
                self.transfer_rows
                    .guard()
                    .remove(finished_index.current_index());
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinishReport {
    Pending,
    Sent,
}

#[derive(Debug)]
pub struct TransferRow {
    transfer_handle: TransferHandle,
    progress: TransferProgress,
    row_index: DynamicIndex,
    finish_report: FinishReport,
}

#[derive(Debug)]
pub enum TransferRowInput {
    Cancel,
}

#[derive(Debug)]
pub enum TransferRowCommand {
    Progress(TransferProgress),
    Cancelled,
}

fn describe_status(progress: TransferProgress) -> String {
    let transferred = megabytes(progress.transferred_bytes);
    let total = megabytes(progress.total_bytes);
    match progress.status {
        TransferStatus::Queued => "Waiting".to_string(),
        TransferStatus::Active => format!("{transferred:.2} of {total:.2} MB"),
        TransferStatus::Suspended => format!("Paused at {transferred:.2} MB"),
        TransferStatus::Complete => format!("Done, {total:.2} MB"),
        TransferStatus::Failed => "Failed".to_string(),
    }
}

fn completed_fraction(progress: TransferProgress) -> f64 {
    if progress.total_bytes == 0 {
        return 0.0;
    }
    (megabytes(progress.transferred_bytes) / megabytes(progress.total_bytes)).clamp(0.0, 1.0)
}

#[relm4::factory(pub)]
impl FactoryComponent for TransferRow {
    type Init = TransferHandle;
    type Input = TransferRowInput;
    type Output = DynamicIndex;
    type CommandOutput = TransferRowCommand;
    type ParentWidget = gtk::ListBox;

    view! {
        adw::ActionRow {
            set_title: &glib::markup_escape_text(&self.transfer_handle.file_name),
            set_activatable: false,
            #[watch]
            set_subtitle: &describe_status(self.progress),

            add_suffix = &gtk::ProgressBar {
                set_valign: gtk::Align::Center,
                set_width_request: 80,
                #[watch]
                set_fraction: completed_fraction(self.progress),
            },

            add_suffix = &gtk::Button {
                set_icon_name: "process-stop-symbolic",
                set_valign: gtk::Align::Center,
                set_tooltip_text: Some("Cancel"),
                add_css_class: "flat",
                #[watch]
                set_visible: !matches!(self.progress.status, TransferStatus::Complete | TransferStatus::Failed),
                connect_clicked => TransferRowInput::Cancel,
            },
        }
    }

    fn init_model(
        transfer_handle: Self::Init,
        row_index: &DynamicIndex,
        sender: FactorySender<Self>,
    ) -> Self {
        let progress_handle = transfer_handle.clone();
        sender.command(move |command_output, shutdown| {
            shutdown
                .register(async move {
                    let Ok(progress_updates) = progress_handle.progress().await else {
                        return;
                    };
                    progress_updates
                        .for_each(|progress| {
                            command_output.emit(TransferRowCommand::Progress(progress));
                            futures::future::ready(())
                        })
                        .await;
                })
                .drop_on_shutdown()
        });
        Self {
            progress: TransferProgress {
                status: TransferStatus::Queued,
                transferred_bytes: 0,
                total_bytes: transfer_handle.total_bytes,
            },
            transfer_handle,
            row_index: row_index.clone(),
            finish_report: FinishReport::Pending,
        }
    }

    fn update(&mut self, input: Self::Input, sender: FactorySender<Self>) {
        match input {
            TransferRowInput::Cancel => {
                let cancelled_handle = self.transfer_handle.clone();
                sender.oneshot_command(async move {
                    let _already_finished = cancelled_handle.cancel().await;
                    TransferRowCommand::Cancelled
                });
            }
        }
    }

    fn update_cmd(&mut self, command_output: Self::CommandOutput, sender: FactorySender<Self>) {
        match command_output {
            TransferRowCommand::Progress(progress) => self.progress = progress,
            TransferRowCommand::Cancelled => {
                self.progress = TransferProgress {
                    status: TransferStatus::Failed,
                    ..self.progress
                };
            }
        }
        let finished = matches!(
            self.progress.status,
            TransferStatus::Complete | TransferStatus::Failed
        );
        if finished && self.finish_report == FinishReport::Pending {
            self.finish_report = FinishReport::Sent;
            let _popover_gone = sender.output(self.row_index.clone());
        }
    }
}
