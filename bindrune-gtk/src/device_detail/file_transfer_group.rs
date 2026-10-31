use std::path::PathBuf;

use bindrune_bluez::{
    bluez_operation_error::BluetoothError,
    obex_file_transfer::{
        obex_dbus_connection::ObexConnection, obex_transfer_progress::TransferHandle,
    },
    remote_device::remote_device_snapshot::DeviceSnapshot,
};
use relm4::{adw, adw::prelude::*, gtk, gtk::gio, prelude::*};

#[derive(Clone, Debug)]
pub enum ObexAvailability {
    Unavailable,
    Available(ObexConnection),
}

pub struct FileTransferGroupInit {
    pub obex: ObexAvailability,
    pub adapter_address: bindrune_bluez::Address,
    pub accepts_files: bool,
    pub received_files_folder: PathBuf,
    pub snapshot: DeviceSnapshot,
}

pub struct FileTransferGroup {
    obex: ObexAvailability,
    adapter_address: bindrune_bluez::Address,
    accepts_files: bool,
    received_files_folder: PathBuf,
    snapshot: DeviceSnapshot,
}

#[derive(Debug)]
pub enum FileTransferGroupInput {
    DeviceChanged(DeviceSnapshot),
    ChooseFilesToSend,
    SendFiles(Vec<PathBuf>),
    AutoAcceptToggled(bool),
    AutoAcceptChanged(bool),
}

#[derive(Debug)]
pub enum FileTransferGroupOutput {
    Failed(BluetoothError),
    AutoAcceptChosen {
        address: bindrune_bluez::Address,
        accepts_files: bool,
    },
    TransfersStarted(Vec<TransferHandle>),
}

#[derive(Debug)]
pub enum FileTransferGroupCommand {
    FilesSending(Result<Vec<TransferHandle>, BluetoothError>),
}

impl FileTransferGroup {
    fn describe_auto_accept(&self) -> String {
        if self.snapshot.bonded {
            format!("Saved straight to {}", self.received_files_folder.display())
        } else {
            "Pair this device first".to_string()
        }
    }
}

#[relm4::component(pub)]
impl Component for FileTransferGroup {
    type Init = FileTransferGroupInit;
    type Input = FileTransferGroupInput;
    type Output = FileTransferGroupOutput;
    type CommandOutput = FileTransferGroupCommand;

    view! {
        adw::PreferencesGroup {
            set_title: "Files",
            #[watch]
            set_visible: model.snapshot.offers_object_push(),

            adw::SwitchRow {
                set_title: "Accept Files Without Asking",
                #[watch]
                set_sensitive: model.snapshot.bonded,
                #[watch]
                set_subtitle: &model.describe_auto_accept(),
                #[watch]
                #[block_signal(auto_accept_handler)]
                set_active: model.accepts_files,
                connect_active_notify[sender] => move |switch_row| {
                    sender.input(FileTransferGroupInput::AutoAcceptToggled(switch_row.is_active()));
                } @auto_accept_handler,
            },

            adw::ActionRow {
                set_title: "Send File To Device",
                #[watch]
                set_sensitive: model.snapshot.connected && matches!(model.obex, ObexAvailability::Available(_)),

                add_suffix = &gtk::Button {
                    set_label: "Choose File",
                    set_valign: gtk::Align::Center,
                    connect_clicked => FileTransferGroupInput::ChooseFilesToSend,
                },
            },
        }
    }

    fn init(
        group_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {
            obex: group_init.obex,
            adapter_address: group_init.adapter_address,
            accepts_files: group_init.accepts_files,
            received_files_folder: group_init.received_files_folder,
            snapshot: group_init.snapshot,
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match input {
            FileTransferGroupInput::DeviceChanged(snapshot) => self.snapshot = snapshot,
            FileTransferGroupInput::ChooseFilesToSend => {
                let file_dialog = gtk::FileDialog::builder()
                    .title("Choose Files to Send")
                    .modal(true)
                    .build();
                let input_sender = sender.input_sender().clone();
                relm4::spawn_local(async move {
                    let parent_window = relm4::main_application().active_window();
                    let Ok(chosen_files) = file_dialog
                        .open_multiple_future(parent_window.as_ref())
                        .await
                    else {
                        return;
                    };
                    let file_paths: Vec<_> = (0..chosen_files.n_items())
                        .filter_map(|file_position| chosen_files.item(file_position))
                        .filter_map(|file_object| file_object.downcast::<gio::File>().ok())
                        .filter_map(|chosen_file| chosen_file.path())
                        .collect();
                    if !file_paths.is_empty() {
                        input_sender.emit(FileTransferGroupInput::SendFiles(file_paths));
                    }
                });
            }
            FileTransferGroupInput::SendFiles(file_paths) => {
                let ObexAvailability::Available(obex_connection) = self.obex.clone() else {
                    return;
                };
                let source_address = self.adapter_address;
                let destination_address = self.snapshot.address();
                sender.oneshot_command(async move {
                    FileTransferGroupCommand::FilesSending(
                        obex_connection
                            .send_files(source_address, destination_address, file_paths)
                            .await,
                    )
                });
            }
            FileTransferGroupInput::AutoAcceptToggled(accepts_files) => {
                self.accepts_files = accepts_files;
                let _device_page_gone = sender.output(FileTransferGroupOutput::AutoAcceptChosen {
                    address: self.snapshot.address(),
                    accepts_files,
                });
            }
            FileTransferGroupInput::AutoAcceptChanged(accepts_files) => {
                self.accepts_files = accepts_files;
            }
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        let group_output = match command_output {
            FileTransferGroupCommand::FilesSending(Ok(transfer_handles)) => {
                FileTransferGroupOutput::TransfersStarted(transfer_handles)
            }
            FileTransferGroupCommand::FilesSending(Err(sending_error)) => {
                FileTransferGroupOutput::Failed(sending_error)
            }
        };
        let _device_page_gone = sender.output(group_output);
    }
}
