//! Application controller: the single authority for proxy state.
//!
//! Owns configuration validation/persistence, listener lifecycle and
//! coordinated shutdown. Knows nothing about FLTK or the tray; those send
//! [`AppCommand`]s and observe [`SharedAppState`].

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use crate::app::commands::AppCommand;
use crate::app::config::{self, Config, ValidatedConfig};
use crate::app::state::{ProxyState, SharedAppState, StatusSnapshot};
use crate::core::matcher::DomainList;
use crate::core::server::{ProxyServer, ServerConfig, SharedDomains};

pub struct Controller {
    path: PathBuf,
    /// Last successfully saved on-disk config.
    saved: Config,
    /// Currently applied runtime settings.
    running: ValidatedConfig,
    domains: SharedDomains,
    enabled_tx: watch::Sender<bool>,
    server: Option<Arc<ProxyServer>>,
    /// Listener failure text while `server` is `None`.
    listener_error: Option<String>,
    /// Transient notice (config load/apply problems) surfaced in the GUI.
    notice: Option<String>,
    stats: Arc<SharedAppState>,
    cmd_rx: mpsc::Receiver<AppCommand>,
}

/// Loads the config, starts the listener and spawns the controller task.
/// Returns the command sender used by the GUI and tray.
pub async fn start(path: PathBuf, stats: Arc<SharedAppState>) -> mpsc::Sender<AppCommand> {
    let (tx, rx) = mpsc::channel(32);

    let (mut saved_config, load_error) = match config::load(&path) {
        Ok(c) => (c, None),
        Err(e) => {
            let msg = format!("config load failed: {e}; using defaults");
            warn!("{msg}");
            (Config::default(), Some(msg))
        }
    };
    // Persist defaults immediately when no config exists yet so the file is
    // discoverable next to the executable's data directory.
    if load_error.is_none() && !path.exists() {
        if let Err(e) = config::save(&path, &saved_config) {
            warn!("cannot write initial config: {e}");
        }
    }

    let validated = match saved_config.validate() {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("config invalid: {e}; using defaults");
            warn!("{msg}");
            saved_config = Config::default();
            let _ = saved_config.validate().expect("defaults must validate");
            saved_config.validate().unwrap()
        }
    };

    let domains: SharedDomains = Arc::new(tokio::sync::RwLock::new(Arc::new(
        DomainList::from_normalized(validated.domains.clone()),
    )));
    let (enabled_tx, enabled_rx) = watch::channel(validated.enabled);

    let mut controller = Controller {
        path,
        saved: saved_config,
        running: validated,
        domains,
        enabled_tx,
        server: None,
        listener_error: None,
        notice: load_error,
        stats,
        cmd_rx: rx,
    };
    controller.start_server(Some(enabled_rx)).await;
    controller.publish();

    tokio::spawn(async move { controller.run().await });
    tx
}

impl Controller {
    async fn run(mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                AppCommand::SetEnabled(on) => self.set_enabled(on).await,
                AppCommand::UpdateDomains(domains) => self.update_domains(domains).await,
                AppCommand::ApplyAdvanced {
                    port,
                    max_connections,
                    connect_timeout_ms,
                } => {
                    self.apply_advanced(port, max_connections, connect_timeout_ms)
                        .await
                }
                AppCommand::SetShowOnStartup(show) => self.set_show_on_startup(show).await,
                AppCommand::Shutdown => break,
            }
        }
        self.stop_server().await;
        info!("controller stopped");
    }

    async fn set_enabled(&mut self, on: bool) {
        self.running.enabled = on;
        self.saved.enabled = on;
        let _ = self.enabled_tx.send(on);
        if on && self.server.is_none() {
            // Retry after a previous listener failure.
            self.start_server(None).await;
        }
        self.persist();
        info!(on, "forwarding {}", if on { "enabled" } else { "paused" });
        self.publish();
    }

    async fn update_domains(&mut self, domains: Vec<String>) {
        let normalized = match DomainList::new(&domains) {
            Ok(list) => list,
            Err(e) => {
                self.notice = Some(format!("domain list rejected: {e}"));
                warn!("{}", self.notice.as_deref().unwrap_or_default());
                self.publish();
                return;
            }
        };
        let entries = normalized.sorted();
        *self.domains.write().await = Arc::new(normalized);
        self.running.domains = entries.clone();
        self.saved.domains = entries;
        self.notice = None;
        self.persist();
        info!("domain allowlist updated ({} entries)", self.running.domains.len());
        self.publish();
    }

    async fn apply_advanced(&mut self, port: u16, max_connections: usize, connect_timeout_ms: u64) {
        let mut candidate = self.saved.clone();
        candidate.listen_addr = format!("127.0.0.1:{port}");
        candidate.max_connections = max_connections;
        candidate.connect_timeout_ms = connect_timeout_ms;
        let validated = match candidate.validate() {
            Ok(v) => v,
            Err(e) => {
                self.notice = Some(format!("settings rejected: {e}"));
                warn!("{}", self.notice.as_deref().unwrap_or_default());
                self.publish();
                return;
            }
        };
        drop(candidate);

        // Stop the old listener first so a rebind to the same port cannot
        // collide with itself.
        self.stop_server().await;
        self.running.listen = validated.listen;
        self.running.max_connections = validated.max_connections;
        self.running.connect_timeout = validated.connect_timeout;
        self.start_server(None).await;

        if self.server.is_some() {
            self.saved.listen_addr = validated.listen.to_string();
            self.saved.max_connections = validated.max_connections;
            self.saved.connect_timeout_ms = validated.connect_timeout.as_millis() as u64;
            self.notice = None;
            self.persist();
        }
        self.publish();
    }

    async fn set_show_on_startup(&mut self, show: bool) {
        self.running.ui.show_on_startup = show;
        self.saved.ui.show_on_startup = show;
        self.persist();
        self.publish();
    }

    async fn start_server(&mut self, enabled_rx: Option<watch::Receiver<bool>>) {
        let server_config = ServerConfig {
            listen: self.running.listen,
            max_connections: self.running.max_connections,
            connect_timeout: self.running.connect_timeout,
            handshake_timeout: self.running.handshake_timeout,
            policy: Default::default(),
        };
        let enabled_rx = enabled_rx.unwrap_or_else(|| self.enabled_tx.subscribe());
        match ProxyServer::start(server_config, self.domains.clone(), enabled_rx).await {
            Ok(server) => {
                self.stats.set_stats_source(Some(server.stats.clone()));
                info!(addr = %server.local_addr, "listener started");
                self.server = Some(server);
                self.listener_error = None;
            }
            Err(e) => {
                error!("listener failed to start: {e}");
                self.stats.set_stats_source(None);
                self.server = None;
                self.listener_error = Some(format!("listener failed: {e}"));
            }
        }
    }

    async fn stop_server(&mut self) {
        if let Some(server) = self.server.take() {
            server.shutdown().await;
            self.stats.set_stats_source(None);
        }
    }

    fn persist(&self) {
        if let Err(e) = config::save(&self.path, &self.saved) {
            error!("config save failed: {e}");
        }
    }

    fn current_state(&self) -> ProxyState {
        if let Some(err) = &self.listener_error {
            return ProxyState::Error(err.clone());
        }
        if self.server.is_none() {
            return ProxyState::Error("listener not running".to_string());
        }
        if !self.running.enabled {
            ProxyState::Paused
        } else {
            ProxyState::Running
        }
    }

    fn publish(&mut self) {
        let previous = self.stats.snapshot();
        let snapshot = StatusSnapshot {
            state: self.current_state(),
            listen_addr: self.running.listen.to_string(),
            // Live values are merged in by `live_snapshot()` at read time.
            active_connections: 0,
            tx_bytes: 0,
            rx_bytes: 0,
            domains: self.running.domains.clone(),
            autostart: previous.autostart,
            show_on_startup: self.running.ui.show_on_startup,
            max_connections: self.running.max_connections,
            connect_timeout_ms: self.running.connect_timeout.as_millis() as u64,
            last_failure_ms: 0,
            last_error: self.notice.clone(),
        };
        self.stats.publish(snapshot);
    }
}
