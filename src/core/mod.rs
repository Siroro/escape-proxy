//! Proxy core: SOCKS5 protocol, destination policy, TCP relay and listener.
//! Knows nothing about FLTK, the tray or Windows startup APIs.

pub mod matcher;
pub mod relay;
pub mod server;
pub mod socks;
