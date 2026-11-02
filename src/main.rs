//! escape-proxy: lightweight Windows SOCKS5 proxy, tray application and
//! settings GUI.
//!
//! `main.rs` wires the components together: single-instance guard, logging,
//! Tokio runtime + controller, FLTK window and the tray thread. It contains
//! no networking protocol or GUI business logic.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gui;

use std::sync::atomic::Ordering;
use std::time::Duration;

use fltk::prelude::*;

use escape_proxy::app::commands::{AppCommand, GuiEvent};
use escape_proxy::app::config;
use escape_proxy::app::controller;
use escape_proxy::app::state::{ProxyState, SharedAppState, StatusSnapshot};
use escape_proxy::logging;
use escape_proxy::platform::windows::{autostart, instance, tray};

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
    let mut main_window = gui::window::build(
        state.clone(),
        cmd_tx.clone(),
        root.clone(),
        show_at_start,
    );
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
    // Custom loop instead of `app::run()`: `run()` returns once all windows
    // are hidden, but the tray must keep the app alive with the settings
    // window closed. `wait_for` sleeps on the OS message queue (no busy
    // spin), and `awake()` wakes it immediately for cross-thread events.
    loop {
        if state.exit_requested.load(Ordering::SeqCst) {
            break;
        }
        let _ = fltk::app::wait_for(0.5);

        let mut config_changed = false;
        let mut exit = false;
        for event in state.take_events() {
            match event {
                GuiEvent::ShowSettings => main_window.show_window(),
                GuiEvent::StatusChanged => {
                    config_changed = true;
                    if main_window.win.visible() {
                        main_window.tick(&state, true);
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
        // Periodic refresh only while the window is visible.
        if main_window.win.visible() {
            main_window.tick(&state, config_changed);
        }
    }

    // --- shutdown ------------------------------------------------------------
    tracing::info!("shutting down");
    state.exit_requested.store(true, Ordering::SeqCst);
    main_window.win.hide();
    let _ = cmd_tx.blocking_send(AppCommand::Shutdown);
    rt.shutdown_timeout(Duration::from_secs(3));
    if let Some(handle) = tray_thread {
        let _ = handle.join();
    }
    std::process::exit(0);
}
