use bindrune_bluez::{
    bluez_dbus_session::BluetoothSession,
    bluez_operation_error::BluetoothError,
    remote_device::{
        remote_device_commands::rename_device, remote_device_snapshot::DeviceSnapshot,
    },
};
use relm4::{adw, adw::prelude::*, gtk, prelude::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperationState {
    Idle,
    Busy,
}

pub struct DeviceNameGroupInit {
    pub session: BluetoothSession,
    pub snapshot: DeviceSnapshot,
}

pub struct DeviceNameGroup {
    session: BluetoothSession,
    snapshot: DeviceSnapshot,
    rename_operation: OperationState,
    pending_alias: Option<String>,
    alias_changed: bool,
}

#[derive(Debug)]
pub enum DeviceNameGroupInput {
    DeviceChanged(DeviceSnapshot),
    RenameRequested(String),
}

#[derive(Debug)]
pub enum DeviceNameGroupCommand {
    Renamed(Result<(), BluetoothError>),
}

#[relm4::component(pub)]
impl Component for DeviceNameGroup {
    type Init = DeviceNameGroupInit;
    type Input = DeviceNameGroupInput;
    type Output = BluetoothError;
    type CommandOutput = DeviceNameGroupCommand;

    view! {
        adw::PreferencesGroup {
            set_description: Some("Stored on this computer for each radio, together with the bond."),

            adw::EntryRow {
                set_title: "Device Name",
                set_show_apply_button: true,
                #[track(model.alias_changed)]
                set_show_apply_button: false,
                #[track(model.alias_changed)]
                set_text: &model.snapshot.alias,
                #[track(model.alias_changed)]
                set_show_apply_button: true,
                #[watch]
                set_editable: model.rename_operation == OperationState::Idle,
                connect_apply[sender] => move |entry_row| {
                    sender.input(DeviceNameGroupInput::RenameRequested(entry_row.text().to_string()));
                },

                add_suffix = &gtk::Spinner {
                    set_tooltip_text: Some("Renaming…"),
                    #[watch]
                    set_spinning: model.rename_operation == OperationState::Busy,
                    #[watch]
                    set_visible: model.rename_operation == OperationState::Busy,
                },
            },
        }
    }

    fn init(
        group_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {
            session: group_init.session,
            snapshot: group_init.snapshot,
            rename_operation: OperationState::Idle,
            pending_alias: None,
            alias_changed: true,
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        self.alias_changed = false;
        match input {
            DeviceNameGroupInput::DeviceChanged(snapshot) => {
                if self.pending_alias.as_ref() == Some(&snapshot.alias) {
                    self.pending_alias = None;
                }
                self.alias_changed =
                    self.pending_alias.is_none() && snapshot.alias != self.snapshot.alias;
                self.snapshot = snapshot;
            }
            DeviceNameGroupInput::RenameRequested(entered_alias) => {
                let requested_alias = entered_alias.trim().to_owned();
                if requested_alias.is_empty() || requested_alias == self.snapshot.alias {
                    self.alias_changed = true;
                    return;
                }
                self.rename_operation = OperationState::Busy;
                self.pending_alias = Some(requested_alias.clone());
                let session = self.session.clone();
                let location = self.snapshot.location.clone();
                sender.oneshot_command(async move {
                    DeviceNameGroupCommand::Renamed(
                        rename_device(&session, &location, &requested_alias).await,
                    )
                });
            }
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        self.alias_changed = false;
        match command_output {
            DeviceNameGroupCommand::Renamed(outcome) => {
                self.rename_operation = OperationState::Idle;
                if let Err(rename_error) = outcome {
                    self.pending_alias = None;
                    self.alias_changed = true;
                    let _device_page_gone = sender.output(rename_error);
                }
            }
        }
    }
}
