//! Settings section: Windows integration checkboxes and advanced option
//! parsing.

use escape_proxy::app::config;

/// Parses and validates the advanced fields. Returns an error message suited
/// for display next to the inputs.
pub fn parse_advanced(
    port: &str,
    max_connections: &str,
    connect_timeout_ms: &str,
) -> Result<(u16, usize, u64), String> {
    let port: u16 = port
        .trim()
        .parse()
        .map_err(|_| "port must be a number between 1 and 65535".to_string())?;
    if port == 0 {
        return Err("port must be a number between 1 and 65535".to_string());
    }
    let max_connections: usize = max_connections
        .trim()
        .parse()
        .map_err(|_| format!("max connections must be between 1 and {}", config::MAX_CONNECTIONS_LIMIT))?;
    if max_connections == 0 || max_connections > config::MAX_CONNECTIONS_LIMIT {
        return Err(format!("max connections must be between 1 and {}", config::MAX_CONNECTIONS_LIMIT));
    }
    let timeout: u64 = connect_timeout_ms
        .trim()
        .parse()
        .map_err(|_| format!("timeout must be between {} and {} ms", config::MIN_TIMEOUT_MS, config::MAX_TIMEOUT_MS))?;
    if !(config::MIN_TIMEOUT_MS..=config::MAX_TIMEOUT_MS).contains(&timeout) {
        return Err(format!("timeout must be between {} and {} ms", config::MIN_TIMEOUT_MS, config::MAX_TIMEOUT_MS));
    }
    Ok((port, max_connections, timeout))
}
