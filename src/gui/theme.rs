//! Clean dark theme. Implemented directly on FLTK's global palette rather
//! than pulling in fltk-theme, which gives exact control over the few colors
//! this window needs.

use fltk::app;
use fltk::enums::Color;

pub const INPUT_BG: Color = Color::from_rgb(46, 52, 64);
pub const TEXT: Color = Color::from_rgb(228, 231, 238);
pub const TEXT_DIM: Color = Color::from_rgb(150, 156, 168);
pub const ACCENT: Color = Color::from_rgb(86, 156, 214);
pub const GREEN: Color = Color::from_rgb(96, 200, 130);
pub const RED: Color = Color::from_rgb(230, 100, 100);
pub const GRAY: Color = Color::from_rgb(140, 140, 150);
pub const SEPARATOR: Color = Color::from_rgb(58, 64, 76);

/// Applies the global dark palette. Call once after `App::default()`.
pub fn apply() {
    app::background(32, 36, 44);
    app::background2(46, 52, 64);
    app::foreground(228, 231, 238);
    app::set_selection_color(66, 133, 244);
    app::set_scheme(app::Scheme::Gleam);
}
