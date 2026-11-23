//! Dark theme: one cohesive palette applied through FLTK's global palette and
//! the custom-drawn controls in `widgets.rs`. FLTK's stock boxes and default
//! fonts (Arial on Windows) read as dated, so the app font is remapped to
//! Segoe UI and every control draws its own rounded chrome.

use std::sync::OnceLock;

use fltk::app;
use fltk::enums::{Color, Font};

// --- surfaces -----------------------------------------------------------

/// Window background behind the cards.
pub const BG: Color = Color::from_rgb(23, 25, 30);
/// Card face.
pub const CARD: Color = Color::from_rgb(32, 35, 42);
/// Card border.
pub const CARD_EDGE: Color = Color::from_rgb(45, 49, 59);
/// Inset fields: inputs, domain list.
pub const FIELD: Color = Color::from_rgb(42, 46, 56);
/// Field border.
pub const FIELD_EDGE: Color = Color::from_rgb(58, 63, 76);
/// Field face on hover.
pub const FIELD_HI: Color = Color::from_rgb(52, 57, 69);
/// Domain list backdrop: one step lighter than FIELD so the inset reads.
pub const LIST: Color = Color::from_rgb(46, 50, 60);
/// Off-state switch track, bright enough to read as interactive.
pub const SWITCH_OFF: Color = Color::from_rgb(62, 68, 82);

// --- text ---------------------------------------------------------------

pub const TEXT: Color = Color::from_rgb(233, 236, 244);
pub const TEXT_DIM: Color = Color::from_rgb(156, 162, 176);
pub const TEXT_FAINT: Color = Color::from_rgb(125, 131, 148);

// --- accent + status ----------------------------------------------------

/// Single interactive accent (switches, selections, primary buttons).
pub const ACCENT: Color = Color::from_rgb(83, 138, 240);
pub const ACCENT_HI: Color = Color::from_rgb(102, 155, 248);
pub const ACCENT_DARK: Color = Color::from_rgb(66, 112, 200);
/// Status colors; only the status badge uses these.
pub const GREEN: Color = Color::from_rgb(90, 195, 130);
pub const AMBER: Color = Color::from_rgb(230, 170, 92);
pub const RED: Color = Color::from_rgb(240, 110, 110);
pub const GRAY: Color = Color::from_rgb(148, 154, 168);

static SEMIBOLD: OnceLock<Font> = OnceLock::new();

/// Font for titles: Segoe UI Semibold when available, Helvetica bold
/// otherwise.
pub fn semibold() -> Font {
    *SEMIBOLD.get_or_init(|| Font::HelveticaBold)
}

/// Applies the global palette and remaps FLTK's base fonts to Segoe UI.
/// Call once right after `App::default()`, before any widget is created.
pub fn apply() {
    map_system_fonts();
    let (r, g, b) = BG.to_rgb();
    app::background(r, g, b);
    let (r, g, b) = FIELD.to_rgb();
    app::background2(r, g, b);
    let (r, g, b) = TEXT.to_rgb();
    app::foreground(r, g, b);
    let (r, g, b) = ACCENT.to_rgb();
    app::set_selection_color(r, g, b);
    app::set_font_size(13);
    app::set_scheme(app::Scheme::Gleam);
    // Dashed keyboard-focus rectangles read as visual noise in this window.
    app::set_visible_focus(false);
}

/// Remaps FLTK's font slots 0/1 (Helvetica/HelveticaBold — Arial by default
/// on Windows) to Segoe UI so every label, input and button picks it up.
fn map_system_fonts() {
    let names = app::get_font_names();
    if let Some(i) = names.iter().position(|n| n == "Segoe UI") {
        app::set_font(Font::by_index(i));
    }
    if let Some(i) = names.iter().position(|n| n == "Segoe UI Semibold") {
        let _ = SEMIBOLD.set(Font::by_index(i));
    }
}
