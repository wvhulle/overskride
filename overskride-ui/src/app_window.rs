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

use crate::app_preferences::AppPreferences;
use crate::device_page::ObexAvailability;
use crate::main_view::{MainView, MainViewInit};

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

pub struct AppWindow {
    _content: WindowContent,
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
    type Input = ();
    type Output = ();
    type CommandOutput = ();
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
        _sender: AsyncComponentSender<Self>,
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
                    .detach();
                root.set_content(Some(main_view.widget()));
                collapse_when_narrow(&root, &main_view);
                WindowContent::Ready {
                    _main_view: main_view,
                }
            }
            Err(bluetooth_error) => {
                root.set_content(Some(&unavailable_page(bluetooth_error)));
                WindowContent::Unavailable
            }
        };
        AsyncComponentParts {
            model: Self { _content: content },
            widgets: (),
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
