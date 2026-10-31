use std::{collections::BTreeMap, time::Instant};

use bindrune_bluez::{
    link_quality::disconnect_reason_history::{ConnectionDrop, ConnectionHistory},
    remote_device::remote_device_change_stream::DeviceChange,
};

pub struct ChangedConnectionHistory {
    pub address: bindrune_bluez::Address,
    pub connection_history: ConnectionHistory,
}

#[derive(Default)]
pub struct DeviceConnectionHistories {
    connection_histories: BTreeMap<bindrune_bluez::Address, ConnectionHistory>,
}

impl DeviceConnectionHistories {
    pub fn history_of(&self, address: bindrune_bluez::Address) -> ConnectionHistory {
        self.connection_histories
            .get(&address)
            .cloned()
            .unwrap_or_default()
    }

    fn change_history(
        &mut self,
        address: bindrune_bluez::Address,
        change_history: impl FnOnce(&mut ConnectionHistory),
    ) -> Option<ChangedConnectionHistory> {
        let connection_history = self.connection_histories.entry(address).or_default();
        let previous_history = connection_history.clone();
        change_history(connection_history);
        (*connection_history != previous_history).then(|| ChangedConnectionHistory {
            address,
            connection_history: connection_history.clone(),
        })
    }

    pub fn record_device_change(
        &mut self,
        device_change: &DeviceChange,
    ) -> Option<ChangedConnectionHistory> {
        match device_change {
            DeviceChange::Appeared(snapshot) | DeviceChange::Updated(snapshot) => {
                let connected = snapshot.connected;
                self.change_history(snapshot.address(), |connection_history| {
                    if connected {
                        connection_history.record_connected(Instant::now());
                    } else {
                        connection_history.record_disconnected();
                    }
                })
            }
            DeviceChange::Disappeared(address) => {
                self.connection_histories.remove(address);
                None
            }
        }
    }

    pub fn record_connection_drop(
        &mut self,
        connection_drop: ConnectionDrop,
    ) -> Option<ChangedConnectionHistory> {
        self.change_history(connection_drop.address, |connection_history| {
            connection_history.record_drop(connection_drop);
        })
    }
}
