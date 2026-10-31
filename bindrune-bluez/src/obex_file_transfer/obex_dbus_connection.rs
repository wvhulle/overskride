use std::{collections::HashMap, path::PathBuf};

use bytesize::ByteSize;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::{
    bluez_operation_error::BluetoothError,
    obex_file_transfer::{
        obex_dbus_proxies::{ObexClientProxy, ObexObjectPushProxy},
        obex_transfer_progress::{ReceivedFileHandling, TransferHandle},
    },
};

const OBJECT_PUSH_TARGET: &str = "OPP";

#[derive(Clone)]
pub struct ObexConnection {
    pub(super) connection: zbus::Connection,
}

impl std::fmt::Debug for ObexConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ObexConnection")
    }
}

impl ObexConnection {
    pub async fn open() -> Result<Self, BluetoothError> {
        Ok(Self {
            connection: zbus::Connection::session().await?,
        })
    }

    pub async fn send_files(
        &self,
        source_address: bluer::Address,
        destination_address: bluer::Address,
        file_paths: Vec<PathBuf>,
    ) -> Result<Vec<TransferHandle>, BluetoothError> {
        let source_text = source_address.to_string();
        let session_arguments = HashMap::from([
            ("Target", Value::from(OBJECT_PUSH_TARGET)),
            ("Source", Value::from(source_text.as_str())),
        ]);
        let session_path = ObexClientProxy::new(&self.connection)
            .await?
            .create_session(&destination_address.to_string(), session_arguments)
            .await?;
        let transfer_starts = file_paths
            .into_iter()
            .map(|file_path| self.send_file(&session_path, file_path));
        futures::future::try_join_all(transfer_starts).await
    }

    async fn send_file(
        &self,
        session_path: &OwnedObjectPath,
        file_path: PathBuf,
    ) -> Result<TransferHandle, BluetoothError> {
        let (transfer_path, transfer_properties) =
            ObexObjectPushProxy::new(&self.connection, session_path)
                .await?
                .send_file(&file_path.to_string_lossy())
                .await?;
        let fallback_name = file_path
            .file_name()
            .map(|file_name| file_name.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(TransferHandle {
            obex_connection: self.clone(),
            transfer_path,
            file_name: property_value(&transfer_properties, "Name").unwrap_or(fallback_name),
            total_size: property_value(&transfer_properties, "Size")
                .map(ByteSize::b)
                .unwrap_or_default(),
            received_file_handling: ReceivedFileHandling::LeaveWhereObexdPutIt,
        })
    }
}

fn property_value<PropertyValue>(
    properties: &HashMap<String, OwnedValue>,
    property_name: &str,
) -> Option<PropertyValue>
where
    PropertyValue: TryFrom<OwnedValue>,
{
    properties
        .get(property_name)?
        .try_clone()
        .ok()?
        .try_into()
        .ok()
}
