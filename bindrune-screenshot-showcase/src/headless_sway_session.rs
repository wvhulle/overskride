use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(200);
const READINESS_TIMEOUT: Duration = Duration::from_secs(30);

pub struct ScreenSize {
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale: u32,
}

pub struct HeadlessSwaySession {
    pub system_bus_address: String,
    environment_variables: Vec<(String, String)>,
    background_processes: Vec<Child>,
}

#[derive(Debug)]
pub enum HeadlessSessionError {
    Filesystem(std::io::Error),
    NeverReady(String),
}

impl From<std::io::Error> for HeadlessSessionError {
    fn from(filesystem_error: std::io::Error) -> Self {
        Self::Filesystem(filesystem_error)
    }
}

impl std::fmt::Display for HeadlessSessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Filesystem(filesystem_error) => write!(formatter, "{filesystem_error}"),
            Self::NeverReady(awaited_condition) => {
                write!(formatter, "timed out waiting for {awaited_condition}")
            }
        }
    }
}

pub fn wait_until_ready(
    awaited_condition: &str,
    mut readiness_check: impl FnMut() -> bool,
) -> Result<(), HeadlessSessionError> {
    let waiting_since = Instant::now();
    while !readiness_check() {
        if waiting_since.elapsed() > READINESS_TIMEOUT {
            return Err(HeadlessSessionError::NeverReady(
                awaited_condition.to_string(),
            ));
        }
        thread::sleep(READINESS_POLL_INTERVAL);
    }
    Ok(())
}

fn write_sway_config(
    config_directory: &Path,
    screen_size: &ScreenSize,
) -> std::io::Result<PathBuf> {
    let sway_config_path = config_directory.join("sway_screenshot_config");
    fs::write(
        &sway_config_path,
        format!(
            "output * resolution {}x{} scale {}\ndefault_border none\ndefault_floating_border \
             none\nfocus_follows_mouse no\n",
            screen_size.logical_width * screen_size.scale,
            screen_size.logical_height * screen_size.scale,
            screen_size.scale,
        ),
    )?;
    Ok(sway_config_path)
}

fn write_isolated_bus_config(
    config_directory: &Path,
    bus_name: &str,
    bus_address: &str,
) -> std::io::Result<PathBuf> {
    let bus_config_path = config_directory.join(format!("{bus_name}_bus.conf"));
    fs::write(
        &bus_config_path,
        format!(
            "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
             <busconfig>\n\
             <type>session</type>\n\
             <listen>{bus_address}</listen>\n\
             <auth>EXTERNAL</auth>\n\
             <policy context=\"default\">\n\
             <allow send_destination=\"*\" eavesdrop=\"true\"/>\n\
             <allow eavesdrop=\"true\"/>\n\
             <allow own=\"*\"/>\n\
             </policy>\n\
             </busconfig>\n"
        ),
    )?;
    Ok(bus_config_path)
}

fn find_wayland_socket_name(runtime_directory: &Path) -> Option<String> {
    fs::read_dir(runtime_directory)
        .ok()?
        .filter_map(Result::ok)
        .map(|directory_entry| directory_entry.file_name().to_string_lossy().to_string())
        .find(|file_name| file_name.starts_with("wayland-") && !file_name.ends_with(".lock"))
}

impl HeadlessSwaySession {
    pub fn start(
        work_directory: &Path,
        screen_size: &ScreenSize,
    ) -> Result<Self, HeadlessSessionError> {
        let home_directory = work_directory.join("home");
        let config_directory = home_directory.join(".config");
        let runtime_directory = work_directory.join("runtime");
        fs::create_dir_all(&config_directory)?;
        fs::create_dir_all(home_directory.join(".cache"))?;
        fs::create_dir_all(home_directory.join(".local/share"))?;
        fs::create_dir_all(&runtime_directory)?;
        fs::set_permissions(&runtime_directory, fs::Permissions::from_mode(0o700))?;

        let system_bus_address = format!(
            "unix:path={}",
            runtime_directory.join("system_bus_socket").display()
        );
        let session_bus_address = format!(
            "unix:path={}",
            runtime_directory.join("session_bus_socket").display()
        );
        let mut session = Self {
            system_bus_address: system_bus_address.clone(),
            environment_variables: vec![
                ("HOME".to_string(), home_directory.display().to_string()),
                (
                    "XDG_CONFIG_HOME".to_string(),
                    config_directory.display().to_string(),
                ),
                (
                    "XDG_CACHE_HOME".to_string(),
                    home_directory.join(".cache").display().to_string(),
                ),
                (
                    "XDG_DATA_HOME".to_string(),
                    home_directory.join(".local/share").display().to_string(),
                ),
                (
                    "XDG_RUNTIME_DIR".to_string(),
                    runtime_directory.display().to_string(),
                ),
                (
                    "DBUS_SYSTEM_BUS_ADDRESS".to_string(),
                    system_bus_address.clone(),
                ),
                (
                    "DBUS_SESSION_BUS_ADDRESS".to_string(),
                    session_bus_address.clone(),
                ),
                ("GSETTINGS_BACKEND".to_string(), "memory".to_string()),
                ("GSK_RENDERER".to_string(), "cairo".to_string()),
                ("GDK_BACKEND".to_string(), "wayland".to_string()),
                ("NO_AT_BRIDGE".to_string(), "1".to_string()),
                ("WLR_BACKENDS".to_string(), "headless".to_string()),
                ("WLR_RENDERER".to_string(), "pixman".to_string()),
                ("WLR_LIBINPUT_NO_DEVICES".to_string(), "1".to_string()),
                (
                    "SWAYSOCK".to_string(),
                    runtime_directory
                        .join("sway-ipc.sock")
                        .display()
                        .to_string(),
                ),
            ],
            background_processes: Vec::new(),
        };

        for (bus_name, bus_address) in [
            ("system", &system_bus_address),
            ("session", &session_bus_address),
        ] {
            let bus_config_path =
                write_isolated_bus_config(&config_directory, bus_name, bus_address)?;
            let bus_daemon = session
                .command("dbus-daemon")
                .args(["--nofork", "--nopidfile"])
                .arg(format!("--config-file={}", bus_config_path.display()))
                .stdout(Stdio::null())
                .spawn()?;
            session.background_processes.push(bus_daemon);
        }
        wait_until_ready("the D-Bus sockets", || {
            runtime_directory.join("system_bus_socket").exists()
                && runtime_directory.join("session_bus_socket").exists()
        })?;

        let sway_config_path = write_sway_config(&config_directory, screen_size)?;
        let sway_log = fs::File::create(work_directory.join("sway.log"))?;
        let sway_compositor = session
            .command("sway")
            .arg("--config")
            .arg(&sway_config_path)
            .stdout(Stdio::null())
            .stderr(sway_log)
            .spawn()?;
        session.background_processes.push(sway_compositor);
        wait_until_ready("the sway IPC socket", || {
            session
                .command("swaymsg")
                .args(["-t", "get_version"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|exit_status| exit_status.success())
        })?;
        let wayland_socket_name = find_wayland_socket_name(&runtime_directory)
            .ok_or_else(|| HeadlessSessionError::NeverReady("the Wayland socket".to_string()))?;
        session
            .environment_variables
            .push(("WAYLAND_DISPLAY".to_string(), wayland_socket_name));
        Ok(session)
    }

    pub fn command(&self, program_name: &str) -> Command {
        let mut session_command = Command::new(program_name);
        session_command.envs(self.environment_variables.iter().cloned());
        session_command
    }
}

impl Drop for HeadlessSwaySession {
    fn drop(&mut self) {
        self.background_processes
            .iter_mut()
            .rev()
            .for_each(|background_process| {
                let _already_exited = background_process.kill();
                let _exit_status = background_process.wait();
            });
    }
}
