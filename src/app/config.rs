//! Configuration loading, validation and atomic persistence.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::matcher::{self, DomainError};

pub const DEFAULT_PORT: u16 = 10808;
pub const MIN_TIMEOUT_MS: u64 = 250;
pub const MAX_TIMEOUT_MS: u64 = 300_000;
pub const MAX_CONNECTIONS_LIMIT: usize = 1024;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("configuration parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("serialization error: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("invalid listen address '{0}': the bind IP is fixed at 127.0.0.1")]
    BindIpNotAllowed(String),
    #[error("max_connections must be between 1 and {MAX_CONNECTIONS_LIMIT}, got {0}")]
    InvalidMaxConnections(usize),
    #[error("timeout must be between {MIN_TIMEOUT_MS} and {MAX_TIMEOUT_MS} ms, got {0}")]
    InvalidTimeout(u64),
    #[error("invalid domain: {0}")]
    Domain(#[from] DomainError),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiConfig {
    #[serde(default = "default_false")]
    pub show_on_startup: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_false() -> bool {
    false
}

fn default_theme() -> String {
    "dark".to_string()
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_on_startup: false,
            theme: "dark".to_string(),
        }
    }
}

/// On-disk configuration. All fields have defaults so an existing config
/// keeps loading when new options appear.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default = "default_timeout")]
    pub connect_timeout_ms: u64,
    #[serde(default = "default_timeout")]
    pub handshake_timeout_ms: u64,
    /// When `true`, only the listed domains are allowed. Default `false`:
    /// every valid hostname is forwarded (like the old `allow *` 3proxy).
    #[serde(default)]
    pub restrict_domains: bool,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub ui: UiConfig,
}

fn default_listen_addr() -> String {
    format!("127.0.0.1:{DEFAULT_PORT}")
}

fn default_true() -> bool {
    true
}

fn default_max_connections() -> usize {
    128
}

fn default_timeout() -> u64 {
    5000
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_addr: default_listen_addr(),
            enabled: true,
            max_connections: default_max_connections(),
            connect_timeout_ms: default_timeout(),
            handshake_timeout_ms: default_timeout(),
            restrict_domains: false,
            domains: Vec::new(),
            ui: UiConfig::default(),
        }
    }
}

/// Immutable validated runtime settings.
#[derive(Debug, Clone)]
pub struct ValidatedConfig {
    pub listen: SocketAddr,
    pub enabled: bool,
    pub max_connections: usize,
    pub connect_timeout: Duration,
    pub handshake_timeout: Duration,
    /// Normalised, deduplicated, sorted.
    pub domains: Vec<String>,
    pub restrict_domains: bool,
    pub ui: UiConfig,
}

impl Config {
    /// Validates the configuration. Port 0 is accepted and means "ephemeral"
    /// (used by tests); the GUI always writes a concrete port.
    pub fn validate(&self) -> Result<ValidatedConfig, ConfigError> {
        let listen: SocketAddr = self
            .listen_addr
            .parse()
            .map_err(|_| ConfigError::BindIpNotAllowed(self.listen_addr.clone()))?;
        if !listen.ip().is_loopback() {
            return Err(ConfigError::BindIpNotAllowed(self.listen_addr.clone()));
        }
        if self.max_connections == 0 || self.max_connections > MAX_CONNECTIONS_LIMIT {
            return Err(ConfigError::InvalidMaxConnections(self.max_connections));
        }
        if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&self.connect_timeout_ms) {
            return Err(ConfigError::InvalidTimeout(self.connect_timeout_ms));
        }
        if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&self.handshake_timeout_ms) {
            return Err(ConfigError::InvalidTimeout(self.handshake_timeout_ms));
        }
        let mut set = std::collections::BTreeSet::new();
        for d in &self.domains {
            set.insert(matcher::normalize_domain(d)?);
        }
        Ok(ValidatedConfig {
            listen,
            enabled: self.enabled,
            max_connections: self.max_connections,
            connect_timeout: Duration::from_millis(self.connect_timeout_ms),
            handshake_timeout: Duration::from_millis(self.handshake_timeout_ms),
            domains: set.into_iter().collect(),
            restrict_domains: self.restrict_domains,
            ui: self.ui.clone(),
        })
    }

    pub fn from_validated(v: &ValidatedConfig) -> Self {
        Self {
            listen_addr: v.listen.to_string(),
            enabled: v.enabled,
            max_connections: v.max_connections,
            connect_timeout_ms: v.connect_timeout.as_millis() as u64,
            handshake_timeout_ms: v.handshake_timeout.as_millis() as u64,
            restrict_domains: v.restrict_domains,
            domains: v.domains.clone(),
            ui: v.ui.clone(),
        }
    }
}

/// Root directory for config and logs. `ESCAPE_PROXY_HOME` overrides the
/// default `%LOCALAPPDATA%\escape-proxy` (used by tests and smoke runs).
pub fn app_root() -> PathBuf {
    if let Ok(p) = std::env::var("ESCAPE_PROXY_HOME") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("escape-proxy")
}

pub fn config_path(root: &Path) -> PathBuf {
    root.join("config.toml")
}

/// Loads the config. A missing file yields defaults; anything else is an
/// error so corrupt files surface instead of silently resetting settings.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.into()),
    }
}

/// Atomically writes the config: serialize, write a temp file next to the
/// target, then rename over the original, so a crash can never truncate a
/// working config.
pub fn save(path: &Path, cfg: &Config) -> Result<(), ConfigError> {
    let text = toml::to_string_pretty(cfg)?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e.into())
        }
    }
}
