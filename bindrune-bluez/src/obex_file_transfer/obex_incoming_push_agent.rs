use std::{path::PathBuf, sync::Arc};

use bytesize::ByteSize;
use dbus::{
    arg::PropMap,
    channel::MatchingReceiver,
    message::MatchRule,
    nonblock::{SyncConnection, stdintf::org_freedesktop_dbus::Properties},
};
use dbus_crossroads::{Crossroads, MethodErr};
use futures::Stream;
use tokio::sync::{mpsc, oneshot};

use crate::{
    bluez_operation_error::BluetoothError,
    obex_file_transfer::{
        obex_dbus_connection::{OBEX_MANAGER_PATH, ObexConnection},
        obex_transfer_progress::{
            ReceivedFileHandling, TRANSFER_INTERFACE, TransferHandle, property_size, property_text,
        },
    },
};

const AGENT_OBJECT_PATH: &str = "/overskride/agent";
const AGENT_INTERFACE: &str = "org.bluez.obex.Agent1";
const AGENT_MANAGER_INTERFACE: &str = "org.bluez.obex.AgentManager1";
const SESSION_INTERFACE: &str = "org.bluez.obex.Session1";

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

impl ObexConnection {
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
