use std::path::{Path, PathBuf};

use bytesize::ByteSize;
use futures::{Stream, StreamExt, future, stream};
use zbus::zvariant::OwnedObjectPath;

use crate::{
    bluez_operation_error::BluetoothError,
    obex_file_transfer::{
        obex_dbus_connection::ObexConnection, obex_dbus_proxies::ObexTransferProxy,
    },
};

const OBEXD_CACHE_FOLDER: &str = "obexd";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferStatus {
    Queued,
    Active,
    Suspended,
    Complete,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferProgress {
    pub status: TransferStatus,
    pub transferred_size: ByteSize,
    pub total_size: ByteSize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ReceivedFileHandling {
    LeaveWhereObexdPutIt,
    MoveInto(PathBuf),
}

#[derive(Clone, Debug)]
pub struct TransferHandle {
    pub(super) obex_connection: ObexConnection,
    pub(super) transfer_path: OwnedObjectPath,
    pub file_name: String,
    pub total_size: ByteSize,
    pub(super) received_file_handling: ReceivedFileHandling,
}

fn parse_transfer_status(status_text: &str) -> TransferStatus {
    match status_text {
        "queued" => TransferStatus::Queued,
        "active" => TransferStatus::Active,
        "suspended" => TransferStatus::Suspended,
        "complete" => TransferStatus::Complete,
        _ => TransferStatus::Failed,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransferPropertyUpdate {
    Status(TransferStatus),
    Transferred(ByteSize),
}

fn apply_transfer_update(
    progress: TransferProgress,
    property_update: TransferPropertyUpdate,
) -> TransferProgress {
    match property_update {
        TransferPropertyUpdate::Status(status) => TransferProgress { status, ..progress },
        TransferPropertyUpdate::Transferred(transferred_size) => TransferProgress {
            transferred_size,
            ..progress
        },
    }
}

fn is_finished(status: TransferStatus) -> bool {
    matches!(status, TransferStatus::Complete | TransferStatus::Failed)
}

fn obexd_received_file_path(file_name: &str) -> Option<PathBuf> {
    let cache_folder = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home_folder| PathBuf::from(home_folder).join(".cache"))
        })?;
    Some(cache_folder.join(OBEXD_CACHE_FOLDER).join(file_name))
}

fn move_received_file(file_name: &str, destination_folder: &Path) -> Result<(), BluetoothError> {
    let Some(source_path) = obexd_received_file_path(file_name) else {
        return Err(BluetoothError::Filesystem(std::io::ErrorKind::NotFound));
    };
    std::fs::create_dir_all(destination_folder)?;
    let destination_path = destination_folder.join(file_name);
    if std::fs::rename(&source_path, &destination_path).is_err() {
        std::fs::copy(&source_path, &destination_path)?;
        std::fs::remove_file(&source_path)?;
    }
    Ok(())
}

impl TransferHandle {
    pub fn transfer_identifier(&self) -> String {
        self.transfer_path.to_string()
    }

    async fn transfer_proxy(&self) -> zbus::Result<ObexTransferProxy<'static>> {
        ObexTransferProxy::new(&self.obex_connection.connection, self.transfer_path.clone()).await
    }

    pub async fn cancel(&self) -> Result<(), BluetoothError> {
        self.transfer_proxy().await?.cancel().await?;
        Ok(())
    }

    fn finish_received_file(&self, progress: TransferProgress) -> TransferProgress {
        let ReceivedFileHandling::MoveInto(destination_folder) = &self.received_file_handling
        else {
            return progress;
        };
        if progress.status != TransferStatus::Complete {
            return progress;
        }
        match move_received_file(&self.file_name, destination_folder) {
            Ok(()) => progress,
            Err(_move_failure) => TransferProgress {
                status: TransferStatus::Failed,
                ..progress
            },
        }
    }

    pub async fn progress(
        self,
    ) -> Result<impl Stream<Item = TransferProgress> + Send + 'static, BluetoothError> {
        let transfer_proxy = self.transfer_proxy().await?;
        let Ok(status_text) = transfer_proxy.status().await else {
            let finished_progress = TransferProgress {
                status: TransferStatus::Complete,
                transferred_size: self.total_size,
                total_size: self.total_size,
            };
            return Ok(
                stream::once(future::ready(self.finish_received_file(finished_progress))).boxed(),
            );
        };
        let initial_progress = TransferProgress {
            status: parse_transfer_status(&status_text),
            transferred_size: transfer_proxy
                .transferred()
                .await
                .map(ByteSize::b)
                .unwrap_or_default(),
            total_size: transfer_proxy
                .size()
                .await
                .map(ByteSize::b)
                .unwrap_or(self.total_size),
        };
        let status_updates =
            transfer_proxy
                .receive_status_changed()
                .await
                .filter_map(|status_change| async move {
                    let status_text = status_change.get().await.ok()?;
                    Some(TransferPropertyUpdate::Status(parse_transfer_status(
                        &status_text,
                    )))
                });
        let transferred_updates = transfer_proxy
            .receive_transferred_changed()
            .await
            .filter_map(|transferred_change| async move {
                let transferred_bytes = transferred_change.get().await.ok()?;
                Some(TransferPropertyUpdate::Transferred(ByteSize::b(
                    transferred_bytes,
                )))
            });
        let progress_updates = stream::select(status_updates, transferred_updates).scan(
            initial_progress,
            |current_progress, property_update| {
                *current_progress = apply_transfer_update(*current_progress, property_update);
                future::ready(Some(*current_progress))
            },
        );
        Ok(async_stream::stream! {
            let _transfer_proxy = transfer_proxy;
            let mut progress_updates = std::pin::pin!(
                stream::once(future::ready(initial_progress)).chain(progress_updates)
            );
            let mut last_yielded_progress = None;
            while let Some(progress) = progress_updates.next().await {
                if last_yielded_progress == Some(progress) {
                    continue;
                }
                last_yielded_progress = Some(progress);
                let finished = is_finished(progress.status);
                yield self.finish_received_file(progress);
                if finished {
                    break;
                }
            }
        }
        .boxed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_updates_change_only_their_own_field() {
        let started = TransferProgress {
            status: TransferStatus::Queued,
            transferred_size: ByteSize::b(0),
            total_size: ByteSize::kb(2),
        };
        let active = apply_transfer_update(
            started,
            TransferPropertyUpdate::Status(parse_transfer_status("active")),
        );
        assert_eq!(active.status, TransferStatus::Active);
        assert_eq!(active.total_size, ByteSize::kb(2));
        let halfway =
            apply_transfer_update(active, TransferPropertyUpdate::Transferred(ByteSize::kb(1)));
        assert_eq!(halfway.status, TransferStatus::Active);
        assert_eq!(halfway.transferred_size, ByteSize::kb(1));
    }

    #[test]
    fn unknown_status_counts_as_failure() {
        assert_eq!(parse_transfer_status("error"), TransferStatus::Failed);
        assert!(is_finished(parse_transfer_status("complete")));
        assert!(!is_finished(parse_transfer_status("suspended")));
    }
}
