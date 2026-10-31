use bindrune_bluez::obex_file_transfer::obex_transfer_progress::{
    TransferHandle, TransferProgress, TransferStatus,
};
use bytesize::ByteSize;
use futures::StreamExt;
use relm4::{
    adw,
    adw::prelude::*,
    factory::{DynamicIndex, FactoryComponent, FactorySender},
    gtk,
    gtk::glib,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinishReport {
    Pending,
    Sent,
}

#[derive(Debug)]
pub struct FileTransferProgressRow {
    transfer_handle: TransferHandle,
    progress: TransferProgress,
    row_index: DynamicIndex,
    finish_report: FinishReport,
}

#[derive(Debug)]
pub enum FileTransferProgressRowInput {
    Cancel,
}

#[derive(Debug)]
pub enum FileTransferProgressRowCommand {
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
impl FactoryComponent for FileTransferProgressRow {
    type Init = TransferHandle;
    type Input = FileTransferProgressRowInput;
    type Output = DynamicIndex;
    type CommandOutput = FileTransferProgressRowCommand;
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
                connect_clicked => FileTransferProgressRowInput::Cancel,
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
                            command_output.emit(FileTransferProgressRowCommand::Progress(progress));
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
            FileTransferProgressRowInput::Cancel => {
                let cancelled_handle = self.transfer_handle.clone();
                sender.oneshot_command(async move {
                    let _already_finished = cancelled_handle.cancel().await;
                    FileTransferProgressRowCommand::Cancelled
                });
            }
        }
    }

    fn update_cmd(&mut self, command_output: Self::CommandOutput, sender: FactorySender<Self>) {
        match command_output {
            FileTransferProgressRowCommand::Progress(progress) => self.progress = progress,
            FileTransferProgressRowCommand::Cancelled => {
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
