//! Domain allowlist editor: add/remove entries with immediate validation.
//! All persistence goes through the controller; nothing here touches the
//! config file directly.

use fltk::browser::HoldBrowser;
use fltk::frame::Frame;
use fltk::input::Input;
use fltk::prelude::*;

use escape_proxy::app::commands::AppCommand;
use escape_proxy::app::state::SharedAppState;
use escape_proxy::core::matcher;

use crate::gui::theme;

/// Validates the input, submits an updated allowlist to the controller and
/// reports validation errors immediately on `error`.
pub fn submit(
    input: &mut Input,
    error: &mut Frame,
    state: &SharedAppState,
    cmd_tx: &tokio::sync::mpsc::Sender<AppCommand>,
) {
    let raw = input.value();
    match matcher::normalize_domain(&raw) {
        Ok(domain) => {
            let mut domains = state.live_snapshot().domains;
            if !domains.contains(&domain) {
                domains.push(domain);
                domains.sort();
            }
            let _ = cmd_tx.blocking_send(AppCommand::UpdateDomains(domains));
            input.set_value("");
            error.set_label("");
        }
        Err(e) => error.set_label(&e.to_string()),
    }
}

pub fn remove_selected(
    browser: &HoldBrowser,
    state: &SharedAppState,
    cmd_tx: &tokio::sync::mpsc::Sender<AppCommand>,
) {
    let selected = browser.value();
    if selected <= 0 {
        return;
    }
    if let Some(text) = browser.text(selected) {
        let mut domains = state.live_snapshot().domains;
        domains.retain(|d| d != &text);
        let _ = cmd_tx.blocking_send(AppCommand::UpdateDomains(domains));
    }
}

pub fn refresh(browser: &mut HoldBrowser, domains: &[String]) {
    browser.clear();
    for d in domains {
        browser.add(d);
    }
}

pub fn style_browser(browser: &mut HoldBrowser) {
    browser.set_color(theme::INPUT_BG);
    browser.set_selection_color(theme::ACCENT);
}
