//! Shared application state: status snapshots published by the controller
//! and consumed by the GUI and tray threads.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::app::commands::GuiEvent;
use crate::core::server::ServerStats;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyState {
    Starting,
    Running,
    Paused,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct StatusSnapshot {
    pub state: ProxyState,
    pub listen_addr: String,
    pub active_connections: u64,
    /// Client -> destination.
    pub tx_bytes: u64,
    /// Destination -> client.
    pub rx_bytes: u64,
    pub domains: Vec<String>,
    pub autostart: bool,
    pub show_on_startup: bool,
    /// Effective advanced settings (for refreshing the form).
    pub max_connections: usize,
    pub connect_timeout_ms: u64,
    /// Unix milliseconds of the last failed destination connection
    /// (0 = none).
    pub last_failure_ms: u64,
    pub last_error: Option<String>,
}

impl StatusSnapshot {
    pub fn total_bytes(&self) -> u64 {
        self.tx_bytes + self.rx_bytes
    }
}

/// State shared between the controller (writer), the GUI and the tray
/// (readers). The GUI is woken through an injectable callback so the library
/// stays FLTK-free.
pub struct SharedAppState {
    snapshot: Mutex<StatusSnapshot>,
    stats_source: Mutex<Option<Arc<ServerStats>>>,
    generation: AtomicU64,
    gui_events: Mutex<Vec<GuiEvent>>,
    pub exit_requested: AtomicBool,
    wake_gui: Mutex<Box<dyn Fn() + Send + Sync>>,
}

impl SharedAppState {
    pub fn new(initial: StatusSnapshot) -> Arc<Self> {
        Arc::new(Self {
            snapshot: Mutex::new(initial),
            stats_source: Mutex::new(None),
            generation: AtomicU64::new(1),
            gui_events: Mutex::new(Vec::new()),
            exit_requested: AtomicBool::new(false),
            wake_gui: Mutex::new(Box::new(|| {})),
        })
    }

    /// Installs the GUI wake callback (e.g. FLTK `awake`). Must be called
    /// before the tray thread starts.
    pub fn set_wake_gui(&self, f: Box<dyn Fn() + Send + Sync>) {
        *self.wake_gui.lock().unwrap() = f;
    }

    /// Replaces the snapshot and notifies the GUI.
    pub fn publish(&self, snapshot: StatusSnapshot) {
        *self.snapshot.lock().unwrap() = snapshot;
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.push_event(GuiEvent::StatusChanged);
    }

    /// The stored snapshot with live counters merged in from the current
    /// stats source.
    pub fn live_snapshot(&self) -> StatusSnapshot {
        let mut s = self.snapshot.lock().unwrap().clone();
        if let Some(stats) = self.stats_source.lock().unwrap().as_ref() {
            s.active_connections = stats.active_connections.load(Ordering::Relaxed).max(0) as u64;
            s.tx_bytes = stats.tx_bytes.load(Ordering::Relaxed);
            s.rx_bytes = stats.rx_bytes.load(Ordering::Relaxed);
            s.last_failure_ms = stats.last_failure_ms.load(Ordering::Relaxed);
        }
        s
    }

    pub fn snapshot(&self) -> StatusSnapshot {
        self.snapshot.lock().unwrap().clone()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Registers (or clears) the live counter source of the running server.
    pub fn set_stats_source(&self, stats: Option<Arc<ServerStats>>) {
        *self.stats_source.lock().unwrap() = stats;
    }

    /// Reflects a change in the HKCU autostart registration.
    pub fn set_autostart(&self, enabled: bool) {
        self.snapshot.lock().unwrap().autostart = enabled;
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.push_event(GuiEvent::AutostartChanged(enabled));
    }

    pub fn push_event(&self, event: GuiEvent) {
        self.gui_events.lock().unwrap().push(event);
        (self.wake_gui.lock().unwrap())();
    }

    pub fn take_events(&self) -> Vec<GuiEvent> {
        std::mem::take(&mut *self.gui_events.lock().unwrap())
    }
}
