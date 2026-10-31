use std::{
    path::Path,
    process::{Child, Stdio},
    thread,
    time::Duration,
};

use crate::headless_sway_session::{HeadlessSessionError, HeadlessSwaySession, wait_until_ready};

const APPLICATION_ID: &str = "page.codeberg.wvhulle.Bindrune";
const APPLICATION_BINARY: &str = "bindrune";
const RENDER_SETTLE_TIME: Duration = Duration::from_secs(3);

pub struct ReadmeScreenshot {
    pub file_stem: &'static str,
    pub application_arguments: Vec<String>,
}

struct RunningApplication(Child);

impl Drop for RunningApplication {
    fn drop(&mut self) {
        let _already_exited = self.0.kill();
        let _exit_status = self.0.wait();
    }
}

fn application_window_is_mapped(session: &HeadlessSwaySession) -> bool {
    session
        .command("swaymsg")
        .args(["-t", "get_tree"])
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|tree_output| {
            String::from_utf8_lossy(&tree_output.stdout)
                .contains(&format!("\"app_id\": \"{APPLICATION_ID}\""))
        })
}

pub fn capture_application_window(
    session: &HeadlessSwaySession,
    readme_screenshot: &ReadmeScreenshot,
    output_directory: &Path,
) -> Result<(), HeadlessSessionError> {
    let _running_application = RunningApplication(
        session
            .command(APPLICATION_BINARY)
            .args(&readme_screenshot.application_arguments)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    wait_until_ready("the Bindrune window", || {
        application_window_is_mapped(session)
    })?;
    thread::sleep(RENDER_SETTLE_TIME);
    let screenshot_path = output_directory.join(format!("{}.png", readme_screenshot.file_stem));
    let grim_status = session.command("grim").arg(&screenshot_path).status()?;
    if !grim_status.success() {
        return Err(HeadlessSessionError::NeverReady(format!(
            "grim to write {}",
            screenshot_path.display()
        )));
    }
    println!("rendered {}", screenshot_path.display());
    Ok(())
}
