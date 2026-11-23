//! Overview section: status badge and error line presentation.

use fltk::enums::Color;
use fltk::frame::Frame;
use fltk::prelude::*;
use fltk::widget::Widget;

use escape_proxy::app::state::{ProxyState, StatusSnapshot};

use crate::gui::theme;

pub fn status_appearance(snapshot: &StatusSnapshot) -> (&'static str, Color) {
    match &snapshot.state {
        ProxyState::Starting => ("Starting", theme::AMBER),
        ProxyState::Running => ("Running", theme::GREEN),
        ProxyState::Paused => ("Paused", theme::GRAY),
        ProxyState::Error(_) => ("Error", theme::RED),
    }
}

/// Refreshes the status badge and the transient error line. Values are only
/// touched when they actually changed — unconditional `set_label` calls
/// would mark the window damaged on every tick and keep the FLTK loop from
/// sleeping. The badge draws its own status dot in `label_color` (see
/// `widgets::style_badge`), so the label carries the word only.
pub fn refresh_status(badge: &mut Widget, error_label: &mut Frame, snapshot: &StatusSnapshot) {
    let (text, color) = status_appearance(snapshot);
    if badge.label() != text {
        badge.set_label(text);
        badge.redraw();
    }
    if badge.label_color() != color {
        badge.set_label_color(color);
        badge.redraw();
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
