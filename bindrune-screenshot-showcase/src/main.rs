mod application_window_capture;
mod fake_bluez_fixture_devices;
mod fake_bluez_object_server;
mod headless_sway_session;

use std::{path::PathBuf, process::ExitCode, thread};

use crate::{
    application_window_capture::{ReadmeScreenshot, capture_application_window},
    fake_bluez_object_server::serve_fake_bluez,
    headless_sway_session::{HeadlessSwaySession, ScreenSize, wait_until_ready},
};

const README_SCREEN_SIZE: ScreenSize = ScreenSize {
    logical_width: 1100,
    logical_height: 720,
    scale: 2,
};

fn readme_screenshots() -> Vec<ReadmeScreenshot> {
    vec![
        ReadmeScreenshot {
            file_stem: "adapter_overview",
            application_arguments: Vec::new(),
        },
        ReadmeScreenshot {
            file_stem: "headset_pairing_timeline",
            application_arguments: vec!["--open-device=4C:87:5D:2A:91:0E".to_string()],
        },
    ]
}

fn fake_bluez_is_registered(session: &HeadlessSwaySession) -> bool {
    session
        .command("dbus-send")
        .args([
            "--system",
            "--print-reply",
            "--dest=org.bluez",
            "/",
            "org.freedesktop.DBus.ObjectManager.GetManagedObjects",
        ])
        .output()
        .is_ok_and(|reply_output| reply_output.status.success())
}

fn render_readme_screenshots(output_directory: PathBuf) -> Result<(), String> {
    std::fs::create_dir_all(&output_directory)
        .map_err(|filesystem_error| filesystem_error.to_string())?;
    let work_directory =
        std::env::temp_dir().join(format!("bindrune-screenshots-{}", std::process::id()));
    let session = HeadlessSwaySession::start(&work_directory, &README_SCREEN_SIZE)
        .map_err(|session_error| session_error.to_string())?;
    let system_bus_address = session.system_bus_address.clone();
    thread::spawn(move || {
        if let Err(serving_error) = serve_fake_bluez(&system_bus_address) {
            eprintln!("fake BlueZ stopped: {serving_error}");
        }
    });
    wait_until_ready("the fake BlueZ service", || {
        fake_bluez_is_registered(&session)
    })
    .map_err(|session_error| session_error.to_string())?;
    let capture_outcome = readme_screenshots()
        .iter()
        .try_for_each(|readme_screenshot| {
            capture_application_window(&session, readme_screenshot, &output_directory)
                .map_err(|capture_error| capture_error.to_string())
        });
    drop(session);
    let _work_directory_gone = std::fs::remove_dir_all(&work_directory);
    capture_outcome
}

fn main() -> ExitCode {
    let Some(output_directory) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: render-readme-screenshots OUTPUT_DIRECTORY");
        return ExitCode::FAILURE;
    };
    match render_readme_screenshots(output_directory) {
        Ok(()) => ExitCode::SUCCESS,
        Err(rendering_error) => {
            eprintln!("{rendering_error}");
            ExitCode::FAILURE
        }
    }
}
