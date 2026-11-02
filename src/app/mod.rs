//! Application controller layer: configuration, commands, shared state and
//! traffic statistics. This is the single authority for proxy state; the GUI
//! and tray talk to it only through typed commands.

pub mod commands;
pub mod config;
pub mod controller;
pub mod state;
pub mod statistics;
