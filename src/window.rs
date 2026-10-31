/* window.rs
*
 * Copyright 2023 kaii
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::gio::Settings;
use gtk::glib::clone;
use gtk::glib::SignalHandlerId;
use gtk::{gio, glib, Accessible, Buildable, ConstraintTarget, Grid, Native, ResponseType, Root, ShortcutManager, Widget, Window};

use crate::agent::register_bluetooth_agent;
use crate::application::OverskrideApplication;
use crate::audio_profiles;
use crate::bluetooth_settings::get_store_location_from_dialog;
use crate::bluetooth_device::{BluetoothDevice, DeviceSnapshot, UNKNOWN_DEVICE_NAME};
use crate::message::Message;

use crate::pairing_timeline::{PairingStepAction, PairingTimeline, ServiceAuthorizationAnswer};
use crate::obex::{self, register_obex_agent};
use crate::receiving_row::ReceivingRow;
use crate::selectable_row::SelectableRow;
use crate::services::get_name_from_service;
use crate::startup_error_message::StartupErrorMessage;
use crate::{bluetooth_settings, connected_switch_row::ConnectedSwitchRow, device};
use crate::Singletons::OverskrideProperties;

use adw::glib::wrapper;
use async_channel::Sender;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use tokio::runtime::Runtime;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref OVERSKRIDE_PROPS: Mutex<OverskrideProperties> = Mutex::new(OverskrideProperties::new());
}

// U N S A F E T Y
static mut RSSI_LUT: Option<HashMap<String, i32>> = None;
pub static mut ADAPTERS_LUT: Option<HashMap<String, String>> = None;

mod imp {
    use crate::{
        battery_indicator::BatteryLevelIndicator, receiving_popover::ReceivingPopover,
        receiving_row::ReceivingRow,
    };

    use super::*;
    #[derive(Debug)]
    pub struct DeviceListModels {
        pub store: gio::ListStore,
        pub filter: gtk::CustomFilter,
        pub sorter: gtk::CustomSorter,
        pub selection: gtk::SingleSelection,
    }

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/io/github/kaii_lb/Overskride/gtk/window.ui")]
    #[properties(wrapper_type = super::OverskrideWindow)]
    pub struct OverskrideWindow {
        #[template_child]
        pub device_list_view: TemplateChild<gtk::ListView>,
        #[template_child]
        pub connected_switch_row: TemplateChild<ConnectedSwitchRow>,
        #[template_child]
        pub device_name_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub pairing_timeline: TemplateChild<PairingTimeline>,
        #[template_child]
        pub scan_toggle_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub scan_spinner: TemplateChild<gtk::Spinner>,
        #[template_child]
        pub radio_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub radio_combo_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub connection_properties_group: TemplateChild<adw::PreferencesGroup>,
        
        
        #[template_child]
        pub bluetooth_settings_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub device_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub main_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub secondary_listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub powered_switch_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub discoverable_switch_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub adapter_name_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub timeout_time_adjustment: TemplateChild<gtk::Adjustment>,
        #[template_child]
        pub default_controller_expander: TemplateChild<adw::ExpanderRow>,
        #[template_child]
        pub split_view: TemplateChild<adw::OverlaySplitView>,
        #[template_child]
        pub show_sidebar_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub listbox_image_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub receiving_popover: TemplateChild<ReceivingPopover>,
        #[template_child]
        pub choose_file_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub send_file_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub file_save_location: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub choose_location_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub auto_accept_trusted_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub sidebar_content_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub audio_profile_expander: TemplateChild<adw::ExpanderRow>,
        #[template_child]
        pub address_detail_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub manufacturer_detail_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub type_detail_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub distance_detail_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub services_detail_row: TemplateChild<adw::ExpanderRow>,
        
        #[template_child]
        pub window_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub hide_unknowns_switch_row: TemplateChild<adw::SwitchRow>,

        pub settings: OnceCell<Settings>,
        pub display_pass_key_dialog: RefCell<Option<adw::MessageDialog>>,
        pub index: RefCell<u32>,
        pub timeout_signal_id: OnceCell<SignalHandlerId>,
        pub powered_signal_id: OnceCell<SignalHandlerId>,
        pub discoverable_signal_id: OnceCell<SignalHandlerId>,
        pub radio_signal_id: OnceCell<SignalHandlerId>,
        pub radio_adapter_names: RefCell<Vec<String>>,
        pub pending_device_selection: RefCell<Option<bluer::Address>>,
        pub pending_service_authorization: RefCell<Option<(bluer::Address, String, String)>>,
        pub service_detail_rows: RefCell<Vec<adw::ActionRow>>,
        pub device_list_models: OnceCell<DeviceListModels>,

        #[property(get, set, nullable)]
        pub selected_device: RefCell<Option<BluetoothDevice>>,
        #[property(get, set)]
        pub device_rename_pending: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OverskrideWindow {
        const NAME: &'static str = "OverskrideWindow";
        type Type = super::OverskrideWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            ConnectedSwitchRow::ensure_type();
            ReceivingPopover::ensure_type();
            ReceivingRow::ensure_type();
            SelectableRow::ensure_type();
            BatteryLevelIndicator::ensure_type();
            
            PairingTimeline::ensure_type();
            BluetoothDevice::ensure_type();

            klass.bind_template();
            /*klass.install_action("win.refresh_devices", None, move |win, _, _| {

            });*/
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for OverskrideWindow {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            obj.setup_settings();
            obj.preload_settings();
        }
    }
    impl WidgetImpl for OverskrideWindow {}

    impl WindowImpl for OverskrideWindow {
        fn close_request(&self) -> glib::Propagation {
            self.obj().save_settings().expect("cannot save window size");

            glib::Propagation::Proceed
        }
    }
    impl ApplicationWindowImpl for OverskrideWindow {}
    impl AdwApplicationWindowImpl for OverskrideWindow {}
}

wrapper! {
    pub struct OverskrideWindow(ObjectSubclass<imp::OverskrideWindow>)
        @extends Widget, adw::ApplicationWindow, OverskrideApplication,
        @implements gio::ActionGroup, gio::ActionMap, Accessible, Buildable, ConstraintTarget, Native, Root, ShortcutManager, gtk::ApplicationWindow, Grid, Window;
}

pub fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Runtime::new().expect("Setting up tokio runtime needs to succeed.")
    })
}

impl OverskrideWindow {
    pub fn new<P: IsA<gtk::Application>>(application: &P) -> Self {
        //glib::Object::builder()
        //    .property("application", application)
        //  .build();

        let win: OverskrideWindow = glib::Object::builder()
            .property("application", application)
            .build();

        win.setup();

        win
    }

    fn setup_settings(&self) {
        let settings = Settings::new("io.github.kaii_lb.Overskride");
        self.imp()
            .settings
            .set(settings)
            .expect("settings not setup");
    }

    /// Sets up the application. Basically it binds actions to stuff and updates what needs to be updated.
    fn setup(&self) {
        let (sender, receiver) = async_channel::unbounded::<Message>();

        // if pre setup is an error, get the hell out, show error to user
        // suggest solutions
        // then profit
        if let Err(err) = self.pre_setup(sender.clone()) {
            println!("ERROR: cannot start presetup, something got REALLY fucked");
            println!("error is: {:?}", err);

            let clone = self.clone();
            let message = StartupErrorMessage::new();

            message.set_transient_for(Some(&clone));
            message.set_modal(true);

            // clone.set_sensitive(false);
            message.connect_destroy(move |_| {
                WidgetExt::activate_action(&clone, "app.quit", None)
                    .expect("cannot exit app on message close");
            });

            message.set_visible(true);
            return;
        }

        let sender_for_receiver_clone = sender.clone();
        let self_clone = self.clone();


        glib::MainContext::default().spawn_local(async move {
            while let Ok(msg) = receiver.recv().await {
                let clone = self_clone.clone();

                match msg {
                    Message::SwitchPairingStages(address, stages) => {
                        if let Some(bluetooth_device) = clone.find_device_by_address(address) {
                            bluetooth_device.apply_pairing_stages(&stages);
                        }

                        if address == OVERSKRIDE_PROPS.lock().unwrap().address {
                            clone.imp().pairing_timeline.get().render(&stages);
                            clone.show_pending_service_authorization_if_open(address);
                        }
                    }
                    Message::PopulateRadios(radios) => {
                        let radio_combo_row = clone.imp().radio_combo_row.get();
                        let radio_labels: Vec<&str> = radios.iter().map(|radio| radio.radio_label.as_str()).collect();
                        let current_adapter = OVERSKRIDE_PROPS.lock().unwrap().current_adapter.clone();
                        let current_position = radios.iter().position(|radio| radio.adapter_name == current_adapter).unwrap_or(0);

                        if let Some(radio_signal_id) = clone.imp().radio_signal_id.get() {
                            radio_combo_row.block_signal(radio_signal_id);
                        }
                        radio_combo_row.set_model(Some(&gtk::StringList::new(&radio_labels)));
                        radio_combo_row.set_selected(current_position as u32);
                        if let Some(radio_signal_id) = clone.imp().radio_signal_id.get() {
                            radio_combo_row.unblock_signal(radio_signal_id);
                        }

                        clone.imp().radio_group.get().set_visible(radios.len() > 1);
                        *clone.imp().radio_adapter_names.borrow_mut() = radios.into_iter().map(|radio| radio.adapter_name).collect();
                    }
                    Message::SwitchScanning(scanning) => {
                        clone.imp().scan_toggle_button.get().set_active(scanning);
                        clone.imp().scan_spinner.get().set_spinning(scanning);
                    }
                    Message::SwitchDeviceDetails(address, device_address, manufacturer, device_type, distance, services_list) => {
                        if address == OVERSKRIDE_PROPS.lock().unwrap().address {
                            clone.imp().address_detail_row.get().set_subtitle(&device_address);
                            clone.imp().manufacturer_detail_row.get().set_subtitle(&glib::markup_escape_text(&manufacturer));
                            clone.imp().type_detail_row.get().set_subtitle(&glib::markup_escape_text(&device_type));
                            clone.imp().distance_detail_row.get().set_subtitle(&glib::markup_escape_text(&distance));

                            let services_detail_row = clone.imp().services_detail_row.get();
                            for service_detail_row in clone.imp().service_detail_rows.borrow_mut().drain(..) {
                                services_detail_row.remove(&service_detail_row);
                            }
                            services_detail_row.set_subtitle(&format!("{} offered", services_list.len()));
                            let mut service_detail_rows = Vec::new();
                            for service_name in services_list {
                                let service_detail_row = adw::ActionRow::builder().title(glib::markup_escape_text(&service_name).as_str()).build();
                                services_detail_row.add_row(&service_detail_row);
                                service_detail_rows.push(service_detail_row);
                            }
                            *clone.imp().service_detail_rows.borrow_mut() = service_detail_rows;
                        }
                    }
                    Message::ExpireServiceAuthorization(address) => {
                        let expired_request = clone.imp().pending_service_authorization.borrow_mut().take();
                        if expired_request.is_some_and(|(pending_address, _, _)| pending_address == address) {
                            clone.imp().pairing_timeline.get().clear_service_authorization_request();
                            sender_for_receiver_clone.send(Message::PopupError("Connection request expired and was denied.".to_string(), adw::ToastPriority::Normal)).await.expect("cannot send message");
                        }
                    }
                    Message::SwitchActive(active, address, is_current) => {
                        if is_current {
                            let connected_switch_row = clone.imp().connected_switch_row.get();
                            connected_switch_row.set_toggle_switch_active(active);

                            // is this redundant? we'll never know
                            if connected_switch_row.row_has_obex() {
                                sender_for_receiver_clone.clone().send(Message::SwitchSendFileActive(active)).await.expect("cannot send message");
                            } else {
                                sender_for_receiver_clone.clone().send(Message::SwitchSendFileActive(false)).await.expect("cannot send message");
                            }
                        }

                        let device_store = clone.device_list_models().store.clone();
                        device_store
                            .iter::<BluetoothDevice>()
                            .filter_map(Result::ok)
                            .filter(|bluetooth_device| address == bluer::Address::any() || bluetooth_device.bluer_address() == address)
                            .for_each(|bluetooth_device| bluetooth_device.set_connected(active && address != bluer::Address::any()));
                    }
                    Message::SwitchActiveSpinner(spinning) => {
                        let connected_switch_row = clone.imp().connected_switch_row.get();

                        connected_switch_row.set_row_spinning(spinning);
                    }
                    Message::SwitchDeviceAlias(address, alias) => {
                        if let Some(bluetooth_device) = clone.find_device_by_address(address) {
                            bluetooth_device.set_alias(alias);
                        }
                    }
                    Message::SwitchDeviceRssi(address, rssi) => {
                        if let Some(bluetooth_device) = clone.find_device_by_address(address) {
                            bluetooth_device.set_rssi(rssi);
                        }
                    }
                    Message::AddDevice(device_snapshot) => {
                        clone.insert_or_update_device(&device_snapshot);
                    }
                    Message::RemoveDevice(address) => {
                        let removed_device_was_selected = clone.selected_device().is_some_and(|bluetooth_device| bluetooth_device.bluer_address() == address);
                        let device_store = clone.device_list_models().store.clone();
                        let removed_device_position = device_store
                            .iter::<BluetoothDevice>()
                            .filter_map(Result::ok)
                            .position(|bluetooth_device| bluetooth_device.bluer_address() == address);
                        if let Some(removed_device_position) = removed_device_position {
                            device_store.remove(removed_device_position as u32);
                        }

                        if removed_device_was_selected {
                            clone.imp().bluetooth_settings_row.get().emit_activate();
                        }
                    }
                    Message::SwitchDeviceIcon(icon_name) => {
                        clone.show_device_icon(&icon_name);
                    }
                    Message::SwitchPage(icon_name) => {
                        clone.show_device_icon(&icon_name);

                        let secondary_listbox = clone.imp().secondary_listbox.get();
                        secondary_listbox.unselect_all();

                        let main_stack = clone.imp().main_stack.get();
                        let pages = main_stack.pages();
                        pages.select_item(0, true);

                        let split_view = clone.imp().split_view.get();
                        if split_view.is_collapsed() {
                            split_view.set_show_sidebar(false);
                        }

                        let window_title = clone.imp().window_title.get();
                        window_title.set_title("Device");
                    }
                    Message::SwitchAdapterPowered(powered) => {
                        let powered_switch_row = clone.imp().powered_switch_row.get();
                        let id = clone.imp().powered_signal_id.get().expect("cannot get signal id");
                        powered_switch_row.block_signal(id);
                        powered_switch_row.set_active(powered);
                        powered_switch_row.unblock_signal(id);
                    }
                    Message::SwitchAdapterDiscoverable(discoverable) => {
                        let discoverable_switch_row = clone.imp().discoverable_switch_row.get();
                        let id = clone.imp().discoverable_signal_id.get().expect("cannot get signal id");
                        discoverable_switch_row.block_signal(id);
                        discoverable_switch_row.set_active(discoverable);
                        discoverable_switch_row.unblock_signal(id);
                    }
                    Message::SwitchAdapterName(new_alias, old_alias) => {
                        let default_controller_expander = clone.imp().default_controller_expander.get();
                        let listbox = default_controller_expander.last_child().unwrap().downcast::<gtk::Box>().unwrap();
                        let revealer = listbox.last_child().unwrap().downcast::<gtk::Revealer>().unwrap();

                        let listbox = revealer.last_child().unwrap().downcast::<gtk::ListBox>().unwrap();

                        // loop over all adapter rows and change the alias to the new one
                        // alias is not the same as name, alias: "laptop 1", name: "hci0"
                        let mut index = 0;
                        while let Some(row) = listbox.row_at_index(index) {
                            let action_row = row.downcast::<adw::ActionRow>().expect("cannot downcast to action row.");

                            if action_row.title() == old_alias {
                                action_row.set_title(new_alias.as_str());
                            }
                            index += 1;
                        }
                        let adapter_name_entry = clone.imp().adapter_name_entry.get();

                        adapter_name_entry.set_text(new_alias.as_str());
                    }
                    Message::SwitchAdapterTimeout(timeout) => {
                        let timeout_time_adjustment = clone.imp().timeout_time_adjustment.get();
                        timeout_time_adjustment.block_signal(clone.imp().timeout_signal_id.get().expect("cannot get signal id"));
                        timeout_time_adjustment.set_value(timeout as f64);
                        timeout_time_adjustment.unblock_signal(clone.imp().timeout_signal_id.get().expect("cannot get signal id"));
                    }
                    Message::PopulateAdapterExpander(hashmap) => {
                        let default_controller_expander = clone.imp().default_controller_expander.get();
                        let listbox = default_controller_expander.last_child().unwrap().downcast::<gtk::Box>().unwrap()
                            .last_child().unwrap().downcast::<gtk::Revealer>().unwrap().last_child().unwrap().downcast::<gtk::ListBox>();

                        // remove all rows in expander
                        if listbox.clone().is_ok() {
                            while let Some(supposed_row) = listbox.clone().unwrap().last_child() {
                                listbox.clone().unwrap().remove(&supposed_row);
                            }
                        }

                        let adapter_aliases: Vec<String> = hashmap.clone().keys().cloned().collect();

                        // create a new row for each adapter and add it to the expander
                        let hashmap_clone = hashmap.clone();
                        for alias in adapter_aliases.clone() {
                            let row = SelectableRow::new();
                            let val = hashmap_clone.get(&alias).cloned();
                            let holder = OVERSKRIDE_PROPS.lock().unwrap().name.to_string();

                            let name = val.clone().unwrap_or(holder);
                            //println!("name is {}", name.clone());
                            //println!("alias is {}", alias.clone());

                            row.set_title(alias.as_str());

                            if OVERSKRIDE_PROPS.lock().unwrap().current_adapter == name.clone() {
                                row.set_selected(true);
                            } else {
                                row.set_selected(false);
                            }

                            let listbox_clone = listbox.clone();
                            let sender_clone = sender_for_receiver_clone.clone();

                            // on row click, set the current adapter to this one and refresh the devices list
                            row.set_activatable(true);
                            row.connect_activated(move |row| {
                                let listbox_clone = listbox_clone.clone();
                                let sender_clone = sender_clone.clone();
                                let name = name.clone();
                                let row_to_set_selected = row.clone();

                                glib::spawn_future_local(async move {
                                    // should move this to the sender message of the audio expander
                                    let mut index = 0;
                                    if listbox_clone.clone().is_ok() {
                                        while let Some(row_to_set_selected) = listbox_clone.clone().unwrap().row_at_index(index) {
                                            //println!("{}", index);
                                            let action_row = row_to_set_selected.downcast::<SelectableRow>().expect("cannot downcast to action row.");
                                            //println!("{:?}", action_row.clone().title());
                                            action_row.set_selected(false);
                                            index += 1;
                                        }
                                    }

                                    OVERSKRIDE_PROPS.lock().unwrap().current_adapter = name.to_string();
                                    println!("current adapter name is: {}", OVERSKRIDE_PROPS.lock().unwrap().current_adapter.clone());

                                    if sender_clone.send(Message::RefreshDevicesList()).await.is_err() {
                                        sender_clone.send(Message::PopupError("bt-refresh-adapter-failed".to_string(), adw::ToastPriority::High)).await.expect("cannot send message");
                                    }
                                    row_to_set_selected.set_selected(true);
                                });
                            });

                            default_controller_expander.add_row(&row);
                        }
                    }
                    Message::PopupError(string, priority) => {
                        let toast_overlay = clone.imp().toast_overlay.get();
                        let toast = adw::Toast::new("");

                        toast.set_priority(priority);

                        // best practices out the window :D
                        // need to ~hashmap~ this shit later
                        let readable_message = glib::markup_escape_text(&string);
                        let title_holder = match string {
                            s if s.to_lowercase().contains("page-timeout") => {
                                "Failed to connect to device, connection timed out"
                            }
                            s if s.to_lowercase().contains("already-connected") => {
                                "Device is already connected"
                            }
                            s if s.to_lowercase().contains("profile-unavailable") => {
                                "Failed to find the target profile"
                            }
                            s if s.to_lowercase().contains("create-socket") => {
                                "Failed to connect to Bluetooth socket, this is bad"
                            }
                            s if s.to_lowercase().contains("bad-socket") => {
                                "Bad socket for connection, this is bad"
                            }
                            s if s.to_lowercase().contains("memory-allocation") => {
                                "Failed to allocate memory"
                            }
                            s if s.to_lowercase().contains("busy") => {
                                "Other operations pending, please try again in a bit"
                            }
                            s if s.to_lowercase().contains("limit") => {
                                "Reached limit, cannot connect to anymore devices"
                            }
                            s if s.to_lowercase().contains("connection-timeout") => {
                                "Failed to connect to device, connection timed out"
                            }
                            s if s.to_lowercase().contains("refused") => {
                                "Connection was refused by target device"
                            }
                            s if s.to_lowercase().contains("aborted-by-remote") => {
                                "Target device aborted connection"
                            }
                            s if s.to_lowercase().contains("aborted-by-local") => {
                                "Connection has been aborted"
                            }
                            s if s.to_lowercase().contains("lmp-protocol-error") => {
                                "Connection failed, lmp protocol error"
                            }
                            s if s.to_lowercase().contains("canceled") => {
                                "Connection was canceled due to unforeseen circumstances"
                            }
                            s if s.to_lowercase().contains("unknown-error") => {
                                "Connection failed, no idea why tho"
                            }
                            s if s.to_lowercase().contains("invalid-arguments") => {
                                "Invalid arguments provided"
                            }
                            s if s.to_lowercase().contains("not-powered") || s.to_lowercase().contains("not powered") || s.to_lowercase().contains("resource not ready") => {
                                "Adapter is not powered"
                            }
                            s if s.to_lowercase().contains("not-supported") => {
                                "Connection failed, requested features are not supported"
                            }
                            s if s.to_lowercase().contains("layer-protocol-error") => {
                                "Connection failed, layer protocol error"
                            }
                            s if s.to_lowercase().contains("gatt-browsing") => {
                                "Failed to complete GATT service browsing"
                            }
                            s if s.to_lowercase().contains("refreshed") => {
                                "Refreshed devices list"
                            }
                            s if s.to_lowercase().contains("stopped searching for devices") => {
                                "Stopped Searching for devices"
                            }
                            s if s.to_lowercase().contains("connection-unknown") => {
                                "Connection unknown, please try again"
                            }
                            s if s.to_lowercase().contains("home-unknown") => {
                                "Unable to get home folder, are you sure its configured correctly?"
                            }
                            s if s.to_lowercase().contains("transfer-complete-inbound") => {
                                "File has been received"
                            }
                            s if s.to_lowercase().contains("transfer-complete-outbound") => {
                                "File has been transferred"
                            }
                            s if s.to_lowercase().contains("transfer-active-inbound") => {
                                "Started receiving file"
                            }
                            s if s.to_lowercase().contains("transfer-active-outbound") => {
                                "Started transferring file"
                            }
                            s if s.to_lowercase().contains("transfer-error-inbound") => {
                                "Receiving file stopped, error occurred"
                            }
                            s if s.to_lowercase().contains("transfer-error-outbound") => {
                                "Sending file stopped, error occurred"
                            }
                            s if s.to_lowercase().contains("transfer-not-authorized") => {
                                "File transfer has been rejected"
                            }
                            s if s.to_lowercase().contains("transfer-cancel-not-authorized") => {
                                "Unable to cancel file transfer"
                            }
                            s if s.to_lowercase().contains("transfer-connection-error") => {
                                "Unable to send file, connection is not possible"
                            }
                            s if s.to_lowercase().contains("refresh-adapter-failed") => {
                                "Unable to refresh devices list after adapter change"
                            }
                            s if s.to_lowercase().contains("file-storage-not-valid") => {
                                "Location is not valid, please try again"
                            }
                            s if s.to_lowercase().contains("file-storage-cache-invalid") => {
                                "File cache location is invalid, are you sure ~/.cache (or equivalent) exists?"
                            }
                            s if s.to_lowercase().contains("device-name-exists") => {
                                "Error, device with name already exists"
                            }
                            e => {
                                println!("unknown error: {}", e.clone());
                                if e.contains(' ') {
                                    readable_message.as_str()
                                } else {
                                    "Unknown error occurred"
                                }
                            }
                        };

                        let mut title = String::new();
                        let boxholder = gtk::Box::new(gtk::Orientation::Horizontal, 8);

                        toast.set_timeout(3);
                        match priority {
                            adw::ToastPriority::High => {
                                // custom_title.set_css_classes(&["warning", state.as_str()]);
                                title += "<span font_weight='bold'>";

                                let icon = gtk::Image::new();
                                icon.set_icon_name(Some("bell-outline-symbolic"));
                                boxholder.append(&icon);
                            }
                            _ => {
                                title += "<span font_weight='regular'>";
                            }
                        }
                        let label = gtk::Label::new(Some(""));
                        boxholder.append(&label);

                        title += title_holder;
                        title += "</span>";

                        label.set_use_markup(true);
                        label.set_label(&title);

                        toast.set_custom_title(Some(&boxholder));

                        toast_overlay.add_toast(toast);
                    }
                    Message::RequestPinCode(request) => {
                        let device: String;
                        let adapter: String;
                        unsafe {
                            device = clone.device_display_name(request.device);
                            adapter = ADAPTERS_LUT.clone().unwrap().get(&request.adapter).unwrap_or(&"Unknown Adapter".to_string()).to_string();
                            OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;
                        }

                        let body = device + "has requested pairing on " + adapter.as_str() + ", please enter the correct pin code.";
                        let popup = adw::MessageDialog::new(Some(&clone), Some("Pin Code Requested"), Some(body.as_str()));
                        let popup2 = popup.clone();

                        // popup.set_modal(true);
                        popup.set_destroy_with_parent(true);

                        popup.add_response("cancel", "Cancel");
                        popup.add_response("confirm", "Confirm");
                        popup.set_response_appearance("confirm", adw::ResponseAppearance::Suggested);
                        popup.set_default_response(Some("confirm"));
                        popup.set_close_response("cancel");

                        let entry = gtk::Entry::new();
                        entry.set_placeholder_text(Some("12345 or abcde"));
                        popup.set_extra_child(Some(&entry));
                        popup.set_response_enabled("confirm", false);

                        entry.connect_changed(move |entry| {
                            let is_empty = entry.text().is_empty();

                            popup.set_response_enabled("confirm", !is_empty);

                            if is_empty {
                                entry.add_css_class("error");
                            } else {
                                entry.remove_css_class("error");
                            }
                        });
                        entry.add_css_class("error");

                        let pin_code = Rc::new(RefCell::new(String::new()));
                        popup2.clone().choose(gio::Cancellable::NONE, move |response| {
                            match response.to_string() {
                                s if s.contains("confirm") => {
                                    *pin_code.borrow_mut() = entry.text().to_string();
                                }
                                _ => {
                                    *pin_code.borrow_mut() = String::new();
                                }
                            }
                            OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = false;
                            OVERSKRIDE_PROPS.lock().unwrap().pin_code = pin_code.borrow().clone();
                        });
                    }
                    Message::DisplayPinCode(request) => {
                        let pin_code = &request.pincode;
                        let device = clone.device_display_name(request.device);
                        OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;

                        let body = "Please enter this pin code on ".to_string() + device.as_str();
                        let popup = adw::MessageDialog::new(Some(&clone), None, Some(body.as_str()));

                        let label = gtk::Label::new(Some(pin_code.as_str()));

                        popup.set_extra_child(Some(&label));
                        popup.add_response("okay", "Okay");
                        popup.set_close_response("okay");

                        popup.clone().choose(
                            gio::Cancellable::NONE, move |_| {
                                OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = false;
                            });
                    }
                    Message::RequestPassKey(request) => {
                        let device: String;
                        let adapter: String;
                        unsafe {
                            device = clone.device_display_name(request.device);
                            adapter = ADAPTERS_LUT.clone().unwrap().get(&request.adapter).unwrap_or(&"Unknown Adapter".to_string()).to_string();
                            OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;
                        }

                        let body = device + "has requested pairing on " + adapter.as_str() + ", please enter the correct pass key.";
                        let popup = adw::MessageDialog::new(Some(&clone), Some("Pass Key Requested"), Some(body.as_str()));
                        let popup2 = popup.clone();

                        popup.set_close_response("cancel");
                        // popup.set_modal(true);
                        popup.set_destroy_with_parent(true);

                        popup.add_response("cancel", "Cancel");
                        popup.add_response("confirm", "Confirm");
                        popup.set_response_appearance("confirm", adw::ResponseAppearance::Suggested);
                        popup.set_default_response(Some("confirm"));

                        let entry = gtk::Entry::new();
                        entry.set_placeholder_text(Some("0-999999"));
                        entry.set_input_purpose(gtk::InputPurpose::Digits);
                        entry.set_max_length(6);

                        popup.set_extra_child(Some(&entry));
                        popup.set_response_enabled("confirm", false);

                        entry.connect_changed(clone!(move |entry| {
                            let is_empty = entry.text().is_empty();

                            popup.set_response_enabled("confirm", !is_empty);

                            if is_empty {
                                entry.add_css_class("error");
                            }
                            else {
                                entry.remove_css_class("error");
                            }
                        }));
                        entry.add_css_class("error");

                        let pass_key = Rc::new(RefCell::new(String::new()));
                        popup2.clone().choose(gio::Cancellable::NONE, move |response| {
                            match response.to_string() {
                                s if s.contains("confirm") => {
                                    *pass_key.borrow_mut() = entry.text().to_string();
                                }
                                _ => {
                                    *pass_key.borrow_mut() = String::new();
                                }
                            }

                            OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = false;
                            OVERSKRIDE_PROPS.lock().unwrap().pass_key = pass_key.borrow().parse::<u32>().unwrap_or(0);
                        });
                    }
                    Message::DisplayPassKey(request) => {
                        let pin_code = &request.passkey;
                        let device = clone.device_display_name(request.device);
                        OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;

                        if clone.imp().display_pass_key_dialog.borrow().clone().is_some() {
                            let dialog = clone.imp().display_pass_key_dialog.borrow().clone().unwrap();
                            let label = dialog.extra_child().unwrap().downcast::<gtk::Label>().unwrap();

                            label.set_text(pin_code.to_string().as_str());
                        } else {
                            let body = "Please enter this pin code on ".to_string() + device.as_str();
                            let popup = adw::MessageDialog::new(Some(&clone), None, Some(body.as_str()));

                            let label = gtk::Label::new(Some(pin_code.to_string().as_str()));

                            popup.set_extra_child(Some(&label));
                            popup.add_response("okay", "Okay");
                            popup.set_close_response("okay");

                            popup.clone().choose(gio::Cancellable::NONE, move |_| {
                                OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = false;
                            });
                            *clone.imp().display_pass_key_dialog.borrow_mut() = Some(popup.clone());
                        }
                    }
                    Message::RequestConfirmation(request) => {
                        let device: String;
                        let adapter: String;
                        let passkey = &request.passkey.to_string();
                        unsafe {
                            OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;
                            device = clone.device_display_name(request.device);
                            let mut holder = String::new();
                            for key in ADAPTERS_LUT.clone().unwrap().keys() {
                                if let Some(pair) = ADAPTERS_LUT.clone().unwrap().get_key_value(key) {
                                    if pair.1 == &request.adapter {
                                        holder = pair.0.to_string();
                                    }
                                }
                            }
                            if holder.is_empty() {
                                adapter = "Unknown Adapter".to_string();
                            } else {
                                adapter = holder;
                            }
                        }

                        let body = "Is this the right code for ".to_string() + device.as_str() + " on " + adapter.as_str();
                        let popup = adw::MessageDialog::new(Some(&clone), Some("Pairing Request"), None);
                        popup.set_body_use_markup(true);
                        popup.set_body(body.as_str());

                        popup.set_close_response("cancel");
                        // popup.set_modal(true);
                        popup.set_destroy_with_parent(true);

                        popup.add_response("cancel", "Cancel");
                        popup.add_response("allow", "Allow");
                        popup.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
                        popup.set_default_response(Some("allow"));

                        let string = "<span font_weight='bold' font_size='32pt'>".to_string() + passkey + "</span>";
                        let label = gtk::Label::new(None);
                        label.set_use_markup(true);
                        label.set_label(string.as_str());

                        popup.set_extra_child(Some(&label));

                        let pass_key = Rc::new(RefCell::new(false));
                        popup.clone().choose(gio::Cancellable::NONE, move |response| {
                            match response.to_string() {
                                s if s.contains("allow") => {
                                    *pass_key.borrow_mut() = true;
                                }
                                _ => {
                                    *pass_key.borrow_mut() = false;
                                }
                            }

                            OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = false;
                            OVERSKRIDE_PROPS.lock().unwrap().confirm_authorization = *pass_key.borrow();
                        });
                    }
                    Message::RequestAuthorization(request) => {
                        let device: String;
                        let adapter: String;
                        unsafe {
                            device = clone.device_display_name(request.device);
                            adapter = ADAPTERS_LUT.clone().unwrap().get(&request.adapter).unwrap_or(&"Unknown Adapter".to_string()).to_string();
                            OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;
                        }

                        let body = "Is `".to_string() + device.as_str() + "` on `" + adapter.as_str() + "` allowed to pair?";
                        let popup = adw::MessageDialog::new(Some(&clone), Some("Pairing Request"), None);
                        popup.set_body_use_markup(true);
                        popup.set_body(body.as_str());

                        popup.set_close_response("cancel");
                        // popup.set_modal(true);
                        popup.set_destroy_with_parent(true);

                        popup.add_response("cancel", "Cancel");
                        popup.add_response("allow", "Allow");
                        popup.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
                        popup.set_default_response(Some("allow"));

                        let pass_key = Rc::new(RefCell::new(false));
                        popup.clone().choose(gio::Cancellable::NONE, move |response| {
                            match response.to_string() {
                                s if s.contains("allow") => {
                                    *pass_key.borrow_mut() = true;
                                }
                                _ => {
                                    *pass_key.borrow_mut() = false;
                                }
                            }

                            OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = false;
                            OVERSKRIDE_PROPS.lock().unwrap().confirm_authorization = *pass_key.borrow();
                        });
                    }
                    Message::AuthorizeService(request) => {
                        let device: String;
                        let adapter: String;
                        unsafe {
                            OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;
                            device = clone.device_display_name(request.device);
                            adapter = ADAPTERS_LUT.clone().unwrap().iter()
                                .find_map(|(key, val)| if val == &request.adapter { Some(key) } else { None })
                                .unwrap_or(&"Unknown Adapter".to_string()).to_string();
                        }

                        let service_id = match bluer::id::Service::try_from(request.service) {
                            Ok(name) => {
                                println!("service name is: {}", name.clone());
                                format!("{}", name)
                            }
                            Err(_) => {
                                if let Ok(name) = get_name_from_service(request.service) {
                                    name
                                } else {
                                    format!("Unknown Service of UUID: {:?}", request.service)
                                }
                            }
                        };

                                                println!("service authorization for {} on {}", device, adapter);
                        let requesting_address = request.device;
                        let requesting_adapter_name = request.adapter.clone();
                        *clone.imp().pending_service_authorization.borrow_mut() = Some((requesting_address, requesting_adapter_name.clone(), service_id.clone()));

                        let request_is_on_open_page = OVERSKRIDE_PROPS.lock().unwrap().address == requesting_address
                            && OVERSKRIDE_PROPS.lock().unwrap().current_adapter == requesting_adapter_name;
                        if request_is_on_open_page {
                            clone.show_pending_service_authorization_if_open(requesting_address);
                        } else {
                            let request_toast = adw::Toast::builder()
                                .title(glib::markup_escape_text(&format!("{} wants to connect ({})", device, service_id)).as_str())
                                .button_label("Review")
                                .timeout(25)
                                .priority(adw::ToastPriority::High)
                                .build();
                            let window_for_review = clone.downgrade();
                            request_toast.connect_button_clicked(move |_| {
                                if let Some(window) = window_for_review.upgrade() {
                                    window.open_device_page(requesting_address, &requesting_adapter_name);
                                }
                            });
                            clone.imp().toast_overlay.get().add_toast(request_toast);
                        }
                    }
                    Message::GoToBluetoothSettings(doso) => {
                        if doso {
                            let bluetooth_settings_row = clone.imp().bluetooth_settings_row.get();
                            bluetooth_settings_row.emit_activate();

                            let window_title = clone.imp().window_title.get();
                            window_title.set_title("Settings");
                        } else {
                            clone.device_list_models().selection.set_selected(0);

                            let window_title = clone.imp().window_title.get();
                            window_title.set_title("Settings");
                        }
                    }
                    Message::RequestYesNo(title, subtitle, confirm, response_type) => {
                        OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;

                        let popup = adw::MessageDialog::new(Some(&clone), Some(&title), None);

                        popup.set_close_response("cancel");
                        popup.set_modal(true);
                        popup.set_body_use_markup(true);
                        popup.set_body(&subtitle);
                        popup.set_destroy_with_parent(true);

                        popup.add_response("cancel", "Cancel");
                        popup.add_response(&confirm.to_lowercase(), &confirm);
                        popup.set_response_appearance(&confirm.to_lowercase(), response_type);
                        popup.set_default_response(Some(&confirm.to_lowercase()));

                        let pass_key = Rc::new(RefCell::new(false));
                        popup.clone().choose(gio::Cancellable::NONE, move |response| {
                            match response.to_string() {
                                s if s.contains(&confirm.to_lowercase()) => {
                                    *pass_key.borrow_mut() = true;
                                }
                                _ => {
                                    *pass_key.borrow_mut() = false;
                                }
                            }

                            OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = false;
                            OVERSKRIDE_PROPS.lock().unwrap().confirm_authorization = *pass_key.borrow();
                        });
                    }
                    Message::RefreshDevicesList() => {
                        WidgetExt::activate_action(&clone, "win.refresh-devices", None).expect("cannot refresh devices list");
                    }
                    Message::StartTransfer(transfer, filename, percent, current, filesize, outbound) => {
                        let receiving_popover = clone.imp().receiving_popover.get();

                        let row = ReceivingRow::new(transfer, filename.clone(), filesize, outbound);
                        println!("row is: {}, {:?}", row.get_row_transfer(), row.filename());

                        row.set_extra(percent, current, filesize, 0);
                        row.set_progress_bar_fraction(percent);
                        // println!("{} {} {}", row.percentage(), row.get_extra(), row.filesize());

                        receiving_popover.add_row(&row);
                    }
                    Message::UpdateTransfer(transfer, filename, current_mb, current_rate, status) => {
                        let receiving_popover = clone.imp().receiving_popover.get();

                        // loops over the transfers then selects the one that matches, updating it accordingly
                        if let Some(row) = receiving_popover.get_row_by_transfer(&transfer, &filename) {
                            let filesize = row.get_row_filesize();
                            let fraction = current_mb / filesize * 100.0;

                            // println!("status {}", &status);

                            row.set_progress_bar_fraction(fraction);
                            row.set_extra(fraction.round(), current_mb, filesize, current_rate);
                            let nuked = row.set_active_icon(status, filesize);

                            // if row is canceled or error, remove it in a minute
                            if nuked {
                                let cloned = sender_for_receiver_clone.clone();
                                runtime().spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                                    cloned.send(Message::RemoveTransfer(transfer, filename)).await.expect("cannot send message");
                                });
                            }
                        }
                    }
                    Message::RemoveTransfer(transfer, filename) => {
                        let receiving_popover = clone.imp().receiving_popover.get();

                        receiving_popover.remove_row(transfer, filename);
                    }
                    Message::GetFile(action) => {
                        // spawn a file chooser and get the chosen files
                        let dialog = gtk::FileChooserDialog::new(Some("Select File To Send"),
                            Some(&clone),
                            action,
                            &[("Cancel", gtk::ResponseType::Cancel),
                              ("Select", gtk::ResponseType::Accept)
                        ]);
                        dialog.set_destroy_with_parent(true);
                        dialog.set_select_multiple(true);
                        dialog.set_default_response(gtk::ResponseType::Accept);
                        dialog.set_modal(true);

                        OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = true;

                        // wait for exit then collect all files, if no files selected reset the file list
                        dialog.run_async(|file_chooser, response| {
	                        let mut all_files: Vec<String> = vec![];

                            if response != ResponseType::Cancel {
                                let files = file_chooser.files();

                                for file in files.into_iter() {
                                    if file.as_ref().unwrap().is::<gio::File>() {
                                        if let Some(path) = file.unwrap().dynamic_cast::<gio::File>().unwrap().path() {
                                            all_files.push(path.to_str().unwrap_or("").to_string());

                                            println!("file {:?}", all_files.last());
                                        };
                                    }
                                }
                            } else {
                                eprintln!("File selection failed: {:?}", response);
                            }

	                        OVERSKRIDE_PROPS.lock().unwrap().displaying_dialog = false;
	                        OVERSKRIDE_PROPS.lock().unwrap().send_files_path = all_files;

	                        file_chooser.destroy();
                        });
                    }
                    Message::SwitchSendFileActive(state) => {
                        let send_file_row = clone.imp().send_file_row.get();
                        send_file_row.set_sensitive(state);
                    }
                    Message::SetFileStorageLocation(holder_location) => {
                        // if the path is not a direct one, do not set anything and communicate to user
                        let file_save_location = clone.imp().file_save_location.get();
                        if !std::path::Path::new(&holder_location).is_dir() {
                            file_save_location.set_css_classes(&["error"]);
                            sender_for_receiver_clone.clone().send(Message::PopupError("file-storage-not-valid".to_string(), adw::ToastPriority::High)).await.expect("cannot send message");
                        } else {
                            file_save_location.set_css_classes(&[""]);

                            // for file names to remain *file names*
                            let mut location = holder_location.clone();
                            if !location.ends_with('/') {
                                location += "/";
                            }

                            OVERSKRIDE_PROPS.lock().unwrap().store_folder = location.clone();

                            if file_save_location.text() != location {
                                file_save_location.set_text(&location);
                            }

                            clone.imp().settings.get().expect("cannot get settings for file save location").set_string("store-folder", &location).expect("cannot set store folder");
                        }
                    }
                    Message::SetHideUnknownDevices(hidden) => {
                        let hide_unknowns_switch_row = clone.imp().hide_unknowns_switch_row.get();
                        hide_unknowns_switch_row.set_active(hidden);

                        println!("hidden devices set to {}", hidden);

                        OVERSKRIDE_PROPS.lock().unwrap().hide_unknown_devices = hidden;
                        clone.device_list_models().filter.changed(gtk::FilterChange::Different);

                        clone.imp().settings.get().expect("cannot get settings for file save location").set_boolean("hide-unknown-devices", hidden).expect("cannot set hide unknown devices");
                    }
                    Message::SwitchHasObexService(state) => {
                        let connected_switch_row = clone.imp().connected_switch_row.get();
                        connected_switch_row.set_row_has_obex(state);
                        clone.imp().send_file_row.get().set_visible(state);
                        clone.update_connection_properties_visibility();
                    }
                    Message::SetNameValid(state) => {
                        let device_name_entry = clone.imp().device_name_entry.get();

                        if state {
                            device_name_entry.remove_css_class("error");
                        } else {
                            device_name_entry.add_css_class("error");
                        }
                    }
                    Message::SwitchDeviceRenamePending(pending) => {
                        clone.set_device_rename_pending(pending);
                    }
                    Message::PopulateAudioProfilesList(hashmap) => {
                        let audio_profile_expander = clone.imp().audio_profile_expander.get();
                        let unknown = &"Unknown Profile".to_string();

                        // unexpand the expander then see loop to see which profile was last selected (could be better)
                        audio_profile_expander.set_expanded(false);
                        audio_profile_expander.connect_enable_expansion_notify(|expander| {
                            let address = OVERSKRIDE_PROPS.lock().unwrap().address;
                            let mut index = 0;
                            let mut last_profile = String::new();

                            let listbox = expander.last_child().unwrap().downcast::<gtk::Box>().unwrap()
                                .last_child().unwrap().downcast::<gtk::Revealer>().unwrap().last_child().unwrap().downcast::<gtk::ListBox>();

                            if let Ok(list) = listbox.clone() {
                                while let Some(row) = list.row_at_index(index) {
                                    // println!("{}", index);
                                    let selectable_row = row.downcast::<SelectableRow>().expect("cannot downcast to action row.");
                                    // println!("{:?}", action_row.clone());

                                    if selectable_row.selected() {
                                        last_profile = selectable_row.get_row_profile();
                                    }
                                    index += 1;
                                }
                            }

                            // if the expander can be expanded (ie it isn't off), then select the last profile, else turn off audio to that device
                            let target_profile = if expander.enables_expansion() {
                                last_profile
                            } else {
                                "off".to_string()
                            };

                            std::thread::spawn(move || {
                                audio_profiles::device_set_profile(address.to_string(), target_profile);
                            });
                        });

                        // trauma
                        let listbox = audio_profile_expander.last_child().unwrap().downcast::<gtk::Box>().unwrap()
                            .last_child().unwrap().downcast::<gtk::Revealer>().unwrap().last_child().unwrap().downcast::<gtk::ListBox>();


                        // remove all child rows and set sort func
                        if listbox.is_ok() {
                            while let Some(supposed_row) = listbox.clone().unwrap().last_child() {
                                listbox.clone().unwrap().remove(&supposed_row);
                            }
                        }

                        // add each profile to the expander, then select the active on
                        for profile in hashmap.keys() {
                            let holder = hashmap.get(profile).unwrap_or(unknown);
                            let description = &holder.replace('&', "&amp;");

                            let child = SelectableRow::new();
                            child.set_title(description);
                            child.set_use_markup(true);
                            child.set_row_profile(profile.clone());

                            let sender_clone = sender_for_receiver_clone.clone();

                            // on row click select this profile
                            child.set_activatable(true);
                            child.connect_activated(move |row| {
                                let profile = row.profile().clone();
                                let profile_clone = profile.clone();
                                let sender_clone_2 = sender_clone.clone();

                                glib::spawn_future_local(async move {
                                    let address = OVERSKRIDE_PROPS.lock().unwrap().address.to_string();

                                    std::thread::spawn(|| {
                                        audio_profiles::device_set_profile(address, profile);
                                    });
                                    sender_clone_2.send(Message::SetActiveAudioProfile(profile_clone)).await.expect("cannot send message");
                                    // println!("set active profile");
                                });
                            });

                            audio_profile_expander.add_row(&child);
                        }

                        listbox.clone().unwrap().set_sort_func(|row_one, row_two| {
                            let binding_one = row_one.clone().downcast::<adw::ActionRow>().unwrap().title();
                            let binding_two = row_two.clone().downcast::<adw::ActionRow>().unwrap().title();

                            let mut one = binding_one.as_str();
                            let mut two = binding_two.as_str();

                            let one_str = one.to_lowercase();
                            let two_str = two.to_lowercase();

                            one = one_str.as_str();
                            two = two_str.as_str();

                            let name_result = two.cmp(one);

                            name_result.into()
                        });
                        listbox.clone().unwrap().invalidate_sort();
                    }
                    Message::SwitchAudioProfilesList(state) => {
                        let audio_profile_expander = clone.imp().audio_profile_expander.get();
                        audio_profile_expander.set_sensitive(state);
                        audio_profile_expander.set_visible(state);
                        clone.update_connection_properties_visibility();
                    }
                    Message::SetActiveAudioProfile(profile) => {
                        let audio_profile_expander = clone.imp().audio_profile_expander.get();
                        let mut index = 0;

                        // absolutely traumatizing way of getting the listbox of an expander row
                        let listbox = audio_profile_expander.last_child().unwrap().downcast::<gtk::Box>().unwrap()
                            .last_child().unwrap().downcast::<gtk::Revealer>().unwrap().last_child().unwrap().downcast::<gtk::ListBox>();

                        // loop over all the devices and check which one matches out profile
                        if let Ok(list) = listbox.clone() {
                            while let Some(row) = list.row_at_index(index) {
                                // println!("{}", index);
                                let selectable_row = row.downcast::<SelectableRow>().expect("cannot downcast to action row.");
                                // println!("{:?}", action_row.clone());

                                if selectable_row.get_row_profile() == profile {
                                    selectable_row.set_selected(true);
                                } else {
                                    selectable_row.set_selected(false)
                                }
                                index += 1;
                            }
                        }
                    }
                    Message::SwitchAudioProfileExpanded(state) => {
                        let audio_profile_expander = clone.imp().audio_profile_expander.get();
                        audio_profile_expander.set_expanded(state);
                    }
                    Message::UpdateBatteryLevel(level) => {
                        clone.imp().pairing_timeline.get().set_battery_level(level);
                    }
                }
            }
        });

        self.setup_device_list(sender.clone());

        // refresh devices action, possibly most important action here
        // refreshes the main list, checks if we can send a "refreshed list" message to the user
        // so no weird "adapter off" then "refreshed list" messages happen
        let refresh_action = gio::SimpleAction::new("refresh-devices", None);
        let sender0 = sender.clone();
        refresh_action.connect_activate(move |_, _| {
            runtime().spawn(clone!(
                #[strong]
                sender0,
                async move {
                device::stop_searching().await;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                let sender = sender0.clone();
                let adapter_name = OVERSKRIDE_PROPS.lock().unwrap().current_adapter.clone();

                let mut can_send = true;
                if let Err(err) = device::get_devices_continuous(sender.clone(), adapter_name).await {
                    let string = err.message;

                    can_send = false;

                    sender
                        .send(Message::PopupError(string, adw::ToastPriority::High))
                        .await.expect("cannot send message");
                }
                println!("can send: {}", can_send);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if can_send {
                    sender
                        .send(Message::PopupError(
                            "br-adapter-refreshed".to_string(),
                            adw::ToastPriority::Normal,
                        ))
                        .await.expect("can't send message");
                }
            }));
        });
        self.add_action(&refresh_action);
        refresh_action.activate(None);

        // try to connect to a device, this will fail often because bluetooth
        // it also updates the "loading spinner" on the row itself
        let connected_switch_row = self.imp().connected_switch_row.get();
        let sender1 = sender.clone();
        connected_switch_row.set_activatable(true);
        connected_switch_row.connect_activated(move |row| {
            row.set_spinning(false);
            row.set_active(!row.active());
            spawn_device_activation(sender1.clone());
        });

        let radio_combo_row = self.imp().radio_combo_row.get();
        let window_for_radio_selector = self.downgrade();
        let radio_signal_id = radio_combo_row.connect_selected_notify(move |combo_row| {
            let Some(window) = window_for_radio_selector.upgrade() else {
                return;
            };
            let selected_adapter_name = window.imp().radio_adapter_names.borrow().get(combo_row.selected() as usize).cloned();
            if let Some(selected_adapter_name) = selected_adapter_name {
                if selected_adapter_name != OVERSKRIDE_PROPS.lock().unwrap().current_adapter {
                    window.switch_radio(&selected_adapter_name, None);
                }
            }
        });
        self.imp().radio_signal_id.set(radio_signal_id).expect("cannot set radio signal id");

        let scan_toggle_button = self.imp().scan_toggle_button.get();
        scan_toggle_button.connect_toggled(|toggle_button| {
            device::request_device_scan(toggle_button.is_active());
        });

        let pairing_timeline = self.imp().pairing_timeline.get();
        let sender_for_timeline = sender.clone();
        let window_for_timeline = self.downgrade();
        pairing_timeline.connect_step_action(move |step_action| {
            let sender_clone = sender_for_timeline.clone();
            let address = OVERSKRIDE_PROPS.lock().unwrap().address;
            let adapter_name = OVERSKRIDE_PROPS.lock().unwrap().current_adapter.clone();

            match step_action {
                PairingStepAction::ShowRadio(adapter_name) => {
                    if let Some(window) = window_for_timeline.upgrade() {
                        window.switch_radio(&adapter_name, Some(address));
                    }
                }
                PairingStepAction::PairViaRadio(adapter_name) => {
                    if let Some(window) = window_for_timeline.upgrade() {
                        window.switch_radio(&adapter_name, None);
                    }
                    let pair_via_radio_hint = format!(
                        "Switched to {}. Put the device in pairing mode; it appears in the list, then press Pair.",
                        crate::radios::describe_radio(&adapter_name)
                    );
                    runtime().spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        device::request_device_scan(true);
                        sender_clone
                            .send(Message::PopupError(pair_via_radio_hint, adw::ToastPriority::High))
                            .await.expect("cannot send message");
                    });
                }
                
                PairingStepAction::Pair | PairingStepAction::Connect | PairingStepAction::Disconnect => {
                    spawn_device_activation(sender_clone);
                }
                PairingStepAction::PairAgain => {
                    runtime().spawn(async move {
                        if let Err(err) = device::pair_device_again(address, sender_clone.clone(), adapter_name).await {
                            sender_clone
                                .send(Message::PopupError(err.message, adw::ToastPriority::High))
                                .await.expect("cannot send message");
                        }
                    });
                }
                PairingStepAction::ForgetDevice => {
                    runtime().spawn(async move {
                        if let Err(err) = device::remove_device(address, sender_clone.clone(), adapter_name).await {
                            sender_clone
                                .send(Message::PopupError(err.message, adw::ToastPriority::High))
                                .await.expect("cannot send message");
                        }
                    });
                }
                PairingStepAction::AnswerServiceAuthorization(authorization_answer) => {
                    if let Some(window) = window_for_timeline.upgrade() {
                        window.imp().pending_service_authorization.replace(None);
                    }
                    let mut overskride_properties = OVERSKRIDE_PROPS.lock().unwrap();
                    overskride_properties.confirm_authorization = authorization_answer != ServiceAuthorizationAnswer::Deny;
                    overskride_properties.trust_after_authorization = authorization_answer == ServiceAuthorizationAnswer::AlwaysAllow;
                    overskride_properties.displaying_dialog = false;
                }
                PairingStepAction::SetConnectionPolicy(connection_policy) => {
                    runtime().spawn(async move {
                        if let Err(err) = device::set_connection_policy(address, connection_policy, sender_clone.clone(), adapter_name).await {
                            sender_clone
                                .send(Message::PopupError(err.message, adw::ToastPriority::High))
                                .await.expect("cannot send message");
                        }
                    });
                }
            }
        });

        

        

        let device_name_entry = self.imp().device_name_entry.get();
        let window_for_apply = self.downgrade();
        let sender_for_apply = sender.clone();
        device_name_entry.connect_apply(move |_| {
            if let Some(window) = window_for_apply.upgrade() {
                window.request_device_rename(sender_for_apply.clone());
            }
        });
        let window_for_enter = self.downgrade();
        let sender_for_enter = sender.clone();
        device_name_entry.connect_entry_activated(move |_| {
            if let Some(window) = window_for_enter.upgrade() {
                window.request_device_rename(sender_for_enter.clone());
            }
        });

        // turn adapter on or off
        let powered_switch_row = self.imp().powered_switch_row.get();
        let sender5 = sender.clone();
        let powered_signal_id = powered_switch_row.connect_active_notify(move |row| {
            let sender_clone = sender5.clone();
            let adapter_name = OVERSKRIDE_PROPS.lock().unwrap().current_adapter.clone();
            let powered = row.is_active();

            runtime().spawn(clone!(
                #[strong]
                sender_clone,
                async move {
                    if let Err(err) =
                        bluetooth_settings::set_adapter_powered(adapter_name, powered, sender_clone.clone()).await
                    {
                        let string = err.message;
                        sender_clone
                            .send(Message::PopupError(string, adw::ToastPriority::High))
                            .await.expect("cannot send message");
                        sender_clone
                            .send(Message::SwitchAdapterPowered(!powered))
                            .await.expect("cannot send message");
                    }
                }
            ));
        });
        self.imp()
            .powered_signal_id
            .set(powered_signal_id)
            .expect("cannot set powered signal id");

        // switches the current adapters discoverable state, making it visible to nearby devices
        let discoverable_switch_row = self.imp().discoverable_switch_row.get();
        let sender6 = sender.clone();
        let discoverable_signal_id = discoverable_switch_row.connect_active_notify(move |row| {
            let sender_clone = sender6.clone();
            let adapter_name = OVERSKRIDE_PROPS.lock().unwrap().current_adapter.clone();
            let discoverable = row.is_active();

            runtime().spawn(async move {
                if let Err(err) =
                    bluetooth_settings::set_adapter_discoverable(adapter_name, discoverable, sender_clone.clone()).await
                {
                    let string = "Adapter ".to_string() + &err.message;
                    sender_clone
                        .send(Message::PopupError(string, adw::ToastPriority::High))
                        .await.expect("cannot send message");
                    sender_clone
                        .send(Message::SwitchAdapterDiscoverable(!discoverable))
                        .await.expect("cannot send message");
                }
            });
        });
        self.imp()
            .discoverable_signal_id
            .set(discoverable_signal_id)
            .expect("cannot set discoverable signal id");

        // change the adapter name, should always work (if not get professional help)
        let adapter_name_entry = self.imp().adapter_name_entry.get();
        let sender7 = sender.clone();
        adapter_name_entry.connect_apply(move |entry| {
            let new_name = entry.text().to_string();
            let sender_clone = sender7.clone();
            let adapter_name = OVERSKRIDE_PROPS.lock().unwrap().current_adapter.clone();

            runtime().spawn(async move {
                if let Err(err) = bluetooth_settings::set_adapter_name(
                    new_name,
                    adapter_name,
                    sender_clone.clone(),
                ).await {
                    let string = "Adapter ".to_string() + &err.message;
                    sender_clone
                        .send(Message::PopupError(string, adw::ToastPriority::High))
                        .await.expect("cannot send message");
                }
            });
        });

        // sets the discoverable timeout of the adapter
        // signal is for not going into infinite loop when set from code
        let timeout_adjustment = self.imp().timeout_time_adjustment.get();
        let sender8 = sender.clone();
        let id = timeout_adjustment.connect_value_changed(move |adjustment| {
            let value = adjustment.value();
            let sender_clone = sender8.clone();
            let adapter_name = OVERSKRIDE_PROPS.lock().unwrap().current_adapter.clone();

            runtime().spawn(async move {
                if let Err(err) = bluetooth_settings::set_timeout_duration(
                    value as u32,
                    adapter_name,
                    sender_clone.clone(),
                ).await {
                    let string = err.message;
                    sender_clone
                        .send(Message::PopupError(string, adw::ToastPriority::High))
                        .await.expect("cannot send message");
                    sender_clone
                        .send(Message::SwitchAdapterTimeout(0))
                        .await.expect("cannot send message");
                }
            });
        });
        self.imp()
            .timeout_signal_id
            .set(id)
            .expect("cannot set timeout signal id");

        // switch to settings page deselecting any devices
        let bluetooth_settings_row = self.imp().bluetooth_settings_row.get();
        let sender9 = sender.clone();
        let self_clone3 = self.clone();
        bluetooth_settings_row.connect_activated(move |_| {
            let sender_clone = sender9.clone();
            runtime().spawn(async move {
                let adapter_names = bluetooth_settings::populate_adapter_expander().await;
                let sender = OVERSKRIDE_PROPS.lock().unwrap().sender.clone().unwrap();
                let adapter_name = OVERSKRIDE_PROPS.lock().unwrap().current_adapter.clone();

                if let Ok(names) = adapter_names {
                    if let Err(err) =
                        bluetooth_settings::get_adapter_properties(names, sender, adapter_name).await
                    {
                        let string = "Adapter ".to_string() + &err.message;
                        sender_clone
                            .send(Message::PopupError(string, adw::ToastPriority::Normal))
                            .await.expect("cannot send message");
                    }
                }
            });

            self_clone3.device_list_models().selection.unselect_all();

            // select the bluetooth settings page on startup
            let main_stack = self_clone3.imp().main_stack.get();
            let pages = main_stack.pages();
            pages.select_item(1, true);

            let split_view = self_clone3.imp().split_view.get();
            if split_view.is_collapsed() {
                split_view.set_show_sidebar(false);
            }

            let window_title = self_clone3.imp().window_title.get();
            window_title.set_title("Settings");
        });
        bluetooth_settings_row.emit_activate();

        // show or hide the sidebar
        let split_view = self.imp().split_view.get();
        let self_clone4 = self.clone();
        split_view.connect_show_sidebar_notify(move |view| {
            let show_sidebar_button = self_clone4.imp().show_sidebar_button.get();
            let active = view.shows_sidebar();

            let text = match active {
                true => "Hide Sidebar",
                false => "Show Sidebar",
            };
            show_sidebar_button.set_tooltip_text(Some(text));
            show_sidebar_button.set_active(active);
        });

        // choose the file to be sent to the selected device
        let choose_file_button = self.imp().choose_file_button.get();
        let sender10 = sender.clone();
        let self_clone5 = self.clone();
        choose_file_button.connect_clicked(move |_| {
            let selected_device = self_clone5.selected_device();
            let connected = self_clone5.imp().connected_switch_row.get().active();
            let another_clone = sender10.clone();

            glib::spawn_future_local(async move {
                if !connected {
                    another_clone
                        .send(Message::PopupError(
                            "obex-transfer-not-connected".to_string(),
                            adw::ToastPriority::Normal,
                        ))
                        .await.expect("cannot send message");
                    return;
                }

                // get the currently selected device from the main listbox, and its adapter, then send the file from the picked out list of files
                if let Some(bluetooth_device) = selected_device {
                    let source = bluetooth_device.bluer_adapter_address();
                    let destination = bluetooth_device.bluer_address();

                    let sender_clone = another_clone.clone();
                    std::thread::spawn(move || {
                        obex::start_send_file(destination, source, sender_clone);
                    });
                } else {
                    println!("error while sending file, destination doesn't exist???");
                    another_clone
                        .send(Message::PopupError(
                            "obex-transfer-not-connected".to_string(),
                            adw::ToastPriority::Normal,
                        ))
                        .await.expect("cannot send message");
                }
            });
        });

        // set the file save location from text input
        let file_save_location = self.imp().file_save_location.get();
        let sender11 = sender.clone();
        file_save_location.connect_apply(move |entry| {
            let location = entry.text().to_string();
            let another_clone = sender11.clone();

            glib::spawn_future_local(async move {
                another_clone
                    .send(Message::SetFileStorageLocation(location))
                    .await.expect("cannot send message");
            });
        });

        // set the file save location from a file picker
        let choose_location_button = self.imp().choose_location_button.get();
        let sender12 = sender.clone();
        choose_location_button.connect_clicked(move |_| {
            let sender_clone = sender12.clone();
            runtime().spawn(async {
                get_store_location_from_dialog(sender_clone).await;
            });
        });

        let auto_accept_trusted_row = self.imp().auto_accept_trusted_row.get();
        let sender13 = sender.clone();
        auto_accept_trusted_row.connect_activated(move |row| {
            let another_clone = sender13.clone();
            let active = row.is_active().clone();

            glib::spawn_future_local(async move {
                // if it's the first auto accept the user has done, warn about how dangerous it is

                if OVERSKRIDE_PROPS.lock().unwrap().auto_accept_first {
                    let title = "Warning!".to_string();
                    let subtitle = "Enabling auto accept from trusted devices <span font_weight='bold'>may put your device at risk</span>, as anyone with a device you labeled as \"trusted\" will be able to freely send you files".to_string();
                    let confirm = "I Understand".to_string();
                    let response_type = adw::ResponseAppearance::Destructive;
                    another_clone.send(Message::RequestYesNo(title, subtitle, confirm, response_type)).await.expect("cannot send message");
                    OVERSKRIDE_PROPS.lock().unwrap().auto_accept_first = false;
                    OVERSKRIDE_PROPS.lock().unwrap().confirm_authorization = false;
                }

                OVERSKRIDE_PROPS.lock().unwrap().auto_accept_from_trusted = !active;
                println!("auto accept is {}", OVERSKRIDE_PROPS.lock().unwrap().auto_accept_from_trusted);
            });
        });
        auto_accept_trusted_row.set_active(OVERSKRIDE_PROPS.lock().unwrap().auto_accept_from_trusted);

        // set the hide unknown devices boolean
        let hide_unknowns_switch_row = self.imp().hide_unknowns_switch_row.get();
        let sender14 = sender.clone();
        hide_unknowns_switch_row.connect_activated(move |row| {
            let active = row.is_active();
            let another_clone = sender14.clone();

            glib::spawn_future_local(async move {
                another_clone
                    .send(Message::SetHideUnknownDevices(!active))
                    .await.expect("cannot send message");
                OVERSKRIDE_PROPS.lock().unwrap().hide_unknown_devices = !active;
            });
        });
        hide_unknowns_switch_row.set_active(OVERSKRIDE_PROPS.lock().unwrap().hide_unknown_devices);
    }

    fn setup_device_list(&self, sender: Sender<Message>) {
        let device_store = gio::ListStore::new::<BluetoothDevice>();
        let device_filter = gtk::CustomFilter::new(|object| {
            let hide_unknown_devices = OVERSKRIDE_PROPS.lock().unwrap().hide_unknown_devices;
            !hide_unknown_devices || object.downcast_ref::<BluetoothDevice>().is_some_and(|bluetooth_device| !bluetooth_device.is_unknown())
        });
        let device_sorter = gtk::CustomSorter::new(compare_bluetooth_devices);
        let filtered_devices = gtk::FilterListModel::new(Some(device_store.clone()), Some(device_filter.clone()));
        let sorted_devices = gtk::SortListModel::new(Some(filtered_devices), Some(device_sorter.clone()));
        let device_selection = gtk::SingleSelection::builder()
            .model(&sorted_devices)
            .autoselect(false)
            .can_unselect(true)
            .build();

        let window_for_selection = self.downgrade();
        device_selection.connect_selected_item_notify(move |selection| {
            let Some(window) = window_for_selection.upgrade() else {
                return;
            };
            let newly_selected_device = selection.selected_item().and_downcast::<BluetoothDevice>();
            if window.selected_device() == newly_selected_device {
                return;
            }
            window.set_selected_device(newly_selected_device.as_ref());
            if let Some(bluetooth_device) = newly_selected_device {
                window.open_selected_device(&bluetooth_device, sender.clone());
            }
        });

        let window_for_placeholder = self.downgrade();
        device_selection.connect_items_changed(move |selection, _, _, _| {
            if let Some(window) = window_for_placeholder.upgrade() {
                window.update_device_list_placeholder(selection.n_items());
            }
        });

        self.imp().device_list_view.get().set_model(Some(&device_selection));
        self.update_device_list_placeholder(device_selection.n_items());
        self.imp()
            .device_list_models
            .set(imp::DeviceListModels {
                store: device_store,
                filter: device_filter,
                sorter: device_sorter,
                selection: device_selection,
            })
            .expect("device list is set up once");
    }

    fn device_list_models(&self) -> &imp::DeviceListModels {
        self.imp().device_list_models.get().expect("device list is set up during window setup")
    }

    fn update_device_list_placeholder(&self, visible_device_count: u32) {
        let has_visible_devices = visible_device_count > 0;
        self.imp().listbox_image_box.get().set_visible(!has_visible_devices);
        self.imp().device_list_view.get().set_visible(has_visible_devices);
    }

    fn find_device_by_address(&self, address: bluer::Address) -> Option<BluetoothDevice> {
        self.device_list_models()
            .store
            .iter::<BluetoothDevice>()
            .filter_map(Result::ok)
            .find(|bluetooth_device| bluetooth_device.bluer_address() == address)
    }

    fn device_display_name(&self, address: bluer::Address) -> String {
        self.find_device_by_address(address)
            .map(|bluetooth_device| bluetooth_device.display_name())
            .unwrap_or_else(|| UNKNOWN_DEVICE_NAME.to_string())
    }

    fn insert_or_update_device(&self, device_snapshot: &DeviceSnapshot) {
        match self.find_device_by_address(device_snapshot.address) {
            Some(existing_device) => existing_device.apply_snapshot(device_snapshot),
            None => {
                let device_list_models = self.device_list_models();
                let bluetooth_device = BluetoothDevice::from_snapshot(device_snapshot);
                let device_filter = device_list_models.filter.clone();
                let device_sorter = device_list_models.sorter.clone();
                bluetooth_device.connect_notify_local(None, move |_, changed_property| match changed_property.name() {
                    "alias" => {
                        device_filter.changed(gtk::FilterChange::Different);
                        device_sorter.changed(gtk::SorterChange::Different);
                    }
                    "rssi" | "connected" | "trusted" => device_sorter.changed(gtk::SorterChange::Different),
                    _ => {}
                });
                device_list_models.store.append(&bluetooth_device);
            }
        }

        let reselect_this_device = *self.imp().pending_device_selection.borrow() == Some(device_snapshot.address);
        if reselect_this_device {
            self.imp().pending_device_selection.replace(None);
            self.select_device_by_address(device_snapshot.address);
        }
    }

    fn select_device_by_address(&self, address: bluer::Address) {
        let device_selection = &self.device_list_models().selection;
        let device_position = device_selection
            .iter::<BluetoothDevice>()
            .filter_map(Result::ok)
            .position(|bluetooth_device| bluetooth_device.bluer_address() == address);
        if let Some(device_position) = device_position {
            device_selection.set_selected(device_position as u32);
        }
    }

    fn open_selected_device(&self, bluetooth_device: &BluetoothDevice, sender: Sender<Message>) {
        let address = bluetooth_device.bluer_address();
        let adapter_name = bluetooth_device.adapter_name();
        OVERSKRIDE_PROPS.lock().unwrap().address = address;

        runtime().spawn(async move {
            if let Err(err) = device::get_device_properties(address, sender.clone(), adapter_name).await {
                sender
                    .send(Message::GoToBluetoothSettings(true))
                    .await.expect("cannot send message");
                sender
                    .send(Message::PopupError(err.message, adw::ToastPriority::High))
                    .await.expect("cannot send message");
            }
        });
    }

    fn show_device_icon(&self, icon_name: &str) {
        let symbolic_icon_name = format!("{}-symbolic", icon_name);
        self.imp().device_icon.get().set_icon_name(Some(symbolic_icon_name.as_str()));
    }

    fn request_device_rename(&self, sender: Sender<Message>) {
        let Some(bluetooth_device) = self.selected_device() else {
            return;
        };
        if self.device_rename_pending() {
            return;
        }

        let device_name_entry = self.imp().device_name_entry.get();
        let requested_name = device_name_entry.text().trim().to_string();
        if requested_name == bluetooth_device.alias() {
            device_name_entry.remove_css_class("error");
            return;
        }

        let name_taken_by_other_device = self
            .device_list_models()
            .store
            .iter::<BluetoothDevice>()
            .filter_map(Result::ok)
            .any(|other_device| other_device.bluer_address() != bluetooth_device.bluer_address() && other_device.alias().trim() == requested_name);
        if name_taken_by_other_device {
            device_name_entry.add_css_class("error");
            sender
                .try_send(Message::PopupError("device-name-exists".to_string(), adw::ToastPriority::High))
                .expect("cannot send message");
            return;
        }

        self.set_device_rename_pending(true);
        let address = bluetooth_device.bluer_address();
        let adapter_name = bluetooth_device.adapter_name();
        runtime().spawn(async move {
            if let Err(rename_error) = device::set_device_name(address, requested_name, sender.clone(), adapter_name).await {
                sender
                    .send(Message::SetNameValid(false))
                    .await.expect("cannot send message");
                sender
                    .send(Message::PopupError(rename_error.message, adw::ToastPriority::High))
                    .await.expect("cannot send message");
            }
            sender
                .send(Message::SwitchDeviceRenamePending(false))
                .await.expect("cannot send message");
        });
    }

    /// on app exit, save the current settings
        fn show_pending_service_authorization_if_open(&self, address: bluer::Address) {
        let pending_request = self.imp().pending_service_authorization.borrow().clone();
        let Some((pending_address, pending_adapter_name, service_name)) = pending_request else {
            return;
        };
        let page_shows_requesting_device = pending_address == address
            && OVERSKRIDE_PROPS.lock().unwrap().address == address
            && OVERSKRIDE_PROPS.lock().unwrap().current_adapter == pending_adapter_name;
        if page_shows_requesting_device {
            self.imp().pairing_timeline.get().show_service_authorization_request(&service_name, &crate::radios::describe_radio(&pending_adapter_name));
        }
    }

    fn open_device_page(&self, address: bluer::Address, adapter_name: &str) {
        if OVERSKRIDE_PROPS.lock().unwrap().current_adapter != adapter_name {
            self.switch_radio(adapter_name, Some(address));
            return;
        }

        self.select_device_by_address(address);
    }

    fn update_connection_properties_visibility(&self) {
        let has_visible_property_row = self.imp().audio_profile_expander.get().is_visible() || self.imp().send_file_row.get().is_visible();
        self.imp().connection_properties_group.get().set_visible(has_visible_property_row);
    }

    fn switch_radio(&self, adapter_name: &str, device_to_reselect: Option<bluer::Address>) {
        OVERSKRIDE_PROPS.lock().unwrap().current_adapter = adapter_name.to_string();
        if let Some(settings) = self.imp().settings.get() {
            if let Err(settings_error) = settings.set_string("current-adapter-name", adapter_name) {
                println!("cannot save current radio: {:?}", settings_error);
            }
        }

        self.device_list_models().store.remove_all();

        if device_to_reselect.is_none() {
            self.imp().bluetooth_settings_row.get().emit_activate();
        }
        self.imp().pending_device_selection.replace(device_to_reselect);

        if let Err(refresh_error) = WidgetExt::activate_action(self, "win.refresh-devices", None) {
            println!("cannot refresh devices after switching radio: {:?}", refresh_error);
        }
    }

    fn save_settings(&self) -> Result<(), glib::BoolError> {
        let size = (
            self.size(gtk::Orientation::Horizontal),
            self.size(gtk::Orientation::Vertical),
        );
        // let size = self.SIZE
        let settings = self
            .imp()
            .settings
            .get()
            .expect("cannot get settings, setup improperly?");

        println!("size is {:?}", size);

        settings.set_int("window-width", size.0)?;
        settings.set_int("window-height", size.1)?;
        settings.set_boolean("window-maximized", self.is_maximized())?;
        settings.set_boolean("first-auto-accept", OVERSKRIDE_PROPS.lock().unwrap().auto_accept_first)?;

        Ok(())
    }

    /// loads settings from save in gsettings
    fn preload_settings(&self) {
        let settings = self
            .imp()
            .settings
            .get()
            .expect("cannot get settings, setup improperly?");

        let width = settings.int("window-width");
        let height = settings.int("window-height");
        let maximized = settings.boolean("window-maximized");
        let first_auto_accept = settings.boolean("first-auto-accept");

        // println!("new size is {:?}", (width, height));

        self.set_default_size(width, height);
        self.set_maximized(maximized);

        let file_save_location = self.imp().file_save_location.get();
        let mut store_folder = settings.string("store-folder").to_string();

        // if the store folder doesn't exist, try to get the download dir, if not then set to the (definitely findable) home dir
        if store_folder.is_empty() {
            if let Some(download_dir) = glib::user_special_dir(glib::UserDirectory::Downloads) {
                let holder = download_dir
                    .to_str()
                    .unwrap_or("Unknown Directory")
                    .to_string();
                store_folder = holder;
                settings
                    .set_string("store-folder", &store_folder)
                    .expect("cannot set store folder");
            } else {
                let holder = glib::home_dir()
                    .to_str()
                    .unwrap_or("Unknown Directory")
                    .to_string();
                store_folder = holder;
                settings
                    .set_string("store-folder", &store_folder)
                    .expect("cannot set store folder");
            }
        }

        // so the filename doesn't get fucked
        if !store_folder.ends_with('/') {
            store_folder += "/";
        }

        println!("store folder is: {}", &store_folder);
        file_save_location.set_text(&store_folder);

        let hide_unknown_devices = settings.boolean("hide-unknown-devices");

        OVERSKRIDE_PROPS.lock().unwrap().store_folder = store_folder;
        OVERSKRIDE_PROPS.lock().unwrap().auto_accept_first = first_auto_accept;
        OVERSKRIDE_PROPS.lock().unwrap().hide_unknown_devices = hide_unknown_devices
    }

    // first thing called when app launches, sets it up so it can be used basically
    #[tokio::main]
    async fn pre_setup(&self, sender: Sender<Message>) -> bluer::Result<()> {
        let settings = self.imp().settings.get().unwrap();

        unsafe {
            // makes a new sender, devices lut, rssi lut, and updates the current adapter name in gsettings
            OVERSKRIDE_PROPS.lock().unwrap().sender = Some(sender.clone());
            RSSI_LUT = Some(HashMap::new());
            let name = settings.string("current-adapter-name").to_string();
            let session = bluer::Session::new().await?;

            // if current adapter doesn't exist, get the default adapter instead (first run/error stuff)
            if name.is_empty() {
                let adapter = session.default_adapter().await?;
                OVERSKRIDE_PROPS.lock().unwrap().current_adapter = adapter.name().to_string();
                OVERSKRIDE_PROPS.lock().unwrap().name = adapter.name().to_string();

                let current_adapter = adapter.name().to_string();
                settings
                    .set_string("current-adapter-name", current_adapter.as_str())
                    .expect("cannot set default adapter at start");
                settings
                    .set_string("original-adapter-name", current_adapter.as_str())
                    .expect("cannot set original adapter at start");
            } else {
                OVERSKRIDE_PROPS.lock().unwrap().current_adapter = name.clone();
            }

            let mut lut = HashMap::new();

            let adapter = session.adapter(OVERSKRIDE_PROPS.lock().unwrap().current_adapter.clone().as_str())?;
            let alias = adapter.alias().await?;
            println!("startup alias is: {}\n", alias);
            self.imp()
                .timeout_time_adjustment
                .get()
                .set_value(adapter.discoverable_timeout().await?.into());

            lut.insert(alias.to_string(), OVERSKRIDE_PROPS.lock().unwrap().current_adapter.to_string());
            // update available adapters lut
            ADAPTERS_LUT = Some(lut);

            // start the bluetooth and obex agents in separate threads, so they're always available to receive input
            let clone = sender.clone();
            std::thread::spawn(move || {
                register_obex_agent(clone.clone()).expect("cannot register obex agent");
            });
            std::thread::spawn(move || {
                register_bluetooth_agent(sender.clone()).expect("cannot register bluetooth agent");
            });
        }

        Ok(())
    }
}

fn spawn_device_activation(sender: Sender<Message>) {
    let address = OVERSKRIDE_PROPS.lock().unwrap().address;
    let adapter_name = OVERSKRIDE_PROPS.lock().unwrap().current_adapter.clone();

    runtime().spawn(async move {
        if let Err(err) = device::set_device_active(address, sender.clone(), adapter_name).await {
            let string = err.clone().message;
            println!("error while connecting {:?}\n", err);

            sender
                .send(Message::PopupError(string, adw::ToastPriority::High))
                .await.expect("cannot send message");
            sender
                .send(Message::SwitchActive(false, address, true))
                .await.expect("cannot send message");
            sender
                .send(Message::SwitchActiveSpinner(false))
                .await.expect("cannot send message");
            sender
                .send(Message::SwitchSendFileActive(false))
                .await.expect("cannot send message");
        }
    });
}

fn compare_bluetooth_devices(first_object: &glib::Object, second_object: &glib::Object) -> gtk::Ordering {
    let first_device = first_object.downcast_ref::<BluetoothDevice>().expect("device list holds bluetooth devices");
    let second_device = second_object.downcast_ref::<BluetoothDevice>().expect("device list holds bluetooth devices");

    first_device
        .is_unknown()
        .cmp(&second_device.is_unknown())
        .then_with(|| second_device.connected().cmp(&first_device.connected()))
        .then_with(|| second_device.trusted().cmp(&first_device.trusted()))
        .then_with(|| first_device.rssi().cmp(&second_device.rssi()))
        .then_with(|| first_device.display_name().to_lowercase().cmp(&second_device.display_name().to_lowercase()))
        .into()
}

// TODO
// - use fxhashmap for even faster lookups
// - add option to auto trust device on pair (include warning about how dangerous it is)
// - background running, with a status taskbar thingy wtv its name is
// - add a loop for if obex and bluetooth agents fail
// - make new battery implementation
// - add a battery enable experimental thingy
// - add a auto accept service if previous
// - add a disable current connected icon
// - add a device distance using rssi and tx power
// - add a possible refresh interval so device that disconnected get picked up immediately (customizable in settings)
// - add a auto accept after first file
// - find out what is causing hang on start
