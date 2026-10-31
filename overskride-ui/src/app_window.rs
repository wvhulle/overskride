use overskride_bluetooth::adapter_state::{AdapterSnapshot, read_adapter};
use overskride_bluetooth::bluetooth_error::BluetoothError;
use overskride_bluetooth::obex_transfers::ObexConnection;
use overskride_bluetooth::radios::RadioDescription;
use overskride_bluetooth::session::BluetoothSession;
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::component::{AsyncComponent, AsyncComponentParts, AsyncComponentSender};
use relm4::gtk;
use relm4::prelude::*;

use ksni::TrayMethods;

use crate::app_preferences::{AppPreferences, RUN_IN_BACKGROUND_KEY, START_AT_LOGIN_KEY};
use crate::device_page::ObexAvailability;
use crate::login_autostart::{LoginAutostartError, apply_login_autostart, managed_by_systemd};
use crate::main_view::{MainView, MainViewInit, MainViewInput, MainViewOutput};
use crate::tray_icon::{OverskrideTray, TrayContent, TrayControl, TrayRequest};

const NARROW_WINDOW_CONDITION: &str = "max-width: 700sp";
const BLUETOOTH_SERVICE_HELP: &str = "Start the Bluetooth service and open Overskride again:\n\nsudo systemctl enable --now bluetooth\n\nIf the service does not exist, install the bluez package for your distribution first.";

struct ReadyBluetooth {
    session: BluetoothSession,
    radios: Vec<RadioDescription>,
    adapter: AdapterSnapshot,
}

pub enum WindowContent {
    Ready { _main_view: Controller<MainView> },
    Unavailable,
}

enum TrayPresence {
    Absent,
    Starting,
    Shown(ksni::Handle<OverskrideTray>),
}

pub struct AppWindow {
    _content: WindowContent,
    tray_presence: TrayPresence,
    tray_control: TrayControl,
    tray_content: TrayContent,
}

#[derive(Debug)]
pub enum AppWindowInput {
    MainView(MainViewOutput),
    RunInBackgroundChanged(bool),
    StartAtLoginChanged(bool),
}

pub enum AppWindowCommand {
    TrayStarted(Result<ksni::Handle<OverskrideTray>, ksni::Error>),
    LoginAutostartApplied(Result<(), LoginAutostartError>),
}

impl std::fmt::Debug for AppWindowCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TrayStarted(tray_outcome) => {
                write!(formatter, "TrayStarted({})", tray_outcome.is_ok())
            }
            Self::LoginAutostartApplied(autostart_outcome) => {
                write!(formatter, "LoginAutostartApplied({autostart_outcome:?})")
            }
        }
    }
}

fn keeps_running_in_background(preferences: &AppPreferences) -> bool {
    managed_by_systemd() || preferences.run_in_background()
}

impl AppWindow {
    fn show_tray(&mut self, sender: &AsyncComponentSender<Self>) {
        if !matches!(self.tray_presence, TrayPresence::Absent) {
            return;
        }
        self.tray_presence = TrayPresence::Starting;
        let tray = OverskrideTray {
            control: self.tray_control.clone(),
            content: self.tray_content.clone(),
        };
        sender.oneshot_command(async { AppWindowCommand::TrayStarted(tray.spawn().await) });
    }

    fn show_tray_content(&mut self, tray_content: TrayContent) {
        self.tray_content = tray_content.clone();
        if let TrayPresence::Shown(tray_handle) = &self.tray_presence {
            let tray_handle = tray_handle.clone();
            relm4::spawn(async move {
                tray_handle
                    .update(move |tray| tray.content = tray_content)
                    .await;
            });
        }
    }

    fn hide_tray(&mut self) {
        if let TrayPresence::Shown(tray_handle) =
            std::mem::replace(&mut self.tray_presence, TrayPresence::Absent)
        {
            let _shutdown_runs_without_waiting = tray_handle.shutdown();
        }
    }

    fn apply_login_autostart(sender: &AsyncComponentSender<Self>, start_at_login: bool) {
        sender.oneshot_command(async move {
            AppWindowCommand::LoginAutostartApplied(apply_login_autostart(start_at_login).await)
        });
    }
}

async fn connect_bluetooth(preferences: &AppPreferences) -> Result<ReadyBluetooth, BluetoothError> {
    let session = BluetoothSession::open().await?;
    let radios = session.radios().await?;
    let remembered_adapter_name = preferences.remembered_adapter_name();
    let adapter_name = if radios
        .iter()
        .any(|radio| radio.adapter_name == remembered_adapter_name)
    {
        remembered_adapter_name
    } else {
        session.default_adapter_name().await?
    };
    let adapter = read_adapter(&session, &adapter_name).await?;
    Ok(ReadyBluetooth {
        session,
        radios,
        adapter,
    })
}

fn unavailable_page(bluetooth_error: BluetoothError) -> adw::ToolbarView {
    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&adw::HeaderBar::new());
    toolbar_view.set_content(Some(
        &adw::StatusPage::builder()
            .icon_name("no-bluetooth-symbolic")
            .title("Bluetooth Is Not Available")
            .description(format!("{bluetooth_error}\n\n{BLUETOOTH_SERVICE_HELP}"))
            .build(),
    ));
    toolbar_view
}

fn collapse_when_narrow(window: &adw::ApplicationWindow, main_view: &Controller<MainView>) {
    let Ok(narrow_condition) = adw::BreakpointCondition::parse(NARROW_WINDOW_CONDITION) else {
        return;
    };
    let breakpoint = adw::Breakpoint::new(narrow_condition);
    breakpoint.add_setter(
        &main_view.widgets().split_view,
        "collapsed",
        Some(&true.to_value()),
    );
    window.add_breakpoint(breakpoint);
}

impl AsyncComponent for AppWindow {
    type Init = AppPreferences;
    type Input = AppWindowInput;
    type Output = ();
    type CommandOutput = AppWindowCommand;
    type Root = adw::ApplicationWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        relm4::set_global_css(include_str!("gtk/style.css"));
        adw::ApplicationWindow::builder()
            .title("Overskride")
            .width_request(360)
            .height_request(500)
            .default_width(900)
            .default_height(700)
            .build()
    }

    async fn init(
        preferences: Self::Init,
        root: Self::Root,
        sender: AsyncComponentSender<Self>,
    ) -> AsyncComponentParts<Self> {
        let settings = preferences.settings();
        settings
            .bind("window-width", &root, "default-width")
            .build();
        settings
            .bind("window-height", &root, "default-height")
            .build();
        settings
            .bind("window-maximized", &root, "maximized")
            .build();
        if managed_by_systemd() {
            root.set_hide_on_close(true);
        } else {
            settings
                .bind(RUN_IN_BACKGROUND_KEY, &root, "hide-on-close")
                .get()
                .build();
        }
        let run_in_background_sender = sender.input_sender().clone();
        settings.connect_changed(
            Some(RUN_IN_BACKGROUND_KEY),
            move |changed_settings, setting_key| {
                run_in_background_sender.emit(AppWindowInput::RunInBackgroundChanged(
                    changed_settings.boolean(setting_key),
                ));
            },
        );
        let start_at_login_sender = sender.input_sender().clone();
        settings.connect_changed(
            Some(START_AT_LOGIN_KEY),
            move |changed_settings, setting_key| {
                start_at_login_sender.emit(AppWindowInput::StartAtLoginChanged(
                    changed_settings.boolean(setting_key),
                ));
            },
        );
        let background_wanted = keeps_running_in_background(&preferences);
        Self::apply_login_autostart(&sender, background_wanted && preferences.start_at_login());

        let (content, tray_control) = match connect_bluetooth(&preferences).await {
            Ok(ready_bluetooth) => {
                let obex = match ObexConnection::open() {
                    Ok(obex_connection) => ObexAvailability::Available(obex_connection),
                    Err(_obex_missing) => ObexAvailability::Unavailable,
                };
                let main_view = MainView::builder()
                    .launch(MainViewInit {
                        session: ready_bluetooth.session,
                        obex,
                        preferences,
                        radios: ready_bluetooth.radios,
                        adapter: ready_bluetooth.adapter,
                    })
                    .forward(sender.input_sender(), AppWindowInput::MainView);
                root.set_content(Some(main_view.widget()));
                collapse_when_narrow(&root, &main_view);
                let (tray_request_sender, tray_request_receiver) = relm4::channel::<TrayRequest>();
                relm4::spawn_local(
                    tray_request_receiver.forward(main_view.sender().clone(), MainViewInput::Tray),
                );
                (
                    WindowContent::Ready {
                        _main_view: main_view,
                    },
                    TrayControl::Bluetooth(tray_request_sender),
                )
            }
            Err(bluetooth_error) => {
                root.set_content(Some(&unavailable_page(bluetooth_error)));
                (WindowContent::Unavailable, TrayControl::WindowOnly)
            }
        };
        let mut model = Self {
            _content: content,
            tray_presence: TrayPresence::Absent,
            tray_control,
            tray_content: TrayContent::default(),
        };
        if background_wanted {
            model.show_tray(&sender);
        }
        AsyncComponentParts { model, widgets: () }
    }

    async fn update(
        &mut self,
        input: Self::Input,
        sender: AsyncComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match input {
            AppWindowInput::MainView(MainViewOutput::TrayContentChanged(tray_content)) => {
                self.show_tray_content(tray_content);
            }
            AppWindowInput::RunInBackgroundChanged(run_in_background) => {
                let preferences = AppPreferences::load();
                if run_in_background || managed_by_systemd() {
                    self.show_tray(&sender);
                } else {
                    self.hide_tray();
                }
                Self::apply_login_autostart(
                    &sender,
                    run_in_background && preferences.start_at_login(),
                );
            }
            AppWindowInput::StartAtLoginChanged(start_at_login) => {
                let preferences = AppPreferences::load();
                Self::apply_login_autostart(
                    &sender,
                    start_at_login && keeps_running_in_background(&preferences),
                );
            }
        }
    }

    async fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        _sender: AsyncComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match command_output {
            AppWindowCommand::TrayStarted(Ok(tray_handle)) => {
                if matches!(self.tray_presence, TrayPresence::Starting) {
                    self.tray_presence = TrayPresence::Shown(tray_handle);
                } else {
                    let _shutdown_runs_without_waiting = tray_handle.shutdown();
                }
            }
            AppWindowCommand::TrayStarted(Err(tray_error)) => {
                self.tray_presence = TrayPresence::Absent;
                eprintln!("No tray icon: {tray_error}");
            }
            AppWindowCommand::LoginAutostartApplied(Ok(())) => {}
            AppWindowCommand::LoginAutostartApplied(Err(autostart_error)) => {
                eprintln!("Start at login not applied: {autostart_error}");
            }
        }
    }

    fn init_loading_widgets(root: Self::Root) -> Option<relm4::loading_widgets::LoadingWidgets> {
        let loading_spinner = gtk::Spinner::builder()
            .spinning(true)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        root.set_content(Some(&loading_spinner));
        Some(relm4::loading_widgets::LoadingWidgets::new(
            root,
            loading_spinner,
        ))
    }
}
