//! Custom-drawn controls. FLTK's stock boxes cannot produce the rounded,
//! flat design this window needs, so cards, switches, buttons and the status
//! badge render themselves through draw callbacks.

use std::cell::Cell;
use std::rc::Rc;

use fltk::button::{Button, ToggleButton};
use fltk::draw;
use fltk::enums::{Align, Color, Damage, Event, FrameType};
use fltk::frame::Frame;
use fltk::group::Flex;
use fltk::prelude::*;
use fltk::widget::Widget;

use super::theme;

// ------------------------------------------------------------- primitives

/// Radius of card corners.
pub const CARD_RADIUS: i32 = 10;
/// Radius of button / field corners.
pub const CONTROL_RADIUS: i32 = 6;
/// Buttons and fields never paint taller than this; taller cells just add
/// air (Flex stretches children vertically).
const CONTROL_MAX_H: i32 = 26;

/// Filled rounded rectangle, assembled from two rectangles plus quarter
/// pies because FLTK has no rounded-rect primitive.
fn round_fill(x: i32, y: i32, w: i32, h: i32, r: i32, color: Color) {
    let r = r.clamp(0, (w / 2).min(h / 2));
    if r == 0 {
        draw::draw_rect_fill(x, y, w, h, color);
        return;
    }
    draw::draw_rect_fill(x + r, y, w - 2 * r, h, color);
    draw::draw_rect_fill(x, y + r, w, h - 2 * r, color);
    draw::set_draw_color(color);
    draw::draw_pie(x, y, 2 * r, 2 * r, 90.0, 180.0); // top-left
    draw::draw_pie(x + w - 2 * r, y, 2 * r, 2 * r, 0.0, 90.0); // top-right
    draw::draw_pie(x, y + h - 2 * r, 2 * r, 2 * r, 180.0, 270.0); // bottom-left
    draw::draw_pie(x + w - 2 * r, y + h - 2 * r, 2 * r, 2 * r, 270.0, 360.0); // bottom-right
}

/// 1px rounded border.
fn round_edge(x: i32, y: i32, w: i32, h: i32, r: i32, color: Color) {
    let r = r.clamp(0, (w / 2).min(h / 2));
    draw::set_draw_color(color);
    draw::set_line_style(draw::LineStyle::Solid, 1);
    if r > 0 {
        draw::draw_arc(x, y, 2 * r, 2 * r, 90.0, 180.0);
        draw::draw_arc(x + w - 2 * r, y, 2 * r, 2 * r, 0.0, 90.0);
        draw::draw_arc(x, y + h - 2 * r, 2 * r, 2 * r, 180.0, 270.0);
        draw::draw_arc(x + w - 2 * r, y + h - 2 * r, 2 * r, 2 * r, 270.0, 360.0);
        draw::draw_line(x + r, y, x + w - r, y);
        draw::draw_line(x + r, y + h - 1, x + w - r, y + h - 1);
        draw::draw_line(x, y + r, x, y + h - r);
        draw::draw_line(x + w - 1, y + r, x + w - 1, y + h - r);
    } else {
        draw::draw_rect(x, y, w - 1, h - 1);
    }
    draw::set_line_style(draw::LineStyle::Solid, 0);
}

fn fill_and_edge(x: i32, y: i32, w: i32, h: i32, r: i32, fill: Color, edge: Color) {
    round_fill(x, y, w, h, r, fill);
    round_edge(x, y, w, h, r, edge);
}

// ------------------------------------------------------ damage discipline

/// True when the widget needs a full repaint of its own chrome. On
/// `Damage::Child`-only redraws the backdrop in the back buffer is still
/// valid, and repainting it would erase undamaged siblings inside the damage
/// region (the same rule stock `Fl_Group::draw` follows for its box).
fn needs_full_repaint<W: WidgetExt>(w: &W) -> bool {
    let d = w.damage_type();
    (d & !Damage::Child) != Damage::None
}

// ------------------------------------------------------------------- card

/// Turns a Flex into a rounded card: dark face, hairline border, children
/// drawn on top.
///
/// The stock Flex draw is disabled (`super_draw(false)`); otherwise it would
/// draw the children first and the backdrop would then paint over them
/// without a second pass to recover.
pub fn card(f: &mut Flex) {
    f.set_frame(FrameType::NoBox);
    f.set_color(theme::CARD);
    f.draw(|g| {
        if needs_full_repaint(g) {
            fill_and_edge(g.x(), g.y(), g.w(), g.h(), CARD_RADIUS, theme::CARD, theme::CARD_EDGE);
        }
        g.draw_children();
    });
    f.super_draw(false);
}

// ----------------------------------------------------------------- switch

/// Switch footprint (the pill is centered inside whatever cell the row
/// gives the widget).
const SWITCH_W: i32 = 42;
const SWITCH_H: i32 = 24;

/// Styles a ToggleButton as a pill toggle switch. The button carries no
/// label; pair it with a label widget in the same row.
pub fn style_switch(b: &mut ToggleButton) {
    b.set_frame(FrameType::NoBox);
    b.draw(|s| {
        let w = SWITCH_W.min(s.w());
        let h = SWITCH_H.min(s.h());
        let x = s.x() + (s.w() - w) / 2;
        let y = s.y() + (s.h() - h) / 2;
        let on = s.value();
        let track = if on { theme::ACCENT } else { theme::SWITCH_OFF };
        let edge = if on { theme::ACCENT } else { theme::SWITCH_OFF };
        fill_and_edge(x, y, w, h, h / 2, track, edge);
        let d = h - 6;
        let kx = if on { x + w - d - 3 } else { x + 3 };
        draw::set_draw_color(Color::White);
        draw::draw_pie(kx, y + 3, d, d, 0.0, 360.0);
    });
    b.super_draw(false);
}

// ----------------------------------------------------------------- button

#[derive(Clone, Copy)]
pub enum ButtonStyle {
    /// Accent fill, white label: the row's main action.
    Primary,
    /// Field fill, hairline border: secondary actions.
    Secondary,
}

/// Styles a Button as a rounded flat control with hover and pressed states.
/// The label is drawn by the callback, so it survives the custom chrome.
pub fn style_button(b: &mut Button, style: ButtonStyle) {
    b.set_frame(FrameType::NoBox);
    let hover: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    {
        let hover2 = hover.clone();
        b.handle(move |w, ev| match ev {
            Event::Enter => {
                if !hover2.replace(true) {
                    w.redraw();
                }
                false
            }
            Event::Leave => {
                if hover2.replace(false) {
                    w.redraw();
                }
                false
            }
            _ => false,
        });
    }
    let hover3 = hover.clone();
    b.draw(move |btn| {
        let h = btn.h().min(CONTROL_MAX_H);
        let x = btn.x();
        let y = btn.y() + (btn.h() - h) / 2;
        let w = btn.w();
        let hovered = hover3.get();
        let pressed = btn.value();
        let (face, edge, text) = match style {
            ButtonStyle::Primary => {
                let face = if pressed {
                    theme::ACCENT_DARK
                } else if hovered {
                    theme::ACCENT_HI
                } else {
                    theme::ACCENT
                };
                (face, face, Color::White)
            }
            ButtonStyle::Secondary => {
                let face = if pressed || hovered {
                    theme::FIELD_HI
                } else {
                    theme::FIELD
                };
                (face, theme::FIELD_EDGE, theme::TEXT)
            }
        };
        fill_and_edge(x, y, w, h, CONTROL_RADIUS, face, edge);
        let ty = if pressed { y + 1 } else { y };
        draw::set_draw_color(text);
        draw::set_font(btn.label_font(), btn.label_size());
        draw::draw_text2(&btn.label(), x, ty, w, h, Align::Center);
    });
    b.super_draw(false);
}

// ------------------------------------------------------------------ field

/// Turns a Flex into a rounded input-field backdrop. The actual input
/// widget lives inside as a frameless child, inset horizontally through the
/// flex margins — FLTK inputs have no text padding of their own. Like
/// buttons, the backdrop never paints taller than `CONTROL_MAX_H`. The
/// backdrop follows the group damage discipline of `card`.
pub fn style_field(f: &mut Flex) {
    f.set_frame(FrameType::NoBox);
    f.draw(|g| {
        if needs_full_repaint(g) {
            let h = g.h().min(CONTROL_MAX_H);
            let y = g.y() + (g.h() - h) / 2;
            fill_and_edge(g.x(), y, g.w(), h, CONTROL_RADIUS, theme::FIELD, theme::FIELD_EDGE);
        }
        g.draw_children();
    });
    f.super_draw(false);
}

// ------------------------------------------------------------------ badge

/// Styles a bare Widget as a status badge: colored dot plus text in the
/// theme's dim color. The dot color is carried by `label_color`; the label
/// text itself is the status word. A plain Widget is used because it has no
/// drawing of its own — a Frame would render its label under the custom
/// draw, doubling the text.
pub fn style_badge(f: &mut Widget) {
    f.set_label_font(theme::semibold());
    f.set_label_size(12);
    f.draw(|fr| {
        let text = fr.label();
        let (tw, _) = draw::measure(&text, false);
        let (x, y, w, h) = (fr.x(), fr.y(), fr.w(), fr.h());
        // Right-aligned: text, then the dot to its left.
        let text_x = x + w - tw;
        draw::set_draw_color(theme::TEXT_DIM);
        draw::set_font(fr.label_font(), fr.label_size());
        draw::draw_text2(&text, text_x, y, tw, h, Align::Right | Align::Inside);
        draw::set_draw_color(fr.label_color());
        draw::draw_pie(text_x - 14, y + h / 2 - 3, 8, 8, 0.0, 360.0);
    });
}

// ------------------------------------------------------------------ misc

/// Small uppercase section title inside a card.
pub fn section_title(parent: &mut Flex, title: &str) {
    let mut t = Frame::default().with_label(title);
    t.set_label_size(10);
    t.set_label_color(theme::TEXT_DIM);
    t.set_align(Align::Left | Align::Inside);
    parent.fixed(&t, 14);
}
