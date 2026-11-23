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

/// Switches a window's native title bar to dark mode so it blends with a
/// dark UI. Attribute 20 covers Windows 10 2004+; 19 is the same flag on
/// older builds. Failures are logged and otherwise ignored.
pub fn set_dark_titlebar(hwnd: *mut core::ffi::c_void) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE,
    };

    let hwnd = HWND(hwnd as _);
    let dark: i32 = 1;
    let ptr = &dark as *const i32 as *const core::ffi::c_void;
    let size = std::mem::size_of::<i32>() as u32;
    unsafe {
        // DWMWA_USE_IMMERSIVE_DARK_MODE is value 20 on Windows 10 2004+;
        // older builds accept the same flag as a raw 19.
        let mut ok = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, ptr, size).is_ok();
        if !ok {
            let old = DWMWINDOWATTRIBUTE(DWMWA_USE_IMMERSIVE_DARK_MODE.0 - 1);
            ok = DwmSetWindowAttribute(hwnd, old, ptr, size).is_ok();
        }
        if !ok {
            tracing::warn!("dark title bar unavailable");
        }
    }
}
