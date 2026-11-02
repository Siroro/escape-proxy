//! Overview section: status light and runtime info presentation.

use fltk::enums::Color;
use fltk::frame::Frame;
use fltk::prelude::*;

use escape_proxy::app::state::{ProxyState, StatusSnapshot};

use crate::gui::theme;

pub fn status_appearance(snapshot: &StatusSnapshot) -> (&'static str, Color) {
    match &snapshot.state {
        ProxyState::Starting => ("Starting", theme::GRAY),
        ProxyState::Running => ("Running", theme::GREEN),
        ProxyState::Paused => ("Paused", theme::GRAY),
        ProxyState::Error(_) => ("Error", theme::RED),
    }
}

/// Refreshes the status dot, status text and the transient error line.
/// Values are only touched when they actually changed — unconditional
/// `set_label` calls would mark the window damaged on every tick and keep
/// the FLTK loop from sleeping.
pub fn refresh_status(
    dot: &mut Frame,
    label: &mut Frame,
    error_label: &mut Frame,
    snapshot: &StatusSnapshot,
) {
    let (text, color) = status_appearance(snapshot);
    if dot.label_color() != color {
        dot.set_label_color(color);
        dot.redraw();
    }
    if label.label() != text {
        label.set_label(text);
        label.redraw();
    }
    let error_text: &str = match (&snapshot.state, snapshot.last_error.as_deref()) {
        (ProxyState::Error(e), _) => e.as_str(),
        (_, Some(notice)) => notice,
        _ => "",
    };
    if error_label.label() != error_text {
        error_label.set_label(error_text);
        error_label.redraw();
    }
}
