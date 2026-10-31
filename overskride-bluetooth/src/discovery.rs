use futures::StreamExt;

use crate::bluetooth_error::BluetoothError;
use crate::session::BluetoothSession;

#[derive(Debug)]
pub struct DiscoverySession {
    discovery_task: tokio::task::JoinHandle<()>,
}

impl Drop for DiscoverySession {
    fn drop(&mut self) {
        self.discovery_task.abort();
    }
}

pub async fn start_discovery(
    session: &BluetoothSession,
    adapter_name: &str,
) -> Result<DiscoverySession, BluetoothError> {
    let adapter = session.adapter(adapter_name)?;
    adapter
        .set_discovery_filter(bluer::DiscoveryFilter {
            transport: bluer::DiscoveryTransport::Auto,
            ..Default::default()
        })
        .await?;
    let discovery_events = adapter.discover_devices().await?;
    Ok(DiscoverySession {
        discovery_task: tokio::spawn(discovery_events.for_each(|_discovery_event| futures::future::ready(()))),
    })
}
