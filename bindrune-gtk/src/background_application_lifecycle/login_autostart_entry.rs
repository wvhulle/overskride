use std::{
    fmt,
    path::{Path, PathBuf},
};

use ashpd::desktop::background::Background;

use crate::background_application_lifecycle::gsettings_stored_preferences::APPLICATION_ID;

pub const BACKGROUND_FLAG: &str = "--background";
const SYSTEMD_SERVICE_MARKER: &str = "BINDRUNE_SYSTEMD_SERVICE";
const BACKGROUND_REASON: &str = "Pair devices and receive files while the window is closed";

#[derive(Debug)]
pub enum LoginAutostartError {
    Portal(ashpd::Error),
    Filesystem(std::io::Error),
    MissingConfigFolder,
}

impl fmt::Display for LoginAutostartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Portal(portal_error) => {
                write!(formatter, "background portal refused: {portal_error}")
            }
            Self::Filesystem(io_error) => {
                write!(formatter, "cannot write the autostart entry: {io_error}")
            }
            Self::MissingConfigFolder => {
                write!(formatter, "neither XDG_CONFIG_HOME nor HOME is set")
            }
        }
    }
}

impl From<ashpd::Error> for LoginAutostartError {
    fn from(portal_error: ashpd::Error) -> Self {
        Self::Portal(portal_error)
    }
}

impl From<std::io::Error> for LoginAutostartError {
    fn from(io_error: std::io::Error) -> Self {
        Self::Filesystem(io_error)
    }
}

pub fn managed_by_systemd() -> bool {
    std::env::var_os(SYSTEMD_SERVICE_MARKER).is_some()
}

fn running_in_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

fn autostart_entry_path() -> Result<PathBuf, LoginAutostartError> {
    let config_folder = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home_folder| PathBuf::from(home_folder).join(".config"))
        })
        .ok_or(LoginAutostartError::MissingConfigFolder)?;
    Ok(config_folder
        .join("autostart")
        .join(format!("{APPLICATION_ID}.desktop")))
}

fn autostart_entry_text() -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Bindrune\nIcon={APPLICATION_ID}\\
         nExec=bindrune {BACKGROUND_FLAG}\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n"
    )
}

fn write_autostart_entry(start_at_login: bool) -> Result<(), LoginAutostartError> {
    let entry_path = autostart_entry_path()?;
    if start_at_login {
        if let Some(autostart_folder) = entry_path.parent() {
            std::fs::create_dir_all(autostart_folder)?;
        }
        std::fs::write(&entry_path, autostart_entry_text())?;
    } else if entry_path.exists() {
        std::fs::remove_file(&entry_path)?;
    }
    Ok(())
}

async fn request_portal_background(start_at_login: bool) -> Result<(), LoginAutostartError> {
    Background::request()
        .reason(BACKGROUND_REASON)
        .auto_start(start_at_login)
        .command(["bindrune", BACKGROUND_FLAG])
        .send()
        .await?
        .response()?;
    Ok(())
}

pub async fn apply_login_autostart(start_at_login: bool) -> Result<(), LoginAutostartError> {
    if managed_by_systemd() {
        return Ok(());
    }
    if running_in_flatpak() {
        request_portal_background(start_at_login).await
    } else {
        write_autostart_entry(start_at_login)
    }
}
