use bindrune_bluez::{
    bluetooth_adapter::{
        adapter_snapshot_and_discovery::{AdapterSnapshot, read_adapter},
        adapter_udev_hardware::RadioDescription,
    },
    bluez_dbus_session::BluetoothSession,
    bluez_operation_error::BluetoothError,
    obex_file_transfer::obex_dbus_connection::ObexConnection,
};
use ksni::TrayMethods;
use relm4::{
    actions::{RelmAction, RelmActionGroup},
    adw,
    adw::prelude::*,
    component::{AsyncComponent, AsyncComponentParts, AsyncComponentSender},
    gtk,
    prelude::*,
};

use crate::{
    background_application_lifecycle::{
        application_menu_actions::{AppActionGroup, QuitAction, ShowWindowAction},
        gsettings_stored_preferences::{AppPreferences, PreferenceChange},
        login_autostart_entry::{LoginAutostartError, apply_login_autostart, managed_by_systemd},
        system_tray_menu::{OverskrideTray, TrayContent, TrayRequest, TrayScope},
    },
    device_detail::file_transfer_group::ObexAvailability,
    icon_names,
    sidebar_content_split::sidebar_content_split_view::{
        MainView, MainViewInit, MainViewInput, MainViewOutput,
    },
};

const NARROW_WINDOW_CONDITION: &str = "max-width: 700sp";
const BLUETOOTH_SERVICE_HELP: &str = "Start the Bluetooth service and open Overskride \
                                      again:\n\nsudo systemctl enable --now bluetooth\n\nIf the \
                                      service does not exist, install the bluez package for your \
                                      distribution first.";

struct ReadyBluetooth {
    session: BluetoothSession,
    radios: Vec<RadioDescription>,
    adapter: AdapterSnapshot,
}

pub enum WindowContent {
    Ready { main_view: Controller<MainView> },
    Unavailable,
}

impl WindowContent {
    fn tray_scope(&self) -> TrayScope {
        match self {
            Self::Ready { .. } => TrayScope::Bluetooth,
            Self::Unavailable => TrayScope::WindowOnly,
        }
    }
}

enum TrayPresence {
    Absent,
    Starting,
    Shown(ksni::Handle<OverskrideTray>),
}

pub struct AppWindowInit {
    pub preferences: AppPreferences,
}

pub struct AppWindow {
    content: WindowContent,
    run_in_background: bool,
    start_at_login: bool,
    tray_presence: TrayPresence,
    tray_content: TrayContent,
}

#[derive(Debug)]
pub enum AppWindowInput {
    MainView(MainViewOutput),
    PreferenceChanged(PreferenceChange),
    Tray(TrayRequest),
    OpenWindowRequested,
    QuitRequested,
}

pub enum AppWindowCommand {
    TrayStarted(Result<ksni::Handle<OverskrideTray>, ksni::Error>),
    TrayRefreshed,
    LoginAutostartApplied(Result<(), LoginAutostartError>),
}

impl std::fmt::Debug for AppWindowCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TrayStarted(tray_outcome) => {
                write!(formatter, "TrayStarted({})", tray_outcome.is_ok())
            }
            Self::TrayRefreshed => write!(formatter, "TrayRefreshed"),
            Self::LoginAutostartApplied(autostart_outcome) => {
                write!(formatter, "LoginAutostartApplied({autostart_outcome:?})")
            }
        }
    }
}

fn register_app_actions(sender: &AsyncComponentSender<AppWindow>) {
    let quit_sender = sender.input_sender().clone();
    let quit_action: RelmAction<QuitAction> = RelmAction::new_stateless(move |_quit_action| {
        quit_sender.emit(AppWindowInput::QuitRequested);
    });
    let show_window_sender = sender.input_sender().clone();
    let show_window_action: RelmAction<ShowWindowAction> =
        RelmAction::new_stateless(move |_show_window_action| {
            show_window_sender.emit(AppWindowInput::OpenWindowRequested);
        });
    let mut app_actions = RelmActionGroup::<AppActionGroup>::new();
    app_actions.add_action(quit_action);
    app_actions.add_action(show_window_action);
    app_actions.register_for_main_application();
}

impl AppWindow {
    fn keeps_running_in_background(&self) -> bool {
        managed_by_systemd() || self.run_in_background
    }

    fn apply_background_mode(
        &mut self,
        root: &adw::ApplicationWindow,
        sender: &AsyncComponentSender<Self>,
    ) {
        let keeps_running = self.keeps_running_in_background();
        root.set_hide_on_close(keeps_running);
        if keeps_running {
            self.show_tray(sender);
        } else {
            self.hide_tray();
        }
        Self::apply_login_autostart(sender, keeps_running && self.start_at_login);
    }

    fn show_tray(&mut self, sender: &AsyncComponentSender<Self>) {
        if !matches!(self.tray_presence, TrayPresence::Absent) {
            return;
        }
        self.tray_presence = TrayPresence::Starting;
        let tray = OverskrideTray {
            window_sender: sender.input_sender().clone(),
            scope: self.content.tray_scope(),
            content: self.tray_content.clone(),
        };
        sender.oneshot_command(async { AppWindowCommand::TrayStarted(tray.spawn().await) });
    }

    fn refresh_tray(&self, sender: &AsyncComponentSender<Self>) {
        if let TrayPresence::Shown(tray_handle) = &self.tray_presence {
            let tray_handle = tray_handle.clone();
            let tray_content = self.tray_content.clone();
            sender.oneshot_command(async move {
                tray_handle
                    .update(move |tray| tray.content = tray_content)
                    .await;
                AppWindowCommand::TrayRefreshed
            });
        }
    }

    fn handle_tray_request(&self, tray_request: TrayRequest, root: &adw::ApplicationWindow) {
        match (tray_request, &self.content) {
            (TrayRequest::OpenWindow, _) => root.present(),
            (TrayRequest::Quit, _) => relm4::main_application().quit(),
            (TrayRequest::ShowSettings, WindowContent::Ready { main_view }) => {
                main_view.emit(MainViewInput::Tray(TrayRequest::ShowSettings));
                root.present();
            }
            (bluetooth_request, WindowContent::Ready { main_view }) => {
                main_view.emit(MainViewInput::Tray(bluetooth_request));
            }
            (_bluetooth_request, WindowContent::Unavailable) => {}
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
            .icon_name(icon_names::custom::NO_BLUETOOTH_SYMBOLIC)
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
        main_view.model().content_navigation(),
        "collapsed",
        Some(&true.to_value()),
    );
    window.add_breakpoint(breakpoint);
}

impl AsyncComponent for AppWindow {
    type Init = AppWindowInit;
    type Input = AppWindowInput;
    type Output = ();
    type CommandOutput = AppWindowCommand;
    type Root = adw::ApplicationWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::builder()
            .title("Overskride")
            .width_request(360)
            .height_request(500)
            .default_width(900)
            .default_height(700)
            .build()
    }

    async fn init(
        window_init: Self::Init,
        root: Self::Root,
        sender: AsyncComponentSender<Self>,
    ) -> AsyncComponentParts<Self> {
        let preferences = window_init.preferences;
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
        let preference_sender = sender.input_sender().clone();
        preferences.connect_preference_changes(move |preference_change| {
            preference_sender.emit(AppWindowInput::PreferenceChanged(preference_change));
        });
        let activation_sender = sender.input_sender().clone();
        relm4::main_application().connect_activate(move |_activated_application| {
            activation_sender.emit(AppWindowInput::OpenWindowRequested);
        });
        register_app_actions(&sender);
        let run_in_background = preferences.run_in_background();
        let start_at_login = preferences.start_at_login();

        let content = match connect_bluetooth(&preferences).await {
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
                WindowContent::Ready { main_view }
            }
            Err(bluetooth_error) => {
                root.set_content(Some(&unavailable_page(bluetooth_error)));
                WindowContent::Unavailable
            }
        };
        let mut model = Self {
            content,
            run_in_background,
            start_at_login,
            tray_presence: TrayPresence::Absent,
            tray_content: TrayContent::default(),
        };
        model.apply_background_mode(&root, &sender);
        AsyncComponentParts { model, widgets: () }
    }

    async fn update(
        &mut self,
        input: Self::Input,
        sender: AsyncComponentSender<Self>,
        root: &Self::Root,
    ) {
        match input {
            AppWindowInput::MainView(MainViewOutput::TrayContentChanged(tray_content)) => {
                self.tray_content = tray_content;
                self.refresh_tray(&sender);
            }
            AppWindowInput::PreferenceChanged(PreferenceChange::RunInBackground(
                run_in_background,
            )) => {
                self.run_in_background = run_in_background;
                self.apply_background_mode(root, &sender);
            }
            AppWindowInput::PreferenceChanged(PreferenceChange::StartAtLogin(start_at_login)) => {
                self.start_at_login = start_at_login;
                self.apply_background_mode(root, &sender);
            }
            AppWindowInput::PreferenceChanged(
                PreferenceChange::ShowUnnamedDevices(_)
                | PreferenceChange::AutoAcceptSenders
                | PreferenceChange::Unrelated,
            ) => {}
            AppWindowInput::Tray(tray_request) => self.handle_tray_request(tray_request, root),
            AppWindowInput::OpenWindowRequested => root.present(),
            AppWindowInput::QuitRequested => relm4::main_application().quit(),
        }
    }

    async fn update_cmd(
        &mut self,
        command_output: Self::CommandOutput,
        sender: AsyncComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match command_output {
            AppWindowCommand::TrayStarted(Ok(tray_handle)) => {
                if matches!(self.tray_presence, TrayPresence::Starting) {
                    self.tray_presence = TrayPresence::Shown(tray_handle);
                    self.refresh_tray(&sender);
                } else {
                    let _shutdown_runs_without_waiting = tray_handle.shutdown();
                }
            }
            AppWindowCommand::TrayStarted(Err(tray_error)) => {
                self.tray_presence = TrayPresence::Absent;
                eprintln!("No tray icon: {tray_error}");
            }
            AppWindowCommand::TrayRefreshed | AppWindowCommand::LoginAutostartApplied(Ok(())) => {}
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
