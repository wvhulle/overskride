use std::{path::PathBuf, sync::Arc, time::Duration};

use dbus::{
    arg::{PropMap, Variant},
    nonblock::{Proxy, SyncConnection},
};

use crate::{
    bluez_operation_error::BluetoothError,
    obex_file_transfer::obex_transfer_progress::{
        ReceivedFileHandling, TransferHandle, property_size, property_text,
    },
};

const OBEX_SERVICE: &str = "org.bluez.obex";
pub(super) const OBEX_MANAGER_PATH: &str = "/org/bluez/obex";
const CLIENT_INTERFACE: &str = "org.bluez.obex.Client1";
const OBJECT_PUSH_INTERFACE: &str = "org.bluez.obex.ObjectPush1";
const OBJECT_PUSH_TARGET: &str = "OPP";
const OBEX_CALL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct ObexConnection {
    pub(super) connection: Arc<SyncConnection>,
}

impl std::fmt::Debug for ObexConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ObexConnection")
    }
}

impl ObexConnection {
    pub fn open() -> Result<Self, BluetoothError> {
        let (connection_resource, connection) = dbus_tokio::connection::new_session_sync()?;
        tokio::spawn(connection_resource);
        Ok(Self { connection })
    }

    pub(super) fn proxy<'path>(
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
