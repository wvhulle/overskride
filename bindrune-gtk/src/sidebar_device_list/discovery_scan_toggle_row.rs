use std::time::Duration;

use bindrune_bluez::{
    bluetooth_adapter::adapter_snapshot_and_discovery::{DiscoverySession, start_discovery},
    bluez_dbus_session::BluetoothSession,
    bluez_operation_error::BluetoothError,
};
use futures::StreamExt;
use relm4::{adw, adw::prelude::*, gtk, prelude::*};

const SCAN_DURATION_SECONDS: u32 = 60;
const SCAN_TICK: Duration = Duration::from_secs(1);

enum ScanState {
    Idle,
    Scanning {
        generation: u64,
        remaining_seconds: u32,
        _discovery: DiscoverySession,
    },
}

pub struct DiscoveryScanToggleRowInit {
    pub session: BluetoothSession,
    pub adapter_name: String,
}

pub struct DiscoveryScanToggleRow {
    session: BluetoothSession,
    adapter_name: String,
    scan_state: ScanState,
    scan_generation: u64,
}

#[derive(Debug)]
pub enum DiscoveryScanToggleRowInput {
    ScanToggled(bool),
    StartScan,
    StopScan,
    AdapterSwitched(String),
}

#[derive(Debug)]
pub enum DiscoveryScanToggleRowCommand {
    ScanStarted {
        generation: u64,
        outcome: Result<DiscoverySession, BluetoothError>,
    },
    ScanTick(u64),
}

impl DiscoveryScanToggleRow {
    fn is_scanning(&self) -> bool {
        matches!(self.scan_state, ScanState::Scanning { .. })
    }

    fn scan_row_title(&self) -> &'static str {
        if self.is_scanning() {
            "Looking for Nearby Devices"
        } else {
            "Scan for Nearby Devices"
        }
    }

    fn scan_row_subtitle(&self) -> String {
        match self.scan_state {
            ScanState::Scanning {
                remaining_seconds, ..
            } => format!("{remaining_seconds} s left"),
            ScanState::Idle => "Finds devices that are in pairing mode".to_string(),
        }
    }

    fn scan_button_label(&self) -> &'static str {
        if self.is_scanning() { "Stop" } else { "Scan" }
    }

    fn scan_button_tooltip(&self) -> &'static str {
        if self.is_scanning() {
            "Stop looking for nearby devices"
        } else {
            "Look for nearby devices for one minute"
        }
    }

    fn start_scan(&mut self, sender: &ComponentSender<Self>) {
        if self.is_scanning() {
            return;
        }
        self.scan_generation += 1;
        let generation = self.scan_generation;
        let session = self.session.clone();
        let adapter_name = self.adapter_name.clone();
        sender.oneshot_command(async move {
            DiscoveryScanToggleRowCommand::ScanStarted {
                generation,
                outcome: start_discovery(&session, &adapter_name).await,
            }
        });
    }
}

#[relm4::component(pub)]
impl Component for DiscoveryScanToggleRow {
    type Init = DiscoveryScanToggleRowInit;
    type Input = DiscoveryScanToggleRowInput;
    type Output = BluetoothError;
    type CommandOutput = DiscoveryScanToggleRowCommand;

    view! {
        adw::ActionRow {
            #[watch]
            set_title: model.scan_row_title(),
            #[watch]
            set_subtitle: &model.scan_row_subtitle(),

            add_suffix = &gtk::ToggleButton {
                set_valign: gtk::Align::Center,
                #[watch]
                set_tooltip_text: Some(model.scan_button_tooltip()),
                #[watch]
                #[block_signal(scan_handler)]
                set_active: model.is_scanning(),
                connect_toggled[sender] => move |scan_button| {
                    sender.input(DiscoveryScanToggleRowInput::ScanToggled(scan_button.is_active()));
                } @scan_handler,

                gtk::Box {
                    set_spacing: 6,

                    adw::Spinner {
                        #[watch]
                        set_visible: model.is_scanning(),
                    },

                    gtk::Label {
                        #[watch]
                        set_label: model.scan_button_label(),
                    },
                },
            },
        }
    }

    fn init(
        row_init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {
            session: row_init.session,
            adapter_name: row_init.adapter_name,
            scan_state: ScanState::Idle,
            scan_generation: 0,
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match input {
            DiscoveryScanToggleRowInput::ScanToggled(true)
            | DiscoveryScanToggleRowInput::StartScan => self.start_scan(&sender),
            DiscoveryScanToggleRowInput::ScanToggled(false)
            | DiscoveryScanToggleRowInput::StopScan => self.scan_state = ScanState::Idle,
            DiscoveryScanToggleRowInput::AdapterSwitched(adapter_name) => {
                self.scan_state = ScanState::Idle;
                self.adapter_name = adapter_name;
            }
        }
    }

    fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match command_output {
            DiscoveryScanToggleRowCommand::ScanStarted {
                generation,
                outcome,
            } => match outcome {
                Ok(discovery) if generation == self.scan_generation => {
                    self.scan_state = ScanState::Scanning {
                        generation,
                        remaining_seconds: SCAN_DURATION_SECONDS,
                        _discovery: discovery,
                    };
                    sender.command(move |command_output, shutdown| {
                        shutdown
                            .register(
                                futures::stream::iter(0..SCAN_DURATION_SECONDS)
                                    .then(|_elapsed_seconds| tokio::time::sleep(SCAN_TICK))
                                    .for_each(move |()| {
                                        command_output.emit(
                                            DiscoveryScanToggleRowCommand::ScanTick(generation),
                                        );
                                        futures::future::ready(())
                                    }),
                            )
                            .drop_on_shutdown()
                    });
                }
                Ok(_stale_discovery) => {}
                Err(scan_error) => {
                    let _main_view_gone = sender.output(scan_error);
                }
            },
            DiscoveryScanToggleRowCommand::ScanTick(ticked_generation) => {
                let ScanState::Scanning {
                    generation,
                    remaining_seconds,
                    ..
                } = &mut self.scan_state
                else {
                    return;
                };
                if *generation != ticked_generation {
                    return;
                }
                *remaining_seconds = remaining_seconds.saturating_sub(1);
                if *remaining_seconds == 0 {
                    self.scan_state = ScanState::Idle;
                }
            }
        }
    }
}
