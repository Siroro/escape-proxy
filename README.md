# escape-proxy

A lightweight Windows SOCKS5 proxy with a system tray icon and a small
FLTK settings window. It replaces a plain `allow *` 3proxy setup with an
**allowlisted** proxy that forwards approved browser traffic directly through
the residential internet connection while everything else stays on the VPN.

```text
Brave (PIA: Only VPN)
   |
   v
FoxyProxy ------------------ matched site --> 127.0.0.1:10808 --> escape-proxy.exe
   |                                                            (PIA: Bypass VPN)
   +---------------------------- unmatched site --> PIA VPN --> Internet
```

**One process, one executable. No external proxy, no embedded browser, no
background services.**

## Features

- SOCKS5 (RFC 1928): no-auth, TCP CONNECT only; BIND and UDP ASSOCIATE are
  rejected.
- Domain allowlist with IDNA (punycode) normalisation and dot-guarded
  subdomain matching (`sub.google.com` matches `google.com`;
  `evilgoogle.com` never does).
- Strict destination policy: literal-IP requests are rejected, hostnames are
  resolved via system DNS, and only public routable addresses are connected
  (loopback/private/CGNAT/link-local/reserved ranges are refused).
- Fail closed: pausing or exiting never opens an alternate outbound route.
- System tray: status icon (green running / gray paused / yellow recent
  failures / red error), enable toggle, settings, autostart, logs, exit.
- Settings window: enabled toggle, live stats, domain editor with instant
  validation, Windows startup, advanced options (port / max connections /
  connect timeout).
- Windows integration: HKCU Run-key autostart (`--background`), named-mutex
  single instance with second-launch window activation, embedded icon and
  version metadata, rotating daily logs (7 kept).

## Install

Requires the Rust toolchain (MSVC) and VS Build Tools (C++ workload for FLTK).

```powershell
git clone https://github.com/Siroro/escape-proxy
cd escape-proxy
.\deploy.ps1
```

`deploy.ps1` builds the release binary, installs to
`%LOCALAPPDATA%\escape-proxy\` (`escape-proxy.exe`, `config.toml`, `logs\`),
registers autostart and starts the app. Use `-NoAutostart` to skip the
autostart registration.

## Configuration

`%LOCALAPPDATA%\escape-proxy\config.toml`:

```toml
listen_addr = "127.0.0.1:10808"
enabled = true
max_connections = 128
connect_timeout_ms = 5000
handshake_timeout_ms = 5000
domains = [
    "google.com",
]

[ui]
show_on_startup = false
theme = "dark"
```

- The bind IP is fixed at `127.0.0.1`; only the port is configurable.
- Changes made in the settings window apply immediately and are saved
  atomically (write-temp-then-rename); there is no file watcher.
- Autostart state lives in the registry (`HKCU\...\Run`), not in the config;
  the GUI reflects the actual registered state.
- Set `ESCAPE_PROXY_HOME` to relocate config + logs (used by tests).

## PIA + FoxyProxy setup

1. Test escape-proxy on an alternate port first (Advanced settings), with a
   temporary FoxyProxy rule pointing at it.
2. In PIA, add the installed `escape-proxy.exe` as **Bypass VPN** (split
   tunnel). Keep Brave as **Only VPN**.
3. Point FoxyProxy patterns at `127.0.0.1:10808` for the sites that should
   exit through the residential IP.
4. Verify: matched sites show the residential public IP, unmatched sites the
   PIA IP. Check DNS and IPv6 behaviour for each rule.
5. Pause escape-proxy and confirm matched requests **fail** (no fallback).
6. Stop 3proxy, disable its autostart, move escape-proxy to port 10808 and
   verify the original rules still work.

## Architecture

Four independent layers (see the design doc for the full specification):

| Layer | Location | Owns |
|---|---|---|
| Proxy core | `src/core/` | SOCKS5, allowlist, DNS policy, TCP relay, limits |
| Controller | `src/app/` | State authority, config, commands, statistics |
| Presentation | `src/gui/` | FLTK window, theme, user actions → commands |
| Platform | `src/platform/windows/` | Tray, autostart, single instance |

The proxy core is GUI-free and runs headlessly in the tests. The tray owns
its own Win32 message-loop thread; the GUI thread and the Tokio runtime
communicate only through typed channels and atomic status snapshots — GUI
widgets are never shared with the runtime, and the FLTK loop never blocks on
network operations.

Deviations from the original design notes (deliberate, kept minimal):

- The tray status icons are rendered in code as RGBA circles instead of
  loading PNG assets; `assets/` carries only `app.ico` (regenerable via
  `assets/make-icons.ps1`).
- The dark theme is a direct FLTK palette instead of the `fltk-theme` crate.

## Development

```powershell
cargo test            # unit + integration tests (uses loopback + test policy flags)
cargo build --release
```

Test policy knobs (`DestinationPolicy { allow_ip_literals, allow_private_ips }`)
exist for the integration tests; production defaults are strict.

## Security notes

- The proxy listens on loopback only.
- Destination allowlist + public-IP-only policy mean a compromised browser
  pattern cannot reach LAN/internal addresses through this proxy.
- Logs record lifecycle, listener failures, config changes and refused
  destinations — not browsing histories or payloads.
- No telemetry, no updates, no web dashboard.
