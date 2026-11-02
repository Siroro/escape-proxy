//! Clean dark theme. Implemented directly on FLTK's global palette rather
//! than pulling in fltk-theme, which gives exact control over the few colors
//! this window needs.

use fltk::app;
use fltk::enums::Color;

/// Window background.
pub const WINDOW_BG: Color = Color::from_rgb(22, 25, 31);
/// Section card background.
pub const CARD: Color = Color::from_rgb(36, 41, 53);
/// Inputs, browsers and button faces.
pub const INPUT_BG: Color = Color::from_rgb(46, 52, 66);
pub const INPUT_BG_HI: Color = Color::from_rgb(52, 58, 73);
/// Checkbox glyph box interior (kept lighter so the box reads on cards).
pub const CHECK_BOX: Color = Color::from_rgb(64, 71, 89);
pub const TEXT: Color = Color::from_rgb(232, 235, 242);
pub const TEXT_DIM: Color = Color::from_rgb(146, 153, 168);
pub const ACCENT: Color = Color::from_rgb(96, 165, 250);
pub const GREEN: Color = Color::from_rgb(74, 222, 128);
/// Face of the enabled toggle while on.
pub const GREEN_FACE: Color = Color::from_rgb(26, 97, 62);
pub const RED: Color = Color::from_rgb(248, 113, 113);
pub const GRAY: Color = Color::from_rgb(148, 152, 162);
pub const SEPARATOR: Color = Color::from_rgb(48, 54, 66);

/// Applies the global dark palette. Call once after `App::default()`.
pub fn apply() {
    app::background(22, 25, 31);
    app::background2(46, 52, 66);
    app::foreground(232, 235, 242);
    app::set_selection_color(96, 165, 250);
    app::set_scheme(app::Scheme::Gleam);
    // Dashed keyboard-focus rectangles read as visual noise in this window.
    app::set_visible_focus(false);
}
