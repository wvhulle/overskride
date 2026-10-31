use std::collections::HashMap;

use dbus::{arg::Variant, blocking::Connection, channel::Channel};
use dbus_crossroads::{Crossroads, IfaceBuilder, MethodErr};

use crate::fake_bluez_fixture_devices::{
    FIXTURE_ADAPTER_NAME, FakeAdapter, FakeBatteryLevel, FakeDevice, fixture_adapter,
    fixture_devices,
};

const BLUEZ_SERVICE_NAME: &str = "org.bluez";
const BLUEZ_ROOT_PATH: &str = "/org/bluez";
const GENERIC_ACCESS_SERVICE: &str = "00001800-0000-1000-8000-00805f9b34fb";
const GENERIC_ATTRIBUTE_SERVICE: &str = "00001801-0000-1000-8000-00805f9b34fb";

fn adapter_object_path() -> String {
    format!("{BLUEZ_ROOT_PATH}/{FIXTURE_ADAPTER_NAME}")
}

fn register_agent_manager(crossroads: &mut Crossroads) -> dbus_crossroads::IfaceToken<()> {
    crossroads.register(
        "org.bluez.AgentManager1",
        |interface_builder: &mut IfaceBuilder<()>| {
            interface_builder.method(
                "RegisterAgent",
                ("agent", "capability"),
                (),
                |_method_context, _no_data, _agent_registration: (dbus::Path<'static>, String)| {
                    Ok(())
                },
            );
            interface_builder.method(
                "UnregisterAgent",
                ("agent",),
                (),
                |_method_context, _no_data, _agent_path: (dbus::Path<'static>,)| Ok(()),
            );
            interface_builder.method(
                "RequestDefaultAgent",
                ("agent",),
                (),
                |_method_context, _no_data, _agent_path: (dbus::Path<'static>,)| Ok(()),
            );
        },
    )
}

fn register_adapter_interface(
    crossroads: &mut Crossroads,
) -> dbus_crossroads::IfaceToken<FakeAdapter> {
    crossroads.register(
        "org.bluez.Adapter1",
        |interface_builder: &mut IfaceBuilder<FakeAdapter>| {
            interface_builder
                .property("Address")
                .get(|_property_context, adapter| Ok(adapter.address.clone()));
            interface_builder
                .property("AddressType")
                .get(|_property_context, _adapter| Ok("public".to_string()));
            interface_builder
                .property("Name")
                .get(|_property_context, adapter| Ok(adapter.alias.clone()));
            interface_builder
                .property("Alias")
                .get(|_property_context, adapter| Ok(adapter.alias.clone()));
            interface_builder
                .property("Class")
                .get(|_property_context, _adapter| Ok(0x0010_010c_u32));
            interface_builder
                .property("Powered")
                .get(|_property_context, adapter| Ok(adapter.powered));
            interface_builder
                .property("Discoverable")
                .get(|_property_context, adapter| Ok(adapter.discoverable));
            interface_builder
                .property("DiscoverableTimeout")
                .get(|_property_context, adapter| Ok(adapter.discoverable_timeout_seconds));
            interface_builder
                .property("Pairable")
                .get(|_property_context, _adapter| Ok(true));
            interface_builder
                .property("PairableTimeout")
                .get(|_property_context, _adapter| Ok(0_u32));
            interface_builder
                .property("Discovering")
                .get(|_property_context, _adapter| Ok(false));
            interface_builder
                .property("UUIDs")
                .get(|_property_context, _adapter| {
                    Ok(vec![
                        GENERIC_ACCESS_SERVICE.to_string(),
                        GENERIC_ATTRIBUTE_SERVICE.to_string(),
                    ])
                });
            interface_builder
                .property("Modalias")
                .get(|_property_context, _adapter| Ok("usb:v1D6Bp0246d0548".to_string()));
            interface_builder
                .property("Roles")
                .get(|_property_context, _adapter| {
                    Ok(vec!["central".to_string(), "peripheral".to_string()])
                });
            interface_builder.method(
                "StartDiscovery",
                (),
                (),
                |_method_context, _adapter, (): ()| Ok(()),
            );
            interface_builder.method(
                "StopDiscovery",
                (),
                (),
                |_method_context, _adapter, (): ()| Ok(()),
            );
            interface_builder.method(
                "SetDiscoveryFilter",
                ("properties",),
                (),
                |_method_context, _adapter, _discovery_filter: (dbus::arg::PropMap,)| Ok(()),
            );
            interface_builder.method(
                "RemoveDevice",
                ("device",),
                (),
                |_method_context, _adapter, _device_path: (dbus::Path<'static>,)| Ok(()),
            );
        },
    )
}

fn register_device_interface(
    crossroads: &mut Crossroads,
) -> dbus_crossroads::IfaceToken<FakeDevice> {
    crossroads.register(
        "org.bluez.Device1",
        |interface_builder: &mut IfaceBuilder<FakeDevice>| {
            interface_builder
                .property("Address")
                .get(|_property_context, device| Ok(device.address.to_string()));
            interface_builder
                .property("AddressType")
                .get(|_property_context, device| Ok(device.address_type.to_string()));
            interface_builder
                .property("Name")
                .get(|_property_context, device| Ok(device.alias.to_string()));
            interface_builder
                .property("Alias")
                .get(|_property_context, device| Ok(device.alias.to_string()));
            interface_builder
                .property("Icon")
                .get(|_property_context, device| Ok(device.icon_name.to_string()));
            interface_builder
                .property("Class")
                .get(|_property_context, device| Ok(device.device_class));
            interface_builder
                .property("Appearance")
                .get(|_property_context, device| Ok(device.appearance));
            interface_builder
                .property("UUIDs")
                .get(|_property_context, device| {
                    Ok(device
                        .service_uuids
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>())
                });
            interface_builder
                .property("Paired")
                .get(|_property_context, device| Ok(device.paired));
            interface_builder
                .property("Bonded")
                .get(|_property_context, device| Ok(device.bonded));
            interface_builder
                .property("Trusted")
                .get(|_property_context, device| Ok(device.trusted));
            interface_builder
                .property("Blocked")
                .get(|_property_context, _device| Ok(false));
            interface_builder
                .property("Connected")
                .get(|_property_context, device| Ok(device.connected));
            interface_builder
                .property("ServicesResolved")
                .get(|_property_context, device| Ok(device.connected));
            interface_builder
                .property("LegacyPairing")
                .get(|_property_context, _device| Ok(false));
            interface_builder
                .property("WakeAllowed")
                .get(|_property_context, _device| Ok(false));
            interface_builder
                .property("RSSI")
                .get(|_property_context, device| Ok(device.signal_strength_dbm));
            interface_builder
                .property("TxPower")
                .get(|_property_context, _device| Ok(4_i16));
            interface_builder
                .property("Adapter")
                .get(|_property_context, _device| Ok(dbus::Path::from(adapter_object_path())));
            interface_builder
                .property("Modalias")
                .get(|_property_context, _device| Ok("bluetooth:v000Fp1200d1436".to_string()));
            interface_builder
                .property("ManufacturerData")
                .get(|_property_context, device| {
                    Ok(device
                        .manufacturer_data
                        .iter()
                        .map(|(company_identifier, payload_bytes)| {
                            (*company_identifier, Variant(payload_bytes.clone()))
                        })
                        .collect::<HashMap<_, _>>())
                });
            interface_builder
                .property("ServiceData")
                .get(|_property_context, _device| Ok(HashMap::<String, Variant<Vec<u8>>>::new()));
            interface_builder.method("Connect", (), (), |_method_context, _device, (): ()| Ok(()));
            interface_builder.method("Disconnect", (), (), |_method_context, _device, (): ()| {
                Ok(())
            });
            interface_builder.method("Pair", (), (), |_method_context, _device, (): ()| Ok(()));
            interface_builder.method(
                "CancelPairing",
                (),
                (),
                |_method_context, _device, (): ()| Ok(()),
            );
        },
    )
}

fn register_battery_interface(
    crossroads: &mut Crossroads,
) -> dbus_crossroads::IfaceToken<FakeDevice> {
    crossroads.register(
        "org.bluez.Battery1",
        |interface_builder: &mut IfaceBuilder<FakeDevice>| {
            interface_builder
                .property("Percentage")
                .get(|_property_context, device| match device.battery_level {
                    FakeBatteryLevel::Percentage(battery_percentage) => Ok(battery_percentage),
                    FakeBatteryLevel::NotReported => Err(MethodErr::invalid_arg(
                        "No such interface 'org.bluez.Battery1'",
                    )),
                });
        },
    )
}

pub fn serve_fake_bluez(system_bus_address: &str) -> Result<(), dbus::Error> {
    let mut system_bus_channel = Channel::open_private(system_bus_address)?;
    system_bus_channel.register()?;
    let system_bus_connection = Connection::from(system_bus_channel);
    system_bus_connection.request_name(BLUEZ_SERVICE_NAME, false, true, false)?;

    let mut crossroads = Crossroads::new();
    let object_manager = crossroads.object_manager();
    crossroads.insert("/", &[object_manager], ());
    let agent_manager = register_agent_manager(&mut crossroads);
    crossroads.insert(BLUEZ_ROOT_PATH, &[agent_manager], ());
    let adapter_interface = register_adapter_interface(&mut crossroads);
    crossroads.insert(
        adapter_object_path(),
        &[adapter_interface],
        fixture_adapter(),
    );
    let device_interface = register_device_interface(&mut crossroads);
    let battery_interface = register_battery_interface(&mut crossroads);
    for fixture_device in fixture_devices() {
        let device_path = format!(
            "{}/{}",
            adapter_object_path(),
            fixture_device.object_path_segment()
        );
        crossroads.insert(
            device_path,
            &[device_interface, battery_interface],
            fixture_device,
        );
    }
    crossroads.serve(&system_bus_connection)
}
