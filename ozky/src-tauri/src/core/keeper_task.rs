//! OS scheduled-task registration for the local headless keeper (K6, free tier).
//!
//! Registers a recurring Windows Task Scheduler task that runs `ozky-keeper --once --cred <path>`
//! near due times; tears it down on disable. The task carries NO secret on its command line — the
//! `--cred` file (written by [`super::keeper::write_local_cred`]) holds the `notes_key` + paths, and
//! `notes_key` cannot derive `owner_sk`, so the scheduled host can relay pre-authorized runs but not
//! forge one. Windows-only for v1 (macOS `launchd` / Linux `systemd --user` are additive later).

use super::CoreError;
use std::path::Path;

/// The Task Scheduler task name (one local keeper per machine in v1).
pub const TASK_NAME: &str = "ozky-keeper";

#[cfg(target_os = "windows")]
use std::process::Command;

/// Register (or replace) a task that runs `<exe> --once --cred <cred>` every `minutes`.
#[cfg(target_os = "windows")]
pub fn register(exe: &Path, cred: &Path, minutes: u32) -> Result<(), CoreError> {
    let tr = format!("\"{}\" --once --cred \"{}\"", exe.display(), cred.display());
    let out = Command::new("schtasks")
        .args([
            "/Create",
            "/F", // replace if it exists
            "/SC",
            "MINUTE",
            "/MO",
            &minutes.max(1).to_string(),
            "/TN",
            TASK_NAME,
            "/TR",
            &tr,
        ])
        .output()
        .map_err(|e| CoreError::Crypto(format!("spawn schtasks: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::Crypto(format!(
            "schtasks create failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// Delete the task. Treats "not found" as success (idempotent disable).
#[cfg(target_os = "windows")]
pub fn unregister() -> Result<(), CoreError> {
    let out = Command::new("schtasks")
        .args(["/Delete", "/F", "/TN", TASK_NAME])
        .output()
        .map_err(|e| CoreError::Crypto(format!("spawn schtasks: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
        if !err.contains("cannot find") && !err.contains("does not exist") {
            return Err(CoreError::Crypto(format!(
                "schtasks delete failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
    }
    Ok(())
}

/// Whether the task currently exists.
#[cfg(target_os = "windows")]
pub fn is_registered() -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", TASK_NAME])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// Non-Windows stubs (the local tier is Windows-first; other hosts come later).
#[cfg(not(target_os = "windows"))]
pub fn register(_exe: &Path, _cred: &Path, _minutes: u32) -> Result<(), CoreError> {
    Err(CoreError::not_implemented(
        "local keeper task registration is Windows-only in v1",
    ))
}

#[cfg(not(target_os = "windows"))]
pub fn unregister() -> Result<(), CoreError> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn is_registered() -> bool {
    false
}
