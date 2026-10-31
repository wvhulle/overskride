use std::{collections::HashSet, time::Duration};

use bindrune_bluez::{
    bluez_dbus_session::{BluezDbusSession, DeviceLocation},
    obex_file_transfer::{
        obex_incoming_push_agent::IncomingTransfer, obex_transfer_progress::TransferHandle,
    },
    remote_device::remote_device_commands::read_device,
};
use futures::StreamExt;
use relm4::{
    adw::prelude::*,
    factory::{DynamicIndex, FactoryVecDeque},
    gtk,
    prelude::*,
};

use crate::{
    background_application_lifecycle::gsettings_stored_preferences::GsettingsStoredPreferences,
    device_detail::file_transfer_group::ObexAvailability,
    header_transfer_popover::file_transfer_progress_row::FileTransferProgressRow,
};

const FINISHED_TRANSFER_LINGER: Duration = Duration::from_secs(60);

pub struct FileTransferPopoverInit {
    pub session: BluezDbusSession,
    pub obex: ObexAvailability,
    pub preferences: GsettingsStoredPreferences,
    pub adapter_name: String,
}

pub struct FileTransferPopover {
    session: BluezDbusSession,
    preferences: GsettingsStoredPreferences,
    adapter_name: String,
    accepted_senders: HashSet<bindrune_bluez::Address>,
    transfer_rows: FactoryVecDeque<FileTransferProgressRow>,
}

#[derive(Debug)]
pub enum FileTransferPopoverInput {
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
pub enum FileTransferPopoverOutput {
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
pub enum FileTransferPopoverCommand {
    Incoming(IncomingTransfer),
    SenderAssessed(SenderAssessment),
    RemoveFinishedRow(DynamicIndex),
}

impl FileTransferPopover {
    fn accept(&mut self, incoming_transfer: IncomingTransfer, sender: &ComponentSender<Self>) {
        let sender_address = incoming_transfer.sender_address;
        let file_name = incoming_transfer.file_name.clone();
        let transfer_handle = incoming_transfer.accept(self.preferences.received_files_folder());
        self.accepted_senders.insert(sender_address);
        self.transfer_rows.guard().push_back(transfer_handle);
        let _app_gone = sender.output(FileTransferPopoverOutput::ReceivingFile { file_name });
    }

    fn assess_sender(&self, incoming_transfer: IncomingTransfer, sender: &ComponentSender<Self>) {
        let session = self.session.clone();
        let location = DeviceLocation {
            adapter_name: self.adapter_name.clone(),
            address: incoming_transfer.sender_address,
        };
        sender.oneshot_command(async move {
            let sender_snapshot = read_device(&session, &location).await;
            FileTransferPopoverCommand::SenderAssessed(SenderAssessment {
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
impl Component for FileTransferPopover {
    type Init = FileTransferPopoverInit;
    type Input = FileTransferPopoverInput;
    type Output = FileTransferPopoverOutput;
    type CommandOutput = FileTransferPopoverCommand;

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
            .forward(sender.input_sender(), FileTransferPopoverInput::RowFinished);
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
                                command_output
                                    .emit(FileTransferPopoverCommand::Incoming(incoming_transfer));
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
            FileTransferPopoverInput::TransfersStarted(transfer_handles) => {
                let mut transfer_rows = self.transfer_rows.guard();
                transfer_handles.into_iter().for_each(|transfer_handle| {
                    transfer_rows.push_back(transfer_handle);
                });
            }
            FileTransferPopoverInput::AdapterSwitched(adapter_name) => {
                self.adapter_name = adapter_name
            }
            FileTransferPopoverInput::Decided {
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
            FileTransferPopoverInput::RowFinished(finished_index) => {
                sender.oneshot_command(async move {
                    tokio::time::sleep(FINISHED_TRANSFER_LINGER).await;
                    FileTransferPopoverCommand::RemoveFinishedRow(finished_index)
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
            FileTransferPopoverCommand::Incoming(incoming_transfer) => {
                if self
                    .accepted_senders
                    .contains(&incoming_transfer.sender_address)
                {
                    self.accept(incoming_transfer, &sender);
                } else {
                    self.assess_sender(incoming_transfer, &sender);
                }
            }
            FileTransferPopoverCommand::SenderAssessed(assessment) => {
                if assessment.bonded
                    && self
                        .preferences
                        .accepts_files_from(assessment.incoming_transfer.sender_address)
                {
                    self.accept(assessment.incoming_transfer, &sender);
                } else {
                    let _main_view_gone =
                        sender.output(FileTransferPopoverOutput::DecisionNeeded(assessment));
                }
            }
            FileTransferPopoverCommand::RemoveFinishedRow(finished_index) => {
                self.transfer_rows
                    .guard()
                    .remove(finished_index.current_index());
            }
        }
    }
}
