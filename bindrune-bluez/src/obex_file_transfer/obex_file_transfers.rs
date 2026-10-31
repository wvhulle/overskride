use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use bytesize::ByteSize;
use dbus::{
    arg::{PropMap, RefArg, Variant},
    channel::MatchingReceiver,
    message::MatchRule,
    nonblock::{
        Proxy, SyncConnection,
        stdintf::org_freedesktop_dbus::{Properties, PropertiesPropertiesChanged},
    },
};
use dbus_crossroads::{Crossroads, MethodErr};
use futures::{Stream, StreamExt, future, stream};
use tokio::sync::{mpsc, oneshot};

use crate::bluez_operation_error::BluetoothError;

const OBEX_SERVICE: &str = "org.bluez.obex";
const OBEX_MANAGER_PATH: &str = "/org/bluez/obex";
const AGENT_OBJECT_PATH: &str = "/overskride/agent";
const AGENT_INTERFACE: &str = "org.bluez.obex.Agent1";
const AGENT_MANAGER_INTERFACE: &str = "org.bluez.obex.AgentManager1";
const CLIENT_INTERFACE: &str = "org.bluez.obex.Client1";
const OBJECT_PUSH_INTERFACE: &str = "org.bluez.obex.ObjectPush1";
const SESSION_INTERFACE: &str = "org.bluez.obex.Session1";
const TRANSFER_INTERFACE: &str = "org.bluez.obex.Transfer1";
const OBJECT_PUSH_TARGET: &str = "OPP";
const OBEX_CALL_TIMEOUT: Duration = Duration::from_secs(10);
const OBEXD_CACHE_FOLDER: &str = "obexd";

#[derive(Clone)]
pub struct ObexConnection {
    connection: Arc<SyncConnection>,
}

impl std::fmt::Debug for ObexConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ObexConnection")
    }
}

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
enum ReceivedFileHandling {
    LeaveWhereObexdPutIt,
    MoveInto(PathBuf),
}

#[derive(Clone, Debug)]
pub struct TransferHandle {
    obex_connection: ObexConnection,
    transfer_path: dbus::Path<'static>,
    pub file_name: String,
    pub total_size: ByteSize,
    received_file_handling: ReceivedFileHandling,
}

#[derive(Debug)]
pub struct IncomingTransfer {
    pub sender_address: bluer::Address,
    pub file_name: String,
    pub total_size: ByteSize,
    obex_connection: ObexConnection,
    transfer_path: dbus::Path<'static>,
    reply_channel: oneshot::Sender<()>,
}

#[derive(Clone)]
struct PushForwarder {
    transfer_sender: mpsc::UnboundedSender<IncomingTransfer>,
    obex_connection: ObexConnection,
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

fn property_text(properties: &PropMap, property_name: &str) -> Option<String> {
    Some(properties.get(property_name)?.0.as_str()?.to_string())
}

fn property_size(properties: &PropMap, property_name: &str) -> Option<ByteSize> {
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

impl ObexConnection {
    pub fn open() -> Result<Self, BluetoothError> {
        let (connection_resource, connection) = dbus_tokio::connection::new_session_sync()?;
        tokio::spawn(connection_resource);
        Ok(Self { connection })
    }

    fn proxy<'path>(
        &self,
        object_path: impl Into<dbus::Path<'path>>,
    ) -> Proxy<'path, Arc<SyncConnection>> {
        Proxy::new(
            OBEX_SERVICE,
            object_path,
            OBEX_CALL_TIMEOUT,
            Arc::clone(&self.connection),
        )
    }

    pub async fn incoming_transfers(
        &self,
    ) -> Result<impl Stream<Item = IncomingTransfer> + Send + 'static, BluetoothError> {
        let (transfer_sender, mut transfer_receiver) = mpsc::unbounded_channel();
        let mut crossroads = Crossroads::new();
        let crossroads_connection: Arc<SyncConnection> = Arc::clone(&self.connection);
        crossroads.set_async_support(Some((
            crossroads_connection,
            Box::new(|agent_future| {
                tokio::spawn(agent_future);
            }),
        )));
        let agent_interface = crossroads.register(AGENT_INTERFACE, |interface_builder| {
            interface_builder.method_with_cr_async(
                "AuthorizePush",
                ("transfer",),
                ("filename",),
                |mut method_context, crossroads, (transfer_path,): (dbus::Path<'static>,)| {
                    let push_forwarder = crossroads
                        .data_mut::<PushForwarder>(method_context.path())
                        .map(|push_forwarder| push_forwarder.clone());
                    async move {
                        let authorization = match push_forwarder {
                            Some(push_forwarder) => {
                                push_forwarder.authorize_push(transfer_path).await
                            }
                            None => Err(PushRefusal::NobodyListening),
                        };
                        method_context.reply(authorization.map_err(MethodErr::from))
                    }
                },
            );
            interface_builder.method(
                "Cancel",
                (),
                (),
                |_method_context, _push_forwarder: &mut PushForwarder, (): ()| Ok(()),
            );
            interface_builder.method(
                "Release",
                (),
                (),
                |_method_context, _push_forwarder: &mut PushForwarder, (): ()| Ok(()),
            );
        });
        crossroads.insert(
            AGENT_OBJECT_PATH,
            &[agent_interface],
            PushForwarder {
                transfer_sender,
                obex_connection: self.clone(),
            },
        );
        let receive_token = self.connection.start_receive(
            MatchRule::new_method_call().with_path(AGENT_OBJECT_PATH),
            Box::new(move |method_call, connection| {
                let _unhandled_call_is_answered_by_crossroads =
                    crossroads.handle_message(method_call, connection);
                true
            }),
        );
        self.proxy(OBEX_MANAGER_PATH)
            .method_call::<(), _, _, _>(
                AGENT_MANAGER_INTERFACE,
                "RegisterAgent",
                (dbus::Path::from(AGENT_OBJECT_PATH),),
            )
            .await?;
        let registered_connection = self.clone();
        Ok(async_stream::stream! {
            let _agent_registration = AgentRegistration { obex_connection: registered_connection, receive_token };
            while let Some(incoming_transfer) = transfer_receiver.recv().await {
                yield incoming_transfer;
            }
        })
    }

    pub async fn send_files(
        &self,
        source_address: bluer::Address,
        destination_address: bluer::Address,
        file_paths: Vec<PathBuf>,
    ) -> Result<Vec<TransferHandle>, BluetoothError> {
        let mut session_arguments = PropMap::new();
        session_arguments.insert(
            "Target".to_string(),
            Variant(Box::new(OBJECT_PUSH_TARGET.to_string())),
        );
        session_arguments.insert(
            "Source".to_string(),
            Variant(Box::new(source_address.to_string())),
        );
        let (session_path,): (dbus::Path<'static>,) = self
            .proxy(OBEX_MANAGER_PATH)
            .method_call(
                CLIENT_INTERFACE,
                "CreateSession",
                (destination_address.to_string(), session_arguments),
            )
            .await?;
        let transfer_starts = file_paths
            .into_iter()
            .map(|file_path| self.send_file(session_path.clone(), file_path));
        futures::future::try_join_all(transfer_starts).await
    }

    async fn send_file(
        &self,
        session_path: dbus::Path<'static>,
        file_path: PathBuf,
    ) -> Result<TransferHandle, BluetoothError> {
        let (transfer_path, transfer_properties): (dbus::Path<'static>, PropMap) = self
            .proxy(session_path)
            .method_call(
                OBJECT_PUSH_INTERFACE,
                "SendFile",
                (file_path.to_string_lossy().to_string(),),
            )
            .await?;
        let fallback_name = file_path
            .file_name()
            .map(|file_name| file_name.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(TransferHandle {
            obex_connection: self.clone(),
            transfer_path,
            file_name: property_text(&transfer_properties, "Name").unwrap_or(fallback_name),
            total_size: property_size(&transfer_properties, "Size").unwrap_or_default(),
            received_file_handling: ReceivedFileHandling::LeaveWhereObexdPutIt,
        })
    }
}

struct AgentRegistration {
    obex_connection: ObexConnection,
    receive_token: dbus::channel::Token,
}

impl Drop for AgentRegistration {
    fn drop(&mut self) {
        let _crossroads_handler = self
            .obex_connection
            .connection
            .stop_receive(self.receive_token);
        let unregistering_connection = self.obex_connection.clone();
        tokio::spawn(async move {
            let _already_unregistered: Result<(), dbus::Error> = unregistering_connection
                .proxy(OBEX_MANAGER_PATH)
                .method_call(
                    AGENT_MANAGER_INTERFACE,
                    "UnregisterAgent",
                    (dbus::Path::from(AGENT_OBJECT_PATH),),
                )
                .await;
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PushRefusal {
    TransferUnreadable,
    TransferWithoutName,
    TransferWithoutSession,
    SessionWithoutSender,
    NobodyListening,
    DeclinedByUser,
}

impl From<PushRefusal> for MethodErr {
    fn from(push_refusal: PushRefusal) -> Self {
        let (error_name, error_text) = match push_refusal {
            PushRefusal::TransferUnreadable => (
                "org.bluez.obex.Error.Failed",
                "transfer properties are unreadable",
            ),
            PushRefusal::TransferWithoutName => {
                ("org.bluez.obex.Error.Failed", "transfer has no name")
            }
            PushRefusal::TransferWithoutSession => {
                ("org.bluez.obex.Error.Failed", "transfer has no session")
            }
            PushRefusal::SessionWithoutSender => (
                "org.bluez.obex.Error.Failed",
                "session has no sender address",
            ),
            PushRefusal::NobodyListening => (
                "org.bluez.obex.Error.Canceled",
                "nobody listens for incoming transfers",
            ),
            PushRefusal::DeclinedByUser => ("org.bluez.obex.Error.Rejected", "Not Authorized"),
        };
        MethodErr::from((error_name, error_text))
    }
}

impl PushForwarder {
    async fn read_properties(
        &self,
        object_path: dbus::Path<'static>,
        interface_name: &str,
    ) -> Result<PropMap, PushRefusal> {
        let Ok(object_properties) = self
            .obex_connection
            .proxy(object_path)
            .get_all(interface_name)
            .await
        else {
            return Err(PushRefusal::TransferUnreadable);
        };
        Ok(object_properties)
    }

    async fn authorize_push(
        &self,
        transfer_path: dbus::Path<'static>,
    ) -> Result<(String,), PushRefusal> {
        let transfer_properties = self
            .read_properties(transfer_path.clone(), TRANSFER_INTERFACE)
            .await?;
        let Some(file_name) = property_text(&transfer_properties, "Name") else {
            return Err(PushRefusal::TransferWithoutName);
        };
        let Some(session_path) = property_text(&transfer_properties, "Session") else {
            return Err(PushRefusal::TransferWithoutSession);
        };
        let session_properties = self
            .read_properties(dbus::Path::from(session_path), SESSION_INTERFACE)
            .await?;
        let Some(sender_address) = property_text(&session_properties, "Destination")
            .and_then(|destination_text| destination_text.parse().ok())
        else {
            return Err(PushRefusal::SessionWithoutSender);
        };
        let (reply_channel, reply_receiver) = oneshot::channel();
        let incoming_transfer = IncomingTransfer {
            sender_address,
            file_name: file_name.clone(),
            total_size: property_size(&transfer_properties, "Size").unwrap_or_default(),
            obex_connection: self.obex_connection.clone(),
            transfer_path,
            reply_channel,
        };
        if self.transfer_sender.send(incoming_transfer).is_err() {
            return Err(PushRefusal::NobodyListening);
        }
        if reply_receiver.await.is_err() {
            return Err(PushRefusal::DeclinedByUser);
        }
        Ok((file_name,))
    }
}

impl IncomingTransfer {
    pub fn accept(self, destination_folder: PathBuf) -> TransferHandle {
        let _sender_gone = self.reply_channel.send(());
        TransferHandle {
            obex_connection: self.obex_connection,
            transfer_path: self.transfer_path,
            file_name: self.file_name,
            total_size: self.total_size,
            received_file_handling: ReceivedFileHandling::MoveInto(destination_folder),
        }
    }

    pub fn reject(self) {}
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
