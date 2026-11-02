//! Windows system tray: icon, native menu, activation handling and the Win32
//! message loop, isolated on its own thread.
//!
//! The tray is the primary interface and must stay responsive while the
//! settings window is hidden. It owns no proxy state: user actions are
//! translated into `AppCommand`s or GUI events. tray-icon re-adds the icon
//! automatically after Explorer restarts (TaskbarCreated).

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};

use crate::app::commands::{AppCommand, GuiEvent};
use crate::app::state::{ProxyState, SharedAppState, StatusSnapshot};

use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    QS_ALLINPUT,
};

use super::instance::SendHandle;

/// Signals produced by menu/tray interaction. Handlers run on the tray thread
/// while messages are dispatched, so a simple queue is enough.
#[derive(Debug)]
enum TraySignal {
    ShowSettings,
    ToggleEnabled,
    ToggleAutostart,
    OpenLogs,
    Exit,
    TrayClick,
}

static PENDING_SIGNALS: Mutex<Vec<TraySignal>> = Mutex::new(Vec::new());

/// Recent-failure window in which the icon turns yellow.
const WARNING_WINDOW_MS: u64 = 30_000;

const GREEN: (u8, u8, u8) = (76, 175, 80);
const GRAY: (u8, u8, u8) = (130, 130, 138);
const YELLOW: (u8, u8, u8) = (255, 193, 7);
const RED: (u8, u8, u8) = (233, 84, 84);
const ICON_SIZE: u32 = 32;

enum TrayStatus {
    Running,
    Paused,
    Warning,
    Error,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn tray_status(snapshot: &StatusSnapshot) -> TrayStatus {
    match &snapshot.state {
        ProxyState::Error(_) => TrayStatus::Error,
        ProxyState::Paused => TrayStatus::Paused,
        ProxyState::Starting | ProxyState::Running => {
            if snapshot.last_failure_ms > 0
                && now_ms().saturating_sub(snapshot.last_failure_ms) < WARNING_WINDOW_MS
            {
                TrayStatus::Warning
            } else {
                TrayStatus::Running
            }
        }
    }
}

/// Renders a simple filled circle as RGBA pixels for the tray icon.
fn circle_rgba(color: (u8, u8, u8), size: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    let center = (size as f64 - 1.0) / 2.0;
    let fill_r = size as f64 * 0.40;
    let edge_r = size as f64 * 0.48;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 - center;
            let dy = y as f64 - center;
            let d = (dx * dx + dy * dy).sqrt();
            let alpha = if d <= fill_r {
                255u8
            } else if d < edge_r {
                (((edge_r - d) / (edge_r - fill_r)).clamp(0.0, 1.0) * 255.0) as u8
            } else {
                0
            };
            // Slightly darker rim for definition against light taskbars.
            let rim = d > fill_r * 0.72;
            let (r, g, b) = if rim {
                (color.0.saturating_sub(50), color.1.saturating_sub(50), color.2.saturating_sub(50))
            } else {
                color
            };
            data.extend_from_slice(&[r, g, b, alpha]);
        }
    }
    data
}

fn make_icon(color: (u8, u8, u8)) -> Result<Icon, Box<dyn std::error::Error>> {
    Ok(Icon::from_rgba(
        circle_rgba(color, ICON_SIZE),
        ICON_SIZE,
        ICON_SIZE,
    )?)
}

/// Spawns the tray thread. `activate_event` is the named event a second
/// launch signals to show the settings window.
pub fn spawn(
    state: Arc<SharedAppState>,
    cmd_tx: tokio::sync::mpsc::Sender<AppCommand>,
    activate_event: SendHandle,
    logs_dir: std::path::PathBuf,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("tray".to_string())
        .spawn(move || {
            if let Err(e) = run(state, cmd_tx, activate_event, logs_dir) {
                tracing::error!("tray thread terminated: {e}");
            }
        })
}

fn run(
    state: Arc<SharedAppState>,
    cmd_tx: tokio::sync::mpsc::Sender<AppCommand>,
    activate_event: SendHandle,
    logs_dir: std::path::PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let menu = Menu::new();
    let status_item = MenuItem::with_id("status", "\u{25cf} Starting", false, None);
    let conns_item = MenuItem::with_id("conns", "Active connections: 0", false, None);
    let enabled_item = CheckMenuItem::with_id("enabled", "Enabled", true, true, None);
    let open_item = MenuItem::with_id("open", "Open settings...", true, None);
    let autostart_item = CheckMenuItem::with_id("autostart", "Start with Windows", true, false, None);
    let logs_item = MenuItem::with_id("logs", "Open log folder", true, None);
    let exit_item = MenuItem::with_id("exit", "Exit", true, None);

    menu.append(&status_item)?;
    menu.append(&conns_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&enabled_item)?;
    menu.append(&open_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&autostart_item)?;
    menu.append(&logs_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&exit_item)?;

    let initial = state.live_snapshot();
    let icon = make_icon(GREEN)?;
    let tooltip = format!("escape-proxy \u{2014} {}", initial.listen_addr);
    let tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_tooltip(&tooltip)
        .build()?;

    let id_enabled = enabled_item.id().clone();
    let id_open = open_item.id().clone();
    let id_autostart = autostart_item.id().clone();
    let id_logs = logs_item.id().clone();
    let id_exit = exit_item.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let signal = if event.id == id_enabled {
            Some(TraySignal::ToggleEnabled)
        } else if event.id == id_open {
            Some(TraySignal::ShowSettings)
        } else if event.id == id_autostart {
            Some(TraySignal::ToggleAutostart)
        } else if event.id == id_logs {
            Some(TraySignal::OpenLogs)
        } else if event.id == id_exit {
            Some(TraySignal::Exit)
        } else {
            None
        };
        if let Some(s) = signal {
            PENDING_SIGNALS.lock().unwrap().push(s);
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => PENDING_SIGNALS.lock().unwrap().push(TraySignal::TrayClick),
            _ => {}
        }
    }));

    let activate = activate_event;
    let mut last_generation = u64::MAX;
    let mut msg = MSG::default();

    loop {
        if state.exit_requested.load(Ordering::SeqCst) {
            break;
        }

        // Handle menu and tray signals queued while dispatching messages.
        let signals = std::mem::take(&mut *PENDING_SIGNALS.lock().unwrap());
        for signal in signals {
            match signal {
                TraySignal::ShowSettings | TraySignal::TrayClick => {
                    state.push_event(GuiEvent::ShowSettings);
                }
                TraySignal::ToggleEnabled => {
                    let enable = !enabled_item.is_checked();
                    let _ = cmd_tx.blocking_send(AppCommand::SetEnabled(enable));
                }
                TraySignal::ToggleAutostart => {
                    let enable = !autostart_item.is_checked();
                    super::apply_autostart(&state, enable);
                }
                TraySignal::OpenLogs => super::open_folder(&logs_dir),
                TraySignal::Exit => {
                    state.exit_requested.store(true, Ordering::SeqCst);
                    state.push_event(GuiEvent::ExitRequested);
                }
            }
        }

        // Refresh icon and menu when the snapshot changed.
        let generation = state.generation();
        if generation != last_generation {
            last_generation = generation;
            let snapshot = state.live_snapshot();
            let (label, color) = match tray_status(&snapshot) {
                TrayStatus::Running => ("\u{25cf} Running".to_string(), GREEN),
                TrayStatus::Paused => ("\u{25cf} Paused".to_string(), GRAY),
                TrayStatus::Warning => ("\u{25cf} Warning".to_string(), YELLOW),
                TrayStatus::Error => {
                    let detail = match &snapshot.state {
                        ProxyState::Error(e) => e,
                        _ => "error",
                    };
                    (format!("\u{25cf} Error: {detail}"), RED)
                }
            };
            status_item.set_text(&label);
            conns_item.set_text(&format!("Active connections: {}", snapshot.active_connections));
            enabled_item.set_checked(matches!(snapshot.state, ProxyState::Running | ProxyState::Starting));
            autostart_item.set_checked(snapshot.autostart);
            tray.set_icon(Some(make_icon(color)?))?;
            tray.set_tooltip(Some(format!("escape-proxy \u{2014} {}", snapshot.listen_addr)))?;
        }

        // Wait for Win32 messages (tray icon/menu), the activation event, or
        // a 1 s tick. This sleeps in the kernel, so idle CPU stays ~zero.
        let wait = unsafe { MsgWaitForMultipleObjects(Some(&[activate.0]), false, 1000, QS_ALLINPUT) };
        let wait_val = wait.0;
        if wait_val == 0 {
            // WAIT_OBJECT_0: second launch activated us.
            state.push_event(GuiEvent::ShowSettings);
        } else if wait_val == 1 {
            // WAIT_OBJECT_0 + 1: a Win32 message is pending.
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        // WAIT_TIMEOUT (258): fall through and re-check state.
    }

    drop(tray); // removes the tray icon
    activate.close();
    tracing::info!("tray stopped");
    Ok(())
}
