use std::{collections::HashSet, time::Duration};

use bindrune_bluez::{
    bluez_dbus_session::{BluetoothSession, DeviceLocation},
    obex_file_transfer::obex_file_transfers::{
        IncomingTransfer, TransferHandle, TransferProgress, TransferStatus,
    },
    remote_device::remote_device_commands::read_device,
};
use bytesize::ByteSize;
use futures::StreamExt;
use relm4::{
    adw,
    adw::prelude::*,
    factory::{DynamicIndex, FactoryComponent, FactorySender, FactoryVecDeque},
    gtk,
    gtk::glib,
    prelude::*,
};

use crate::{
    background_application_lifecycle::gsettings_stored_preferences::AppPreferences,
    device_detail::device_detail_page::ObexAvailability,
};

const FINISHED_TRANSFER_LINGER: Duration = Duration::from_secs(60);

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
    accepted_senders: HashSet<bindrune_bluez::Address>,
    transfer_rows: FactoryVecDeque<TransferRow>,
}

#[derive(Debug)]
pub enum TransfersInput {
    TransfersStarted(Vec<TransferHandle>),
    AdapterSwitched(String),
    RowFinished(DynamicIndex),
    Decided {
        incoming_transfer: IncomingTransfer,
        transfer_decision: TransferDecision,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferDecision {
    Rejected,
    Accepted,
    AlwaysAccepted,
}

impl TransferDecision {
    pub fn from_response(response_id: &str) -> Self {
        match response_id {
            "always" => Self::AlwaysAccepted,
            "accept" => Self::Accepted,
            _ => Self::Rejected,
        }
    }
}

#[derive(Debug)]
pub enum TransfersOutput {
    ReceivingFile { file_name: String },
    DecisionNeeded(SenderAssessment),
}

#[derive(Debug)]
pub struct SenderAssessment {
    pub incoming_transfer: IncomingTransfer,
    pub sender_name: String,
    pub bonded: bool,
}

#[derive(Debug)]
pub enum TransfersCommand {
    Incoming(IncomingTransfer),
    SenderAssessed(SenderAssessment),
    RemoveFinishedRow(DynamicIndex),
}

impl TransfersPopover {
    fn accept(&mut self, incoming_transfer: IncomingTransfer, sender: &ComponentSender<Self>) {
        let sender_address = incoming_transfer.sender_address;
        let file_name = incoming_transfer.file_name.clone();
        let transfer_handle = incoming_transfer.accept(self.preferences.received_files_folder());
        self.accepted_senders.insert(sender_address);
        self.transfer_rows.guard().push_back(transfer_handle);
        let _app_gone = sender.output(TransfersOutput::ReceivingFile { file_name });
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
                bonded: sender_snapshot.is_ok_and(|snapshot| snapshot.bonded),
                incoming_transfer,
            })
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
                transfer_decision,
            } => match transfer_decision {
                TransferDecision::Rejected => incoming_transfer.reject(),
                TransferDecision::Accepted => self.accept(incoming_transfer, &sender),
                TransferDecision::AlwaysAccepted => {
                    self.preferences
                        .allow_auto_accept(incoming_transfer.sender_address);
                    self.accept(incoming_transfer, &sender);
                }
            },
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
                if assessment.bonded
                    && self
                        .preferences
                        .accepts_files_from(assessment.incoming_transfer.sender_address)
                {
                    self.accept(assessment.incoming_transfer, &sender);
                } else {
                    let _main_view_gone =
                        sender.output(TransfersOutput::DecisionNeeded(assessment));
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
    let transferred = progress.transferred_size.display().si();
    let total = progress.total_size.display().si();
    match progress.status {
        TransferStatus::Queued => "Waiting".to_string(),
        TransferStatus::Active => format!("{transferred} of {total}"),
        TransferStatus::Suspended => format!("Paused at {transferred}"),
        TransferStatus::Complete => format!("Done, {total}"),
        TransferStatus::Failed => "Failed".to_string(),
    }
}

fn completed_fraction(progress: TransferProgress) -> f64 {
    if progress.total_size.as_u64() == 0 {
        return 0.0;
    }
    (progress.transferred_size.as_kb() / progress.total_size.as_kb()).clamp(0.0, 1.0)
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
                transferred_size: ByteSize::b(0),
                total_size: transfer_handle.total_size,
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
