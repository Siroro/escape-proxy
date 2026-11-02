//! Settings window composition: a single compact dark window containing the
//! overview, domain editor and settings sections.

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

pub const W: i32 = 460;
pub const COLLAPSED_H: i32 = 520;
pub const EXPANDED_H: i32 = 640;

pub struct MainWindow {
    pub win: Window,
    status_dot: Frame,
    status_label: Frame,
    error_label: Frame,
    enabled_toggle: ToggleButton,
    addr_value: Frame,
    conns_value: Frame,
    bytes_value: Frame,
    domain_browser: HoldBrowser,
    autostart_check: CheckButton,
    startup_check: CheckButton,
    port_input: IntInput,
    maxconn_input: IntInput,
    timeout_input: IntInput,
    domains_sig: String,
}

fn separator(col: &mut Flex) {
    let mut sep = Frame::default();
    col.fixed(&sep, 2);
    sep.set_frame(FrameType::FlatBox);
    sep.set_color(theme::SEPARATOR);
}

fn dim_label(label: &str) -> Frame {
    let mut f = Frame::default().with_label(label);
    f.set_label_color(theme::TEXT_DIM);
    f.set_align(Align::Left | Align::Inside);
    f
}

fn info_row(col: &mut Flex, label: &str, value: &str) -> Frame {
    let mut row = Flex::default().row();
    col.fixed(&row, 24);
    let l = dim_label(label);
    row.fixed(&l, 190);
    let _gap = Frame::default();
    let mut v = Frame::default().with_label(value);
    v.set_label_color(theme::TEXT);
    v.set_align(Align::Right | Align::Inside);
    row.fixed(&v, 170);
    row.end();
    v
}

fn adv_input_row(col: &mut Flex, label: &str, default: &str) -> (Flex, IntInput) {
    let mut row = Flex::default().row();
    col.fixed(&row, 26);
    let l = dim_label(label);
    row.fixed(&l, 190);
    let _gap = Frame::default();
    let mut input = IntInput::default();
    input.set_value(default);
    input.set_color(theme::INPUT_BG);
    input.set_text_color(theme::TEXT);
    input.set_label_color(theme::TEXT);
    row.fixed(&input, 110);
    row.end();
    (row, input)
}

pub fn build(
    state: Arc<SharedAppState>,
    cmd_tx: mpsc::Sender<AppCommand>,
    root: PathBuf,
    show: bool,
) -> MainWindow {
    let mut win = Window::default().with_size(W, COLLAPSED_H).with_label("escape-proxy");
    win.begin();
    let mut col = Flex::default_fill().column();
    col.set_pad(12);
    col.set_spacing(5);

    // ---- header -------------------------------------------------------
    let mut header = Flex::default().row();
    col.fixed(&header, 30);
    let mut title = Frame::default().with_label("escape-proxy");
    title.set_label_size(17);
    title.set_label_font(Font::HelveticaBold);
    title.set_label_color(theme::TEXT);
    title.set_align(Align::Left | Align::Inside);
    header.fixed(&title, 170);
    let _gap = Frame::default();
    let mut status_dot = Frame::default().with_label("\u{25cf}");
    status_dot.set_label_size(15);
    status_dot.set_label_color(theme::GRAY);
    header.fixed(&status_dot, 20);
    let mut status_label = Frame::default().with_label("Starting");
    status_label.set_label_color(theme::TEXT_DIM);
    status_label.set_align(Align::Right | Align::Inside);
    header.fixed(&status_label, 110);
    header.end();

    separator(&mut col);

    // ---- enabled toggle -------------------------------------------------
    let mut enabled_row = Flex::default().row();
    col.fixed(&enabled_row, 30);
    let mut enabled_label = dim_label("Proxy enabled");
    enabled_label.set_label_color(theme::TEXT);
    enabled_row.fixed(&enabled_label, 190);
    let _gap = Frame::default();
    let mut enabled_toggle = ToggleButton::default().with_label("ON");
    enabled_row.fixed(&enabled_toggle, 90);
    enabled_row.end();
    {
        let cmd_tx = cmd_tx.clone();
        enabled_toggle.set_callback(move |b| {
            let on = b.value();
            b.set_label(if on { "ON" } else { "OFF" });
            let _ = cmd_tx.blocking_send(AppCommand::SetEnabled(on));
        });
    }

    // ---- runtime info ----------------------------------------------------
    let addr_value = info_row(&mut col, "SOCKS5", "127.0.0.1:10808");
    let conns_value = info_row(&mut col, "Active connections", "0");
    let bytes_value = info_row(&mut col, "Data transferred", "0 B");

    let mut error_label = Frame::default().with_label("");
    error_label.set_label_color(theme::RED);
    error_label.set_label_size(11);
    error_label.set_align(Align::Left | Align::Inside);
    col.fixed(&error_label, 15);

    separator(&mut col);

    // ---- domain editor -----------------------------------------------------
    let domains_label = dim_label("Allowed domains");
    col.fixed(&domains_label, 20);

    let mut domain_browser = HoldBrowser::default();
    domains::style_browser(&mut domain_browser);

    let mut add_row = Flex::default().row();
    col.fixed(&add_row, 28);
    let mut domain_input = Input::default();
    domain_input.set_color(theme::INPUT_BG);
    domain_input.set_text_color(theme::TEXT);
    let mut add_btn = Button::default().with_label("+ Add");
    add_row.fixed(&add_btn, 70);
    add_row.end();

    let mut remove_row = Flex::default().row();
    col.fixed(&remove_row, 26);
    let mut remove_btn = Button::default().with_label("Remove selected");
    remove_row.fixed(&remove_btn, 140);
    let mut domain_error = Frame::default().with_label("");
    domain_error.set_label_color(theme::RED);
    domain_error.set_label_size(11);
    domain_error.set_align(Align::Left | Align::Inside);
    remove_row.end();

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
        let mut error = domain_error.clone();
        domain_input.set_trigger(CallbackTrigger::EnterKey);
        domain_input.set_callback(move |i| domains::submit(i, &mut error, &state, &cmd_tx));
    }
    {
        let state = state.clone();
        let cmd_tx = cmd_tx.clone();
        let browser = domain_browser.clone();
        remove_btn.set_callback(move |_| domains::remove_selected(&browser, &state, &cmd_tx));
    }

    separator(&mut col);

    // ---- settings checkboxes ----------------------------------------------
    let mut autostart_check = CheckButton::default().with_label("Start with Windows");
    col.fixed(&autostart_check, 24);
    {
        let state = state.clone();
        autostart_check.set_callback(move |b| windows::apply_autostart(&state, b.value()));
    }
    let mut startup_check = CheckButton::default().with_label("Show settings on startup");
    col.fixed(&startup_check, 24);
    {
        let cmd_tx = cmd_tx.clone();
        startup_check.set_callback(move |b| {
            let _ = cmd_tx.blocking_send(AppCommand::SetShowOnStartup(b.value()));
        });
    }

    // ---- advanced (collapsible) ---------------------------------------------
    let mut adv_row = Flex::default().row();
    col.fixed(&adv_row, 28);
    let adv_label = dim_label("Advanced settings");
    adv_row.fixed(&adv_label, 190);
    let _gap = Frame::default();
    let mut advanced_button = Button::default().with_label("Expand");
    adv_row.fixed(&advanced_button, 90);
    adv_row.end();

    let (mut port_row, port_input) = adv_input_row(&mut col, "Listening port", "10808");
    let (mut maxconn_row, maxconn_input) = adv_input_row(&mut col, "Max connections", "128");
    let (mut timeout_row, timeout_input) = adv_input_row(&mut col, "Connect timeout (ms)", "5000");

    let mut apply_row = Flex::default().row();
    col.fixed(&apply_row, 30);
    let mut advanced_error = Frame::default().with_label("");
    advanced_error.set_label_color(theme::RED);
    advanced_error.set_label_size(11);
    advanced_error.set_align(Align::Left | Align::Inside);
    let mut apply_btn = Button::default().with_label("Apply");
    apply_row.fixed(&apply_btn, 80);
    let mut config_btn = Button::default().with_label("Open config folder");
    apply_row.fixed(&config_btn, 150);
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

    // Advanced rows start hidden (collapsed window).
    for row in [&mut port_row, &mut maxconn_row, &mut timeout_row, &mut apply_row] {
        row.hide();
    }
    {
        let mut rows = vec![
            port_row.clone(),
            maxconn_row.clone(),
            timeout_row.clone(),
            apply_row.clone(),
        ];
        let col2 = col.clone();
        let mut win2 = win.clone();
        advanced_button.set_callback(move |b| {
            let expand = b.label() == "Expand";
            for row in rows.iter_mut() {
                if expand {
                    row.show();
                } else {
                    row.hide();
                }
            }
            col2.recalc();
            let h = if expand { EXPANDED_H } else { COLLAPSED_H };
            win2.size_range(W, h, W, h);
            win2.resize(win2.x(), win2.y(), W, h);
            b.set_label(if expand { "Collapse" } else { "Expand" });
        });
    }

    // ---- footer ------------------------------------------------------------
    let mut logs_row = Flex::default().row();
    col.fixed(&logs_row, 30);
    let _gap = Frame::default();
    let mut logs_btn = Button::default().with_label("Open logs");
    logs_row.fixed(&logs_btn, 110);
    logs_row.end();
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
        status_dot,
        status_label,
        error_label,
        enabled_toggle,
        addr_value,
        conns_value,
        bytes_value,
        domain_browser,
        autostart_check,
        startup_check,
        port_input,
        maxconn_input,
        timeout_input,
        domains_sig: String::new(),
    }
}

impl MainWindow {
    pub fn show_window(&mut self) {
        self.win.show();
    }

    /// Refreshes dynamic widgets. Called from the main loop only while the
    /// window is visible (plus once on status-change events).
    pub fn tick(&mut self, state: &SharedAppState, config_changed: bool) {
        let snap = state.live_snapshot();
        overview::refresh_status(&mut self.status_dot, &mut self.status_label, &mut self.error_label, &snap);

        let on = matches!(snap.state, ProxyState::Running | ProxyState::Starting);
        if self.enabled_toggle.value() != on {
            self.enabled_toggle.set_value(on);
            self.enabled_toggle.set_label(if on { "ON" } else { "OFF" });
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
