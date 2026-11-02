//! escape-proxy library: SOCKS5 proxy core, application controller and
//! Windows platform integration.
//!
//! The library is intentionally GUI-free so the proxy can run headlessly in
//! tests. The FLTK GUI lives in the binary target (`src/main.rs` + `src/gui`).

pub mod app;
pub mod core;
pub mod logging;
pub mod platform;
