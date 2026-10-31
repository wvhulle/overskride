use std::collections::HashMap;

use zbus::{
    proxy,
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value},
};

#[proxy(
    interface = "org.bluez.obex.Client1",
    default_service = "org.bluez.obex",
    default_path = "/org/bluez/obex"
)]
pub(super) trait ObexClient {
    fn create_session(
        &self,
        destination_address: &str,
        session_arguments: HashMap<&str, Value<'_>>,
    ) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.bluez.obex.ObjectPush1",
    default_service = "org.bluez.obex"
)]
pub(super) trait ObexObjectPush {
    fn send_file(
        &self,
        source_file: &str,
    ) -> zbus::Result<(OwnedObjectPath, HashMap<String, OwnedValue>)>;
}

#[proxy(
    interface = "org.bluez.obex.Transfer1",
    default_service = "org.bluez.obex"
)]
pub(super) trait ObexTransfer {
    fn cancel(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn status(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn name(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn size(&self) -> zbus::Result<u64>;

    #[zbus(property)]
    fn transferred(&self) -> zbus::Result<u64>;

    #[zbus(property)]
    fn session(&self) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.bluez.obex.Session1",
    default_service = "org.bluez.obex"
)]
pub(super) trait ObexSession {
    #[zbus(property)]
    fn destination(&self) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.bluez.obex.AgentManager1",
    default_service = "org.bluez.obex",
    default_path = "/org/bluez/obex"
)]
pub(super) trait ObexAgentManager {
    fn register_agent(&self, agent_path: &ObjectPath<'_>) -> zbus::Result<()>;

    fn unregister_agent(&self, agent_path: &ObjectPath<'_>) -> zbus::Result<()>;
}
