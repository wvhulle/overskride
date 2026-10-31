use std::path::{Path, PathBuf};

use bytesize::ByteSize;
use dbus::{
    arg::{PropMap, RefArg},
    message::MatchRule,
    nonblock::stdintf::org_freedesktop_dbus::{Properties, PropertiesPropertiesChanged},
};
use futures::{Stream, StreamExt, future, stream};

use crate::{
    bluez_operation_error::BluetoothError, obex_file_transfer::obex_dbus_connection::ObexConnection,
};

pub(super) const TRANSFER_INTERFACE: &str = "org.bluez.obex.Transfer1";
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
    pub(super) transfer_path: dbus::Path<'static>,
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

pub(super) fn property_text(properties: &PropMap, property_name: &str) -> Option<String> {
    Some(properties.get(property_name)?.0.as_str()?.to_string())
}

pub(super) fn property_size(properties: &PropMap, property_name: &str) -> Option<ByteSize> {
    properties.get(property_name)?.0.as_u64().map(ByteSize::b)
}

fn apply_transfer_properties(
    progress: TransferProgress,
    changed_properties: &PropMap,
) -> TransferProgress {
    TransferProgress {
        status: property_text(changed_properties, "Status")
            .map_or(progress.status, |status_text| {
                parse_transfer_status(&status_text)
            }),
        transferred_size: property_size(changed_properties, "Transferred")
            .unwrap_or(progress.transferred_size),
        total_size: property_size(changed_properties, "Size").unwrap_or(progress.total_size),
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

    pub async fn cancel(&self) -> Result<(), BluetoothError> {
        self.obex_connection
            .proxy(self.transfer_path.clone())
            .method_call::<(), _, _, _>(TRANSFER_INTERFACE, "Cancel", ())
            .await?;
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
        let property_rule =
            MatchRule::new_signal("org.freedesktop.DBus.Properties", "PropertiesChanged")
                .with_path(self.transfer_path.clone());
        let (signal_match, property_signals) = self
            .obex_connection
            .connection
            .add_match(property_rule)
            .await?
            .stream::<PropertiesPropertiesChanged>();
        let initial_progress = match self
            .obex_connection
            .proxy(self.transfer_path.clone())
            .get_all(TRANSFER_INTERFACE)
            .await
        {
            Ok(transfer_properties) => apply_transfer_properties(
                TransferProgress {
                    status: TransferStatus::Queued,
                    transferred_size: ByteSize::b(0),
                    total_size: self.total_size,
                },
                &transfer_properties,
            ),
            Err(_transfer_already_gone) => TransferProgress {
                status: TransferStatus::Complete,
                transferred_size: self.total_size,
                total_size: self.total_size,
            },
        };
        let signal_updates = property_signals
            .filter(|(_signal_message, properties_changed)| {
                future::ready(properties_changed.interface_name == TRANSFER_INTERFACE)
            })
            .scan(
                initial_progress,
                |current_progress, (_signal_message, properties_changed)| {
                    *current_progress = apply_transfer_properties(
                        *current_progress,
                        &properties_changed.changed_properties,
                    );
                    future::ready(Some(*current_progress))
                },
            );
        let progress_updates = stream::once(future::ready(initial_progress)).chain(signal_updates);
        Ok(async_stream::stream! {
            let _signal_subscription = signal_match;
            let mut progress_updates = std::pin::pin!(progress_updates);
            while let Some(progress) = progress_updates.next().await {
                let finished = is_finished(progress.status);
                yield self.finish_received_file(progress);
                if finished {
                    break;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use dbus::arg::Variant;

    use super::*;

    fn property_map(entries: Vec<(&str, Box<dyn RefArg>)>) -> PropMap {
        entries
            .into_iter()
            .map(|(property_name, property_value)| {
                (property_name.to_string(), Variant(property_value))
            })
            .collect()
    }

    #[test]
    fn transfer_properties_update_only_what_changed() {
        let started = TransferProgress {
            status: TransferStatus::Queued,
            transferred_size: ByteSize::b(0),
            total_size: ByteSize::kb(2),
        };
        let active = apply_transfer_properties(
            started,
            &property_map(vec![("Status", Box::new("active".to_string()))]),
        );
        assert_eq!(active.status, TransferStatus::Active);
        assert_eq!(active.total_size, ByteSize::kb(2));
        let halfway = apply_transfer_properties(
            active,
            &property_map(vec![("Transferred", Box::new(1_000_u64))]),
        );
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
