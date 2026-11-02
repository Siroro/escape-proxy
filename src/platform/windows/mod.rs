//! Windows integration: autostart, single instance and system tray.

pub mod autostart;
pub mod instance;
pub mod tray;

use std::path::Path;

use crate::app::state::SharedAppState;

/// Applies an autostart change through the registry and reflects the actual
/// registered state back into the shared snapshot.
pub fn apply_autostart(state: &SharedAppState, enable: bool) {
    if let Err(e) = autostart::set_enabled(enable) {
        tracing::error!("autostart change failed: {e}");
        return;
    }
    let actual = autostart::is_enabled().unwrap_or(enable);
    tracing::info!(enabled = actual, "autostart registration updated");
    state.set_autostart(actual);
}

/// Opens a folder in Windows Explorer.
pub fn open_folder(path: &Path) {
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}
