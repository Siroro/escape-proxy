//! Settings window composition: a single dark window laid out as rounded
//! cards — proxy state, domain allowlist, Windows integration and advanced
//! options. All custom chrome (cards, switches, buttons, badge) is drawn by
//! `widgets.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use fltk::browser::HoldBrowser;
use fltk::button::{Button, ToggleButton};
use fltk::enums::{Align, CallbackTrigger, FrameType};
use fltk::frame::Frame;
use fltk::group::Flex;
use fltk::input::{Input, IntInput};
use fltk::prelude::*;
use fltk::widget::Widget;
use fltk::window::Window;
use tokio::sync::mpsc;

use escape_proxy::app::commands::AppCommand;
use escape_proxy::app::state::{ProxyState, SharedAppState, StatusSnapshot};
use escape_proxy::app::statistics::format_bytes;
use escape_proxy::platform::windows;

use crate::gui::{domains, overview, settings, theme, widgets};
use crate::gui::widgets::ButtonStyle;

// ------------------------------------------------------------- layout math

pub const W: i32 = 480;
/// Window padding (outer) and gap between cards.
const PAD: i32 = 16;
const GAP: i32 = 12;
/// Padding and row spacing inside a card.
const CARD_PAD: i32 = 14;
const ROW_GAP: i32 = 8;

const HEADER_H: i32 = 26;
const FOOTER_H: i32 = 26;

const SWITCH_ROW_H: i32 = 26;
const INFO_ROW_H: i32 = 21;
const ERROR_ROW_H: i32 = 14;
const PROXY_CARD_H: i32 = CARD_PAD * 2 + SWITCH_ROW_H + INFO_ROW_H * 2 + ERROR_ROW_H + ROW_GAP * 4;

const TITLE_H: i32 = 14;
const CHECK_ROW_H: i32 = 24;
const HINT_H: i32 = 14;
const ADD_ROW_H: i32 = 28;
const REMOVE_ROW_H: i32 = 22;
const DOMAINS_FIXED_H: i32 =
    CARD_PAD * 2 + TITLE_H + CHECK_ROW_H + HINT_H + ADD_ROW_H + REMOVE_ROW_H + ROW_GAP * 5;

const SETTING_ROW_H: i32 = 38;
const WINDOWS_CARD_H: i32 = CARD_PAD * 2 + SETTING_ROW_H * 2 + ROW_GAP;

const ADV_HEADER_H: i32 = 24;
const ADV_ROW_H: i32 = 28;
const APPLY_ROW_H: i32 = 28;
const ADV_BODY_H: i32 = ADV_ROW_H * 3 + APPLY_ROW_H + ROW_GAP * 3;
const ADV_COLLAPSED_H: i32 = CARD_PAD * 2 + ADV_HEADER_H;
const ADV_EXPANDED_H: i32 = ADV_COLLAPSED_H + ROW_GAP + ADV_BODY_H;

/// Minimum height reserved for the domain list; it flexes with the window.
const LIST_MIN_H: i32 = 44;
/// A few px of slack so rounding in the native window frame can never eat
/// the bottom margin (the flexible list absorbs it instead).
const LAYOUT_SLACK: i32 = 6;
pub const DEFAULT_H: i32 = PAD * 2 + HEADER_H + GAP * 5
    + PROXY_CARD_H + DOMAINS_FIXED_H + WINDOWS_CARD_H + ADV_COLLAPSED_H + FOOTER_H + LIST_MIN_H
    + LAYOUT_SLACK;
pub const MIN_H: i32 = DEFAULT_H - LIST_MIN_H + 24;
const MAX_H: i32 = 900;

const HINT_RESTRICTED: &str = "Only the listed domains and their subdomains are proxied.";
const HINT_UNRESTRICTED: &str = "All domains are proxied; the list is used only when restricted.";

// ---------------------------------------------------------- advanced form

/// The port part of a listen address (`127.0.0.1:10808` gives `10808`).
fn port_text(snap: &StatusSnapshot) -> String {
    snap.listen_addr.rsplit(':').next().unwrap_or("").to_string()
}

/// Identifies the advanced settings last loaded into the form.
fn advanced_signature(snap: &StatusSnapshot) -> String {
    format!(
        "{}\u{0}{}\u{0}{}",
        snap.listen_addr, snap.max_connections, snap.connect_timeout_ms
    )
}

pub struct MainWindow {
    pub win: Window,
    status_badge: Widget,
    error_label: Frame,
    enabled_toggle: ToggleButton,
    addr_value: Frame,
    activity_value: Frame,
    restrict_switch: ToggleButton,
    domains_hint: Frame,
    domain_browser: HoldBrowser,
    autostart_switch: ToggleButton,
    startup_switch: ToggleButton,
    port_input: IntInput,
    maxconn_input: IntInput,
    timeout_input: IntInput,
    domains_sig: String,
    advanced_sig: String,
}

// -------------------------------------------------------------- row helpers

/// A fixed-height row inside a card column.
fn row(parent: &mut Flex, h: i32) -> Flex {
    let r = Flex::default().row();
    parent.fixed(&r, h);
    r
}

fn card_column(parent: &mut Flex, fixed_h: Option<i32>) -> Flex {
    let mut c = Flex::default().column();
    // Outer margin keeps content off the drawn card border; spacing
    // separates the rows.
    c.set_margin(CARD_PAD);
    c.set_spacing(ROW_GAP);
    if let Some(h) = fixed_h {
        parent.fixed(&c, h);
    }
    widgets::card(&mut c);
    c
}

fn label(text: &str, size: i32, color: fltk::enums::Color) -> Frame {
    let mut l = Frame::default().with_label(text);
    l.set_label_size(size);
    l.set_label_color(color);
    l.set_align(Align::Left | Align::Inside);
    l
}

/// Label + value row (value right-aligned in a fixed column).
fn info_row(parent: &mut Flex, label_text: &str) -> Frame {
    let mut r = row(parent, INFO_ROW_H);
    let l = label(label_text, 13, theme::TEXT_DIM);
    r.fixed(&l, 150);
    let _gap = Frame::default();
    let mut v = Frame::default();
    v.set_label_size(13);
    v.set_label_color(theme::TEXT);
    v.set_align(Align::Right | Align::Inside);
    r.fixed(&v, 180);
    r.end();
    v
}

/// Label + numeric input row (input right-aligned in a fixed column).
fn input_row(parent: &mut Flex, label_text: &str, default: &str) -> IntInput {
    let mut r = row(parent, ADV_ROW_H);
    let l = label(label_text, 13, theme::TEXT_DIM);
    r.fixed(&l, 150);
    let _gap = Frame::default();
    let mut field = Flex::default().row();
    field.set_margins(8, 0, 8, 0); // text inset; FLTK inputs have no padding
    widgets::style_field(&mut field);
    let mut input = IntInput::default();
    style_input(&mut input);
    input.set_value(default);
    field.end();
    r.fixed(&field, 120);
    r.end();
    input
}

fn style_input<I: WidgetExt + InputExt>(input: &mut I) {
    // Frameless: the surrounding field flex paints the backdrop.
    input.set_frame(FrameType::NoBox);
    input.set_text_color(theme::TEXT);
    input.set_text_size(13);
    input.set_selection_color(theme::ACCENT);
}

fn style_button(b: &mut Button, style: ButtonStyle) {
    b.set_label_size(13);
    widgets::style_button(b, style);
}

/// A two-line setting row: title + description on the left, switch on the
/// right. Returns the switch.
fn setting_row(parent: &mut Flex, title: &str, desc: &str) -> ToggleButton {
    let mut r = row(parent, SETTING_ROW_H);
    let mut left = Flex::default().column();
    left.set_spacing(2);
    let t = label(title, 13, theme::TEXT);
    left.fixed(&t, 18);
    let d = label(desc, 11, theme::TEXT_FAINT);
    left.fixed(&d, 14);
    left.end();
    let mut sw = ToggleButton::default();
    widgets::style_switch(&mut sw);
    r.fixed(&sw, 42);
    r.end();
    sw
}

fn error_frame() -> Frame {
    let mut e = Frame::default();
    e.set_label_size(11);
    e.set_label_color(theme::RED);
    e.set_align(Align::Left | Align::Inside);
    e
}

// ---------------------------------------------------------------- build

pub fn build(
    state: Arc<SharedAppState>,
    cmd_tx: mpsc::Sender<AppCommand>,
    root: PathBuf,
    show: bool,
) -> MainWindow {
    let mut win = Window::default().with_size(W, DEFAULT_H).with_label("escape-proxy");
    win.begin();
    let mut col = Flex::default_fill().column();
    // Margins keep content off the window edges; spacing separates cards.
    col.set_margin(PAD);
    col.set_spacing(GAP);

    // ---- header ----------------------------------------------------------
    let mut header = row(&mut col, HEADER_H);
    let mut title = Frame::default().with_label("escape-proxy");
    title.set_label_size(16);
    title.set_label_font(theme::semibold());
    title.set_label_color(theme::TEXT);
    title.set_align(Align::Left | Align::Inside);
    let _gap = Frame::default();
    let mut status_badge = Widget::default().with_label("Starting");
    widgets::style_badge(&mut status_badge);
    header.fixed(&status_badge, 120);
    header.end();

    // ---- card: proxy -------------------------------------------------------
    let mut proxy_card = card_column(&mut col, Some(PROXY_CARD_H));

    let mut enabled_row = row(&mut proxy_card, SWITCH_ROW_H);
    label("Proxy enabled", 13, theme::TEXT);
    let _gap = Frame::default();
    let mut enabled_toggle = ToggleButton::default();
    widgets::style_switch(&mut enabled_toggle);
    enabled_row.fixed(&enabled_toggle, 42);
    enabled_row.end();
    {
        let cmd_tx = cmd_tx.clone();
        enabled_toggle.set_callback(move |b| {
            let _ = cmd_tx.blocking_send(AppCommand::SetEnabled(b.value()));
            b.redraw();
        });
    }

    let addr_value = info_row(&mut proxy_card, "Listening");
    let activity_value = info_row(&mut proxy_card, "Activity");

    let error_label = error_frame();
    proxy_card.fixed(&error_label, ERROR_ROW_H);
    proxy_card.end();

    // ---- card: domains -----------------------------------------------------
    let mut domains_card = card_column(&mut col, None); // flexes: list grows with window

    widgets::section_title(&mut domains_card, "DOMAINS");

    let mut restrict_row = row(&mut domains_card, CHECK_ROW_H);
    label("Restrict to listed domains", 13, theme::TEXT);
    let _gap = Frame::default();
    let mut restrict_switch = ToggleButton::default();
    widgets::style_switch(&mut restrict_switch);
    restrict_row.fixed(&restrict_switch, 42);
    restrict_row.end();
    {
        let cmd_tx = cmd_tx.clone();
        restrict_switch.set_callback(move |b| {
            let _ = cmd_tx.blocking_send(AppCommand::SetRestrictDomains(b.value()));
            b.redraw();
        });
    }

    let mut domains_hint = Frame::default().with_label(HINT_UNRESTRICTED);
    domains_hint.set_label_size(11);
    domains_hint.set_label_color(theme::TEXT_FAINT);
    domains_hint.set_align(Align::Left | Align::Inside);
    domains_card.fixed(&domains_hint, HINT_H);

    let mut domain_browser = HoldBrowser::default();
    domains::style_browser(&mut domain_browser);

    let mut add_row = row(&mut domains_card, ADD_ROW_H);
    let mut field = Flex::default().row();
    field.set_margins(8, 0, 8, 0); // text inset; FLTK inputs have no padding
    widgets::style_field(&mut field);
    let mut domain_input = Input::default();
    style_input(&mut domain_input);
    field.end();
    let mut add_btn = Button::default().with_label("Add");
    style_button(&mut add_btn, ButtonStyle::Primary);
    add_row.fixed(&add_btn, 74);
    add_row.end();

    let mut remove_row = row(&mut domains_card, REMOVE_ROW_H);
    let domain_error = error_frame();
    let _gap = Frame::default();
    let mut remove_btn = Button::default().with_label("Remove selected");
    style_button(&mut remove_btn, ButtonStyle::Secondary);
    // Right-aligned under the Add button so both actions share a column.
    remove_row.fixed(&remove_btn, 120);
    remove_row.end();
    domains_card.end();

    // Enter in the input adds a domain; clicking Add does the same.
    {
        domain_input.set_trigger(CallbackTrigger::EnterKey);
        domain_input.set_callback({
            let state = state.clone();
            let cmd_tx = cmd_tx.clone();
            let mut error = domain_error.clone();
            move |i| domains::submit(i, &mut error, &state, &cmd_tx)
        });
    }
    {
        let state = state.clone();
        let cmd_tx = cmd_tx.clone();
        let mut input = domain_input.clone();
        let mut error = domain_error.clone();
        add_btn.set_callback(move |_| domains::submit(&mut input, &mut error, &state, &cmd_tx));
    }
    {
        let state = state.clone();
        let cmd_tx = cmd_tx.clone();
        let browser = domain_browser.clone();
        remove_btn.set_callback(move |_| domains::remove_selected(&browser, &state, &cmd_tx));
    }

    // ---- card: windows -------------------------------------------------------
    let mut windows_card = card_column(&mut col, Some(WINDOWS_CARD_H));

    let mut autostart_switch = setting_row(
        &mut windows_card,
        "Start with Windows",
        "Run automatically at sign-in, in the tray.",
    );
    {
        let state = state.clone();
        autostart_switch.set_callback(move |b| {
            windows::apply_autostart(&state, b.value());
            b.redraw();
        });
    }
    let mut startup_switch = setting_row(
        &mut windows_card,
        "Show settings on startup",
        "Open this window whenever the app launches.",
    );
    {
        let cmd_tx = cmd_tx.clone();
        startup_switch.set_callback(move |b| {
            let _ = cmd_tx.blocking_send(AppCommand::SetShowOnStartup(b.value()));
            b.redraw();
        });
    }
    windows_card.end();

    // ---- card: advanced (collapsible) ----------------------------------------
    let mut advanced_card = card_column(&mut col, Some(ADV_COLLAPSED_H));

    let mut adv_header = row(&mut advanced_card, ADV_HEADER_H);
    label("ADVANCED", 10, theme::TEXT_FAINT);
    let _gap = Frame::default();
    let mut advanced_button = Button::default().with_label("Expand \u{25be}");
    advanced_button.set_label_size(12);
    style_button(&mut advanced_button, ButtonStyle::Secondary);
    adv_header.fixed(&advanced_button, 90);
    adv_header.end();

    let mut adv_body = Flex::default().column();
    adv_body.set_spacing(ROW_GAP);

    // Seed the form from the effective settings. Hard-coded defaults would
    // misreport a customised config until the first status event, and Apply
    // would then write them.
    let initial = state.snapshot();
    let port_input = input_row(&mut adv_body, "Listening port", &port_text(&initial));
    let maxconn_input =
        input_row(&mut adv_body, "Max connections", &initial.max_connections.to_string());
    let timeout_input =
        input_row(&mut adv_body, "Connect timeout (ms)", &initial.connect_timeout_ms.to_string());

    let mut apply_row = row(&mut adv_body, APPLY_ROW_H);
    let advanced_error = error_frame();
    let mut config_btn = Button::default().with_label("Open config folder");
    config_btn.set_label_size(12);
    style_button(&mut config_btn, ButtonStyle::Secondary);
    apply_row.fixed(&config_btn, 140);
    let mut apply_btn = Button::default().with_label("Apply");
    style_button(&mut apply_btn, ButtonStyle::Primary);
    apply_row.fixed(&apply_btn, 80);
    apply_row.end();

    adv_body.end();

    {
        let cmd_tx = cmd_tx.clone();
        let port = port_input.clone();
        let maxconn = maxconn_input.clone();
        let timeout = timeout_input.clone();
        let mut error = advanced_error.clone();
        apply_btn.set_callback(move |_| {
            match settings::parse_advanced(&port.value(), &maxconn.value(), &timeout.value()) {
                Ok((port, max_connections, connect_timeout_ms)) => {
                    error.set_label("");
                    error.redraw();
                    let _ = cmd_tx.blocking_send(AppCommand::ApplyAdvanced {
                        port,
                        max_connections,
                        connect_timeout_ms,
                    });
                }
                Err(e) => {
                    error.set_label(&e);
                    error.redraw();
                }
            }
        });
    }
    {
        let root = root.clone();
        config_btn.set_callback(move |_| windows::open_folder(&root));
    }
    advanced_card.end();

    // Body starts hidden; the header row stays visible so the expander is
    // always reachable (it used to be hidden together with the body).
    adv_body.hide();

    {
        let mut advanced_card = advanced_card.clone();
        let mut adv_body = adv_body.clone();
        let mut col2 = col.clone();
        let mut win2 = win.clone();
        advanced_button.set_callback(move |b| {
            let expand = b.label().starts_with("Expand");
            if expand {
                advanced_card.fixed(&adv_body, ADV_BODY_H);
                adv_body.show();
                col2.fixed(&advanced_card, ADV_EXPANDED_H);
                b.set_label("Collapse \u{25b4}");
                grow_window(&mut win2, ADV_EXPANDED_H - ADV_COLLAPSED_H);
            } else {
                adv_body.hide();
                advanced_card.fixed(&adv_body, 0);
                col2.fixed(&advanced_card, ADV_COLLAPSED_H);
                b.set_label("Expand \u{25be}");
                grow_window(&mut win2, ADV_COLLAPSED_H - ADV_EXPANDED_H);
            }
            col2.recalc();
            advanced_card.redraw();
            b.redraw();
        });
    }

    // ---- footer ---------------------------------------------------------------
    let mut footer = row(&mut col, FOOTER_H);
    let mut version = Frame::default().with_label(concat!("v", env!("CARGO_PKG_VERSION")));
    version.set_label_size(11);
    version.set_label_color(theme::TEXT_FAINT);
    version.set_align(Align::Left | Align::Inside);
    footer.fixed(&version, 60);
    let _gap = Frame::default();
    let mut logs_btn = Button::default().with_label("Open logs");
    style_button(&mut logs_btn, ButtonStyle::Secondary);
    footer.fixed(&logs_btn, 110);
    footer.end();
    {
        let logs_dir = root.join("logs");
        logs_btn.set_callback(move |_| windows::open_folder(&logs_dir));
    }

    col.end();
    win.end();
    win.resizable(&col);
    win.size_range(W, MIN_H, W, MAX_H);
    // Closing the window hides it to the tray; the app keeps running.
    win.set_callback(|w| w.hide());
    if show {
        win.show();
        // The HWND only exists once shown; the dark title bar needs it.
        dark_titlebar(&win);
    }
    MainWindow {
        win,
        status_badge,
        error_label,
        enabled_toggle,
        addr_value,
        activity_value,
        restrict_switch,
        domains_hint,
        domain_browser,
        autostart_switch,
        startup_switch,
        port_input,
        maxconn_input,
        timeout_input,
        // Sentinel that never matches a real signature, so the first tick
        // populates the browser (including its empty-list placeholder).
        domains_sig: String::from("\u{1}uninitialized"),
        advanced_sig: advanced_signature(&initial),
    }
}

/// Resizes the window by `delta` while keeping it inside the height range.
fn grow_window(win: &mut Window, delta: i32) {
    let h = (win.h() + delta).clamp(MIN_H, MAX_H);
    win.resize(win.x(), win.y(), W, h);
}

/// Switches the native title bar to dark so it blends with the window.
fn dark_titlebar(win: &Window) {
    windows::set_dark_titlebar(win.raw_handle() as _);
}

impl MainWindow {
    pub fn show_window(&mut self, state: &SharedAppState) {
        self.win.show();
        // The HWND only exists once shown; the dark title bar needs it.
        dark_titlebar(&self.win);
        // Refresh now rather than on the next periodic tick.
        self.tick(state);
    }

    /// Refreshes dynamic widgets. Called from the main loop while the window
    /// is visible (periodic tick and status-change events) and when it is
    /// shown.
    pub fn tick(&mut self, state: &SharedAppState) {
        let snap = state.live_snapshot();
        overview::refresh_status(&mut self.status_badge, &mut self.error_label, &snap);

        let on = matches!(snap.state, ProxyState::Running | ProxyState::Starting);
        if self.enabled_toggle.value() != on {
            self.enabled_toggle.set_value(on);
            self.enabled_toggle.redraw();
        }
        // Only touch labels that changed; unconditional set_label calls
        // would keep the window damaged and busy-spin the FLTK loop.
        if self.addr_value.label() != snap.listen_addr {
            self.addr_value.set_label(&snap.listen_addr);
        }
        let activity = format!(
            "{} connection{} \u{b7} {}",
            snap.active_connections,
            if snap.active_connections == 1 { "" } else { "s" },
            format_bytes(snap.total_bytes())
        );
        if self.activity_value.label() != activity {
            self.activity_value.set_label(&activity);
        }

        if self.restrict_switch.value() != snap.restrict_domains {
            self.restrict_switch.set_value(snap.restrict_domains);
            self.restrict_switch.redraw();
        }
        let hint = if snap.restrict_domains {
            HINT_RESTRICTED
        } else {
            HINT_UNRESTRICTED
        };
        if self.domains_hint.label() != hint {
            self.domains_hint.set_label(hint);
            self.domains_hint.redraw();
        }

        let sig = snap.domains.join("\u{0}");
        if sig != self.domains_sig {
            self.domains_sig = sig;
            domains::refresh(&mut self.domain_browser, &snap.domains);
        }

        if self.autostart_switch.value() != snap.autostart {
            self.autostart_switch.set_value(snap.autostart);
            self.autostart_switch.redraw();
        }
        if self.startup_switch.value() != snap.show_on_startup {
            self.startup_switch.set_value(snap.show_on_startup);
            self.startup_switch.redraw();
        }
        // Reload the form only when the effective settings changed, so values
        // typed but not applied yet survive other status updates.
        let sig = advanced_signature(&snap);
        if sig != self.advanced_sig {
            self.advanced_sig = sig;
            self.port_input.set_value(&port_text(&snap));
            self.maxconn_input.set_value(&snap.max_connections.to_string());
            self.timeout_input.set_value(&snap.connect_timeout_ms.to_string());
        }
    }
}
