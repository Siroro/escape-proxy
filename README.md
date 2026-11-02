# escape-proxy

A lightweight Windows SOCKS5 proxy 

## Features

- SOCKS5 (RFC 1928): no-auth, TCP CONNECT only; BIND and UDP ASSOCIATE are
  rejected.
- Strict address policy either way: literal-IP requests are rejected,
  hostnames are resolved via system DNS, and only public routable addresses
  are connected (loopback/private/CGNAT/link-local/reserved ranges are
  refused).
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
restrict_domains = false
domains = [
    "google.com",
]

[ui]
show_on_startup = false
theme = "dark"
```

- The bind IP is fixed at `127.0.0.1`; only the port is configurable.
- `restrict_domains = false` (default): every valid hostname is forwarded.
  Set it to `true` (or toggle **Restrict to list** in the GUI) to allow only
  the entries in `domains`.
- Changes made in the settings window apply immediately and are saved
  atomically (write-temp-then-rename); there is no file watcher.
- Autostart state lives in the registry (`HKCU\...\Run`), not in the config;
  the GUI reflects the actual registered state.
- Set `ESCAPE_PROXY_HOME` to relocate config + logs (used by tests).


## Architecture

Four independent layers:

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

