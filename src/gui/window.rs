//! Settings window composition: a single compact dark window with the
//! proxy overview, domain filter + editor, Windows settings and advanced
//! options, each laid out as a rounded-feeling "card" on the window
//! background.

use std::path::PathBuf;
use std::sync::Arc;

use fltk::browser::HoldBrowser;
use fltk::button::{Button, CheckButton, ToggleButton};
use fltk::enums::{Align, CallbackTrigger, Font, FrameType};
use fltk::frame::Frame;
use fltk::group::Flex;
use fltk::input::{Input, IntInput};
use fltk::prelude::*;
use fltk::window::Window;
use tokio::sync::mpsc;

use escape_proxy::app::commands::AppCommand;
use escape_proxy::app::state::{ProxyState, SharedAppState};
use escape_proxy::app::statistics::format_bytes;
use escape_proxy::platform::windows;

use crate::gui::{domains, overview, settings, theme};

pub const W: i32 = 480;
pub const COLLAPSED_H: i32 = 560;
pub const EXPANDED_H: i32 = 730;
/// Fixed slot the advanced section occupies while expanded.
const ADVANCED_H: i32 = 168;

const HINT_RESTRICTED: &str = "Only the listed domains and their subdomains are forwarded.";
const HINT_UNRESTRICTED: &str = "All domains are forwarded. Enable \u{201c}Restrict to list\u{201d} to limit to the list below.";

pub struct MainWindow {
    pub win: Window,
    status_badge: Frame,
    error_label: Frame,
    enabled_toggle: ToggleButton,
    addr_value: Frame,
    conns_value: Frame,
    bytes_value: Frame,
    restrict_check: CheckButton,
    domains_hint: Frame,
    domain_browser: HoldBrowser,
    autostart_check: CheckButton,
    startup_check: CheckButton,
    port_input: IntInput,
    maxconn_input: IntInput,
    timeout_input: IntInput,
    domains_sig: String,
}

// ------------------------------------------------------------- card helpers

/// A flat section: column flex, no card chrome. Sections are separated by a
/// thin separator line in the outer column.
fn section() -> Flex {
    let mut c = Flex::default().column();
    c.set_spacing(7);
    c
}

/// Thin separator line between sections.
fn separator(col: &mut Flex) {
    let mut sep = Frame::default();
    col.fixed(&sep, 2);
    sep.set_frame(FrameType::FlatBox);
    sep.set_color(theme::SEPARATOR);
}

/// Small uppercase-style section title, standalone (no trailing control).
fn title_label(card: &mut Flex, title: &str) {
    let row = Flex::default().row();
    card.fixed(&row, 16);
    let mut t = Frame::default().with_label(title);
    t.set_label_size(11);
    t.set_label_color(theme::TEXT_DIM);
    t.set_align(Align::Left | Align::Inside);
    row.end();
}

/// A label/value row inside a card.
fn info_row(card: &mut Flex, label: &str, value: &str) -> Frame {
    let mut row = Flex::default().row();
    card.fixed(&row, 22);
    let mut l = Frame::default().with_label(label);
    l.set_label_size(13);
    l.set_label_color(theme::TEXT_DIM);
    l.set_align(Align::Left | Align::Inside);
    row.fixed(&l, 150);
    let _gap = Frame::default();
    let mut v = Frame::default().with_label(value);
    v.set_label_size(13);
    v.set_label_color(theme::TEXT);
    v.set_align(Align::Right | Align::Inside);
    row.fixed(&v, 190);
    row.end();
    v
}

/// A label/input row inside a card.
fn input_row(card: &mut Flex, label: &str, default: &str) -> (Flex, IntInput) {
    let mut row = Flex::default().row();
    card.fixed(&row, 26);
    let mut l = Frame::default().with_label(label);
    l.set_label_size(13);
    l.set_label_color(theme::TEXT_DIM);
    l.set_align(Align::Left | Align::Inside);
    row.fixed(&l, 170);
    let _gap = Frame::default();
    let mut input = IntInput::default();
    input.set_value(default);
    input.set_label_size(13);
    style_input(&mut input);
    row.fixed(&input, 110);
    row.end();
    (row, input)
}

fn style_input<I: WidgetExt + InputExt>(input: &mut I) {
    input.set_color(theme::INPUT_BG);
    input.set_text_color(theme::TEXT);
    input.set_text_size(13);
    input.set_selection_color(theme::ACCENT);
}

fn style_button(b: &mut Button) {
    b.set_frame(FrameType::FlatBox);
    b.set_color(theme::INPUT_BG);
    b.set_label_color(theme::TEXT);
    b.set_label_size(13);
    b.set_selection_color(theme::INPUT_BG_HI);
}

// ---------------------------------------------------------------- build

pub fn build(
    state: Arc<SharedAppState>,
    cmd_tx: mpsc::Sender<AppCommand>,
    root: PathBuf,
    show: bool,
) -> MainWindow {
    let mut win = Window::default().with_size(W, COLLAPSED_H).with_label("escape-proxy");
    win.begin();
    let mut col = Flex::default_fill().column();
    col.set_pad(14);
    col.set_spacing(10);

    // ---- header ----------------------------------------------------------
    let mut header = Flex::default().row();
    col.fixed(&header, 32);
    let lead = Frame::default();
    header.fixed(&lead, 6);
    let mut title = Frame::default().with_label("escape-proxy");
    title.set_label_size(17);
    title.set_label_font(Font::HelveticaBold);
    title.set_label_color(theme::TEXT);
    title.set_align(Align::Left | Align::Inside);
    header.fixed(&title, 160);
    let _gap = Frame::default();
    let mut status_badge = Frame::default().with_label("\u{25cf} Starting");
    status_badge.set_label_size(13);
    status_badge.set_label_color(theme::GRAY);
    status_badge.set_align(Align::Right | Align::Inside);
    header.fixed(&status_badge, 130);
    header.end();

    // ---- section: proxy -------------------------------------------------------
    let mut proxy_card = section();
    col.fixed(&proxy_card, 165);
    title_label(&mut proxy_card, "PROXY");

    let mut enabled_row = Flex::default().row();
    proxy_card.fixed(&enabled_row, 30);
    let mut enabled_label = Frame::default().with_label("Proxy enabled");
    enabled_label.set_label_size(13);
    enabled_label.set_label_color(theme::TEXT);
    enabled_label.set_align(Align::Left | Align::Inside);
    enabled_row.fixed(&enabled_label, 150);
    let _gap = Frame::default();
    let mut enabled_toggle = ToggleButton::default().with_label("ON");
    enabled_toggle.set_frame(FrameType::FlatBox);
    enabled_toggle.set_color(theme::INPUT_BG);
    enabled_toggle.set_selection_color(theme::GREEN_FACE);
    enabled_toggle.set_label_size(13);
    enabled_toggle.set_label_color(theme::TEXT);
    enabled_row.fixed(&enabled_toggle, 88);
    enabled_row.end();
    {
        let cmd_tx = cmd_tx.clone();
        enabled_toggle.set_callback(move |b| {
            let on = b.value();
            apply_toggle_look(b, on);
            let _ = cmd_tx.blocking_send(AppCommand::SetEnabled(on));
        });
    }

    let addr_value = info_row(&mut proxy_card, "Listening", "127.0.0.1:10808");
    let conns_value = info_row(&mut proxy_card, "Active connections", "0");
    let bytes_value = info_row(&mut proxy_card, "Data transferred", "0 B");

    let mut error_label = Frame::default().with_label("");
    error_label.set_label_color(theme::RED);
    error_label.set_label_size(11);
    error_label.set_align(Align::Left | Align::Inside);
    proxy_card.fixed(&error_label, 14);
    proxy_card.end();

    separator(&mut col);

    // ---- section: domains -----------------------------------------------------
    let mut domains_card = section();
    // No fixed height: this section flexes with the window.

    let mut filter_row = Flex::default().row();
    domains_card.fixed(&filter_row, 24);
    let mut dl = Frame::default().with_label("DOMAINS");
    dl.set_label_size(11);
    dl.set_label_color(theme::TEXT_DIM);
    dl.set_align(Align::Left | Align::Inside);
    filter_row.fixed(&dl, 90);
    let _gap = Frame::default();
    let mut restrict_check = CheckButton::default().with_label("Restrict to list");
    restrict_check.set_label_size(12);
    restrict_check.set_label_color(theme::TEXT);
    restrict_check.set_color(theme::CHECK_BOX);
    restrict_check.set_selection_color(theme::ACCENT);
    filter_row.fixed(&restrict_check, 130);
    filter_row.end();
    {
        let cmd_tx = cmd_tx.clone();
        restrict_check.set_callback(move |b| {
            let _ = cmd_tx.blocking_send(AppCommand::SetRestrictDomains(b.value()));
        });
    }

    let mut domains_hint = Frame::default().with_label(HINT_UNRESTRICTED);
    domains_hint.set_label_size(11);
    domains_hint.set_label_color(theme::TEXT_DIM);
    domains_hint.set_align(Align::Left | Align::Wrap | Align::Inside);
    domains_card.fixed(&domains_hint, 28);

    let mut domain_browser = HoldBrowser::default();
    domains::style_browser(&mut domain_browser);

    let mut add_row = Flex::default().row();
    domains_card.fixed(&add_row, 30);
    let mut domain_input = Input::default();
    style_input(&mut domain_input);
    let mut add_btn = Button::default().with_label("Add");
    style_button(&mut add_btn);
    add_row.fixed(&add_btn, 64);
    add_row.end();

    let mut remove_row = Flex::default().row();
    domains_card.fixed(&remove_row, 24);
    let mut remove_btn = Button::default().with_label("Remove selected");
    style_button(&mut remove_btn);
    remove_btn.set_label_size(12);
    remove_row.fixed(&remove_btn, 130);
    let mut domain_error = Frame::default().with_label("");
    domain_error.set_label_color(theme::RED);
    domain_error.set_label_size(11);
    domain_error.set_align(Align::Left | Align::Inside);
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

    separator(&mut col);

    // ---- section: windows ------------------------------------------------------
    let mut windows_card = section();
    col.fixed(&windows_card, 96);
    title_label(&mut windows_card, "WINDOWS");
    let mut autostart_check = CheckButton::default().with_label("Start with Windows");
    style_check(&mut autostart_check);
    windows_card.fixed(&autostart_check, 24);
    {
        let state = state.clone();
        autostart_check.set_callback(move |b| windows::apply_autostart(&state, b.value()));
    }
    let mut startup_check = CheckButton::default().with_label("Show settings on startup");
    style_check(&mut startup_check);
    windows_card.fixed(&startup_check, 24);
    {
        let cmd_tx = cmd_tx.clone();
        startup_check.set_callback(move |b| {
            let _ = cmd_tx.blocking_send(AppCommand::SetShowOnStartup(b.value()));
        });
    }
    let mut windows_hint = Frame::default().with_label("Autostart launches the app tray-only at login.");
    windows_hint.set_label_size(11);
    windows_hint.set_label_color(theme::TEXT_DIM);
    windows_hint.set_align(Align::Left | Align::Inside);
    windows_card.fixed(&windows_hint, 14);
    windows_card.end();

    separator(&mut col);

    // ---- section: advanced (collapsible) ----------------------------------------
    let mut advanced_section = section();
    // Height set when toggling; hidden while collapsed.

    let mut adv_row = Flex::default().row();
    advanced_section.fixed(&adv_row, 24);
    let mut al = Frame::default().with_label("ADVANCED");
    al.set_label_size(11);
    al.set_label_color(theme::TEXT_DIM);
    al.set_align(Align::Left | Align::Inside);
    adv_row.fixed(&al, 90);
    let _gap = Frame::default();
    let mut advanced_button = Button::default().with_label("Expand");
    style_button(&mut advanced_button);
    advanced_button.set_label_size(12);
    adv_row.fixed(&advanced_button, 84);
    adv_row.end();

    let (_port_row, port_input) = input_row(&mut advanced_section, "Listening port", "10808");
    let (_maxconn_row, maxconn_input) = input_row(&mut advanced_section, "Max connections", "128");
    let (_timeout_row, timeout_input) = input_row(&mut advanced_section, "Connect timeout (ms)", "5000");

    let mut apply_row = Flex::default().row();
    advanced_section.fixed(&apply_row, 30);
    let mut advanced_error = Frame::default().with_label("");
    advanced_error.set_label_color(theme::RED);
    advanced_error.set_label_size(11);
    advanced_error.set_align(Align::Left | Align::Inside);
    let mut apply_btn = Button::default().with_label("Apply");
    style_button(&mut apply_btn);
    apply_row.fixed(&apply_btn, 84);
    let mut config_btn = Button::default().with_label("Open config folder");
    style_button(&mut config_btn);
    config_btn.set_label_size(12);
    apply_row.fixed(&config_btn, 140);
    apply_row.end();

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
                    let _ = cmd_tx.blocking_send(AppCommand::ApplyAdvanced {
                        port,
                        max_connections,
                        connect_timeout_ms,
                    });
                }
                Err(e) => error.set_label(&e),
            }
        });
    }
    {
        let root = root.clone();
        config_btn.set_callback(move |_| windows::open_folder(&root));
    }
    advanced_section.end();

    // Advanced card starts hidden (collapsed window).
    advanced_section.hide();

    // Expand/collapse wiring: showing the card gives it a fixed slot in the
    // column and the window grows accordingly.
    {
        let mut advanced = advanced_section.clone();
        let mut col2 = col.clone();
        let mut win2 = win.clone();
        advanced_button.set_callback(move |b| {
            let expand = b.label() == "Expand";
            if expand {
                col2.fixed(&advanced, ADVANCED_H);
                advanced.show();
                col2.recalc();
                win2.size_range(W, EXPANDED_H, W, EXPANDED_H);
                win2.resize(win2.x(), win2.y(), W, EXPANDED_H);
                b.set_label("Collapse");
            } else {
                advanced.hide();
                col2.fixed(&advanced, 0);
                col2.recalc();
                win2.size_range(W, COLLAPSED_H, W, COLLAPSED_H);
                win2.resize(win2.x(), win2.y(), W, COLLAPSED_H);
                b.set_label("Expand");
            }
        });
    }

    // ---- footer ---------------------------------------------------------------
    let mut footer = Flex::default().row();
    col.fixed(&footer, 28);
    let mut version = Frame::default().with_label(concat!("v", env!("CARGO_PKG_VERSION")));
    version.set_label_size(11);
    version.set_label_color(theme::TEXT_DIM);
    version.set_align(Align::Left | Align::Inside);
    footer.fixed(&version, 60);
    let _gap = Frame::default();
    let mut logs_btn = Button::default().with_label("Open logs");
    style_button(&mut logs_btn);
    footer.fixed(&logs_btn, 110);
    footer.end();
    {
        let logs_dir = root.join("logs");
        logs_btn.set_callback(move |_| windows::open_folder(&logs_dir));
    }

    col.end();
    win.end();
    win.size_range(W, COLLAPSED_H, W, COLLAPSED_H);
    // Closing the window hides it to the tray; the app keeps running.
    win.set_callback(|w| w.hide());
    if show {
        win.show();
    }

    MainWindow {
        win,
        status_badge,
        error_label,
        enabled_toggle,
        addr_value,
        conns_value,
        bytes_value,
        restrict_check,
        domains_hint,
        domain_browser,
        autostart_check,
        startup_check,
        port_input,
        maxconn_input,
        timeout_input,
        domains_sig: String::new(),
    }
}

fn style_check(c: &mut CheckButton) {
    c.set_label_size(13);
    c.set_label_color(theme::TEXT);
    c.set_color(theme::CHECK_BOX);
    c.set_selection_color(theme::ACCENT);
    c.set_down_frame(FrameType::RoundedBox);
}

fn apply_toggle_look(b: &mut ToggleButton, on: bool) {
    b.set_label(if on { "ON" } else { "OFF" });
    b.set_color(if on { theme::GREEN_FACE } else { theme::INPUT_BG });
    b.set_label_color(if on { theme::TEXT } else { theme::TEXT_DIM });
}

impl MainWindow {
    pub fn show_window(&mut self) {
        self.win.show();
    }

    /// Refreshes dynamic widgets. Called from the main loop only while the
    /// window is visible (plus once on status-change events).
    pub fn tick(&mut self, state: &SharedAppState, config_changed: bool) {
        let snap = state.live_snapshot();
        overview::refresh_status(&mut self.status_badge, &mut self.error_label, &snap);

        let on = matches!(snap.state, ProxyState::Running | ProxyState::Starting);
        if self.enabled_toggle.value() != on {
            self.enabled_toggle.set_value(on);
            apply_toggle_look(&mut self.enabled_toggle, on);
        }
        // Only touch labels that changed; unconditional set_label calls
        // would keep the window damaged and busy-spin the FLTK loop.
        if self.addr_value.label() != snap.listen_addr {
            self.addr_value.set_label(&snap.listen_addr);
        }
        let conns = snap.active_connections.to_string();
        if self.conns_value.label() != conns {
            self.conns_value.set_label(&conns);
        }
        let bytes = format_bytes(snap.total_bytes());
        if self.bytes_value.label() != bytes {
            self.bytes_value.set_label(&bytes);
        }

        if self.restrict_check.value() != snap.restrict_domains {
            self.restrict_check.set_value(snap.restrict_domains);
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

        if self.autostart_check.value() != snap.autostart {
            self.autostart_check.set_value(snap.autostart);
        }
        if self.startup_check.value() != snap.show_on_startup {
            self.startup_check.set_value(snap.show_on_startup);
        }
        if config_changed {
            let port = snap.listen_addr.rsplit(':').next().unwrap_or("").to_string();
            self.port_input.set_value(&port);
            self.maxconn_input.set_value(&snap.max_connections.to_string());
            self.timeout_input.set_value(&snap.connect_timeout_ms.to_string());
        }
    }
}
