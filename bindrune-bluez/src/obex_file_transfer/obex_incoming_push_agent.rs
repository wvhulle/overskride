use std::path::PathBuf;

use bytesize::ByteSize;
use futures::Stream;
use tokio::sync::{mpsc, oneshot};
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

use crate::{
    bluez_operation_error::BluetoothError,
    obex_file_transfer::{
        obex_dbus_connection::ObexConnection,
        obex_dbus_proxies::{ObexAgentManagerProxy, ObexSessionProxy, ObexTransferProxy},
        obex_transfer_progress::{ReceivedFileHandling, TransferHandle},
    },
};

const AGENT_OBJECT_PATH: ObjectPath<'static> =
    ObjectPath::from_static_str_unchecked("/bindrune/agent");

#[derive(Debug)]
pub struct IncomingTransfer {
    pub sender_address: bluer::Address,
    pub file_name: String,
    pub total_size: ByteSize,
    obex_connection: ObexConnection,
    transfer_path: OwnedObjectPath,
    reply_channel: oneshot::Sender<()>,
}

struct PushForwarder {
    transfer_sender: mpsc::UnboundedSender<IncomingTransfer>,
    obex_connection: ObexConnection,
}

#[zbus::interface(name = "org.bluez.obex.Agent1")]
impl PushForwarder {
    async fn authorize_push(&self, transfer: OwnedObjectPath) -> Result<String, AgentReplyError> {
        Ok(self.forward_push(transfer).await?)
    }

    fn cancel(&self) {}

    fn release(&self) {}
}

impl ObexConnection {
    pub async fn incoming_transfers(
        &self,
    ) -> Result<impl Stream<Item = IncomingTransfer> + Send + 'static, BluetoothError> {
        let (transfer_sender, mut transfer_receiver) = mpsc::unbounded_channel();
        self.connection
            .object_server()
            .at(
                &AGENT_OBJECT_PATH,
                PushForwarder {
                    transfer_sender,
                    obex_connection: self.clone(),
                },
            )
            .await?;
        let agent_registration = AgentRegistration {
            obex_connection: self.clone(),
        };
        ObexAgentManagerProxy::new(&self.connection)
            .await?
            .register_agent(&AGENT_OBJECT_PATH)
            .await?;
        Ok(async_stream::stream! {
            let _agent_registration = agent_registration;
            while let Some(incoming_transfer) = transfer_receiver.recv().await {
                yield incoming_transfer;
            }
        })
    }
}

struct AgentRegistration {
    obex_connection: ObexConnection,
}

impl Drop for AgentRegistration {
    fn drop(&mut self) {
        let unregistering_connection = self.obex_connection.connection.clone();
        tokio::spawn(async move {
            let _already_removed: zbus::Result<bool> = unregistering_connection
                .object_server()
                .remove::<PushForwarder, _>(&AGENT_OBJECT_PATH)
                .await;
            let Ok(agent_manager) = ObexAgentManagerProxy::new(&unregistering_connection).await
            else {
                return;
            };
            let _already_unregistered = agent_manager.unregister_agent(&AGENT_OBJECT_PATH).await;
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

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.obex.Error")]
enum AgentReplyError {
    Failed(String),
    Canceled(String),
    Rejected(String),
}

impl From<PushRefusal> for AgentReplyError {
    fn from(push_refusal: PushRefusal) -> Self {
        match push_refusal {
            PushRefusal::TransferUnreadable => {
                Self::Failed("transfer properties are unreadable".to_string())
            }
            PushRefusal::TransferWithoutName => Self::Failed("transfer has no name".to_string()),
            PushRefusal::TransferWithoutSession => {
                Self::Failed("transfer has no session".to_string())
            }
            PushRefusal::SessionWithoutSender => {
                Self::Failed("session has no sender address".to_string())
            }
            PushRefusal::NobodyListening => {
                Self::Canceled("nobody listens for incoming transfers".to_string())
            }
            PushRefusal::DeclinedByUser => Self::Rejected("Not Authorized".to_string()),
        }
    }
}

impl PushForwarder {
    async fn forward_push(&self, transfer_path: OwnedObjectPath) -> Result<String, PushRefusal> {
        let connection = &self.obex_connection.connection;
        let transfer_proxy = ObexTransferProxy::new(connection, transfer_path.clone())
            .await
            .map_err(|_unreachable_transfer| PushRefusal::TransferUnreadable)?;
        let file_name = transfer_proxy
            .name()
            .await
            .map_err(|_missing_name| PushRefusal::TransferWithoutName)?;
        let session_path = transfer_proxy
            .session()
            .await
            .map_err(|_missing_session| PushRefusal::TransferWithoutSession)?;
        let session_proxy = ObexSessionProxy::new(connection, session_path)
            .await
            .map_err(|_unreachable_session| PushRefusal::TransferWithoutSession)?;
        let Some(sender_address) = session_proxy
            .destination()
            .await
            .ok()
            .and_then(|destination_text| destination_text.parse().ok())
        else {
            return Err(PushRefusal::SessionWithoutSender);
        };
        let (reply_channel, reply_receiver) = oneshot::channel();
        let incoming_transfer = IncomingTransfer {
            sender_address,
            file_name: file_name.clone(),
            total_size: transfer_proxy
                .size()
                .await
                .map(ByteSize::b)
                .unwrap_or_default(),
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
        Ok(file_name)
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
