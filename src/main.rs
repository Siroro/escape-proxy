//! escape-proxy: lightweight Windows SOCKS5 proxy, tray application and
//! settings GUI.
//!
//! `main.rs` wires the components together: single-instance guard, logging,
//! Tokio runtime + controller, FLTK window and the tray thread. It contains
//! no networking protocol or GUI business logic.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gui;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use fltk::prelude::*;

use escape_proxy::app::commands::{AppCommand, GuiEvent};
use escape_proxy::app::config;
use escape_proxy::app::controller;
use escape_proxy::app::state::{ProxyState, SharedAppState, StatusSnapshot};
use escape_proxy::logging;
use escape_proxy::platform::windows::{autostart, instance, tray};

thread_local! {
    /// The periodic UI refresh runs from an FLTK timeout (which fires inside
    /// `wait()` on this thread), so it reaches the window through here.
    static UI: RefCell<Option<(Arc<SharedAppState>, Rc<RefCell<gui::window::MainWindow>>)>> =
        const { RefCell::new(None) };
}

fn ui_tick(handle: fltk::app::TimeoutHandle) {
    // One-shot timeout: re-arm, then refresh the window if it is visible.
    fltk::app::repeat_timeout3(0.5, handle);
    UI.with(|slot| {
        let borrowed = slot.borrow();
        if let Some((state, window)) = borrowed.as_ref() {
            let visible = window.borrow().win.visible();
            if visible {
                window.borrow_mut().tick(state, false);
            }
        }
    });
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let launch_hidden = args.iter().any(|a| a == "--background");

    // --- single instance ----------------------------------------------------
    let _instance = match instance::SingleInstance::try_acquire() {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            // Second launch: the running instance has been asked to show its
            // settings window.
            return;
        }
        Err(e) => {
            eprintln!("escape-proxy: instance guard failed: {e}");
            return;
        }
    };

    let root = config::app_root();
    let logs_dir = root.join("logs");
    let _log_guard = logging::init(&logs_dir);
    tracing::info!("escape-proxy starting (pid {})", std::process::id());

    // --- runtime + controller ----------------------------------------------
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let initial = StatusSnapshot {
        state: ProxyState::Starting,
        listen_addr: "127.0.0.1:10808".to_string(),
        active_connections: 0,
        tx_bytes: 0,
        rx_bytes: 0,
        domains: Vec::new(),
        restrict_domains: false,
        autostart: autostart::is_enabled().unwrap_or(false),
        show_on_startup: false,
        max_connections: 128,
        connect_timeout_ms: 5000,
        last_failure_ms: 0,
        last_error: None,
    };
    let state = SharedAppState::new(initial);
    let cmd_tx = rt.block_on(controller::start(config::config_path(&root), state.clone()));

    // --- GUI ------------------------------------------------------------------
    let _app = fltk::app::App::default();
    gui::theme::apply();
    let show_at_start = !launch_hidden || state.snapshot().show_on_startup;
    let window = Rc::new(RefCell::new(gui::window::build(
        state.clone(),
        cmd_tx.clone(),
        root.clone(),
        show_at_start,
    )));
    // Wake callback must exist before the tray thread starts.
    state.set_wake_gui(Box::new(|| fltk::app::awake()));

    // --- tray -------------------------------------------------------------------
    let tray_thread = match instance::create_activation_event() {
        Ok(activate_event) => {
            tray::spawn(state.clone(), cmd_tx.clone(), activate_event, logs_dir.clone()).ok()
        }
        Err(e) => {
            tracing::error!("activation event unavailable: {e}");
            None
        }
    };

    // --- FLTK event loop -----------------------------------------------------
    // `app::run()`/`wait()`-style blocking is required: with this fltk
    // version, the timed `wait_for()` variant sleeps but never dispatches
    // widget events, which made every control dead. `wait()` blocks in the
    // OS message queue (idle CPU stays ~0) and `awake()` wakes it for
    // cross-thread events. When all windows are hidden (tray-only),
    // `wait()` returns immediately, so a short sleep throttles the loop.
    UI.with(|slot| *slot.borrow_mut() = Some((state.clone(), window.clone())));
    fltk::app::add_timeout3(0.5, ui_tick);

    loop {
        if state.exit_requested.load(Ordering::SeqCst) {
            break;
        }
        let more = fltk::app::wait();

        let mut exit = false;
        for event in state.take_events() {
            match event {
                GuiEvent::ShowSettings => window.borrow_mut().show_window(),
                GuiEvent::StatusChanged => {
                    let visible = window.borrow().win.visible();
                    if visible {
                        window.borrow_mut().tick(&state, true);
                    }
                }
                GuiEvent::AutostartChanged(_) => {
                    // Reflected by the regular tick.
                }
                GuiEvent::ExitRequested => exit = true,
            }
        }
        if exit || state.exit_requested.load(Ordering::SeqCst) {
            break;
        }
        if !more {
            // No visible windows: throttle instead of spinning.
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    // --- shutdown ------------------------------------------------------------
    tracing::info!("shutting down");
    state.exit_requested.store(true, Ordering::SeqCst);
    window.borrow_mut().win.hide();
    let _ = cmd_tx.blocking_send(AppCommand::Shutdown);
    rt.shutdown_timeout(Duration::from_secs(3));
    if let Some(handle) = tray_thread {
        let _ = handle.join();
    }
    std::process::exit(0);
}
