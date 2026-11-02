//! Typed commands and GUI events exchanged between GUI, tray, controller and
//! the network runtime.

/// Commands accepted by the application controller. The GUI and tray send
/// these from their own threads; the controller owns all proxy state
/// transitions.
#[derive(Debug, Clone)]
pub enum AppCommand {
    /// Enable or disable (pause) forwarding.
    SetEnabled(bool),
    /// Replace the entire domain allowlist.
    UpdateDomains(Vec<String>),
    /// Apply advanced settings. Port or limit changes require a listener
    /// rebind.
    ApplyAdvanced {
        port: u16,
        max_connections: usize,
        connect_timeout_ms: u64,
    },
    /// Change whether the settings window is shown on Windows login.
    SetShowOnStartup(bool),
    /// Shut the network runtime down.
    Shutdown,
}

/// Events the main (FLTK) thread should react to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiEvent {
    /// Bring the settings window up (tray click or second launch).
    ShowSettings,
    /// Snapshot changed; refresh visible widgets.
    StatusChanged,
    /// Autostart registration changed; refresh the checkbox.
    AutostartChanged(bool),
    /// The user requested exit from the tray.
    ExitRequested,
}
