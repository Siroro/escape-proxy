//! Platform integration. Windows-specific modules are gated so the proxy
//! core remains portable.

#[cfg(windows)]
pub mod windows;
