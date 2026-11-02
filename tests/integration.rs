//! End-to-end tests: SOCKS5 sessions against a live proxy server on
//! loopback, plus headless controller lifecycle tests.
//!
//! The proxy's default policy rejects loopback/private destinations, so these
//! tests enable the test-only `allow_private_ips` / `allow_ip_literals`
//! policy knobs to route through local echo servers.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use escape_proxy::app::commands::AppCommand;
use escape_proxy::app::config::{self, Config};
use escape_proxy::app::controller;
use escape_proxy::app::state::{ProxyState, SharedAppState, StatusSnapshot};
use escape_proxy::core::matcher::DomainList;
use escape_proxy::core::server::{
    DestinationPolicy, ProxyServer, ServerConfig, SharedDomains,
};

// ---------------------------------------------------------------- helpers

struct TestProxy {
    server: Arc<ProxyServer>,
    addr: SocketAddr,
    enabled_tx: tokio::sync::watch::Sender<bool>,
    domains: SharedDomains,
}

fn test_config(listen: SocketAddr, max_connections: usize) -> ServerConfig {
    ServerConfig {
        listen,
        max_connections,
        connect_timeout: Duration::from_secs(2),
        handshake_timeout: Duration::from_secs(5),
        policy: DestinationPolicy {
            allow_ip_literals: true,
            allow_private_ips: true,
        },
    }
}

async fn start_proxy(domains: &[&str], max_connections: usize) -> TestProxy {
    let shared: SharedDomains = Arc::new(tokio::sync::RwLock::new(Arc::new(
        DomainList::new(domains).unwrap(),
    )));
    let (enabled_tx, enabled_rx) = tokio::sync::watch::channel(true);
    let server = ProxyServer::start(
        test_config("127.0.0.1:0".parse().unwrap(), max_connections),
        shared.clone(),
        enabled_rx,
    )
    .await
    .unwrap();
    TestProxy {
        addr: server.local_addr,
        server,
        enabled_tx,
        domains: shared,
    }
}

/// Echo server: mirrors bytes and appends `<END>` once the client half-closes.
async fn spawn_echo() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let (mut r, mut w) = stream.into_split();
                let mut buf = [0u8; 8192];
                loop {
                    match r.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if w.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                let _ = w.write_all(b"<END>").await;
                let _ = w.shutdown().await;
            });
        }
    });
    addr
}

/// Performs a SOCKS5 handshake for `host` against the proxy.
async fn socks_connect(proxy: SocketAddr, host: &str, port: u16) -> std::io::Result<(TcpStream, [u8; 10])> {
    let mut stream = TcpStream::connect(proxy).await?;
    stream.write_all(&[5, 1, 0]).await?;
    let mut method = [0u8; 2];
    stream.read_exact(&mut method).await?;
    assert_eq!(method, [5, 0], "method negotiation failed");
    let mut req = vec![5, 1, 0, 3, host.len() as u8];
    req.extend_from_slice(host.as_bytes());
    req.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&req).await?;
    let mut reply = [0u8; 10];
    stream.read_exact(&mut reply).await?;
    Ok((stream, reply))
}

async fn socks_connect_ip(
    proxy: SocketAddr,
    ip: std::net::IpAddr,
    port: u16,
) -> std::io::Result<(TcpStream, [u8; 10])> {
    let mut stream = TcpStream::connect(proxy).await?;
    stream.write_all(&[5, 1, 0]).await?;
    let mut method = [0u8; 2];
    stream.read_exact(&mut method).await?;
    let mut req = vec![5, 1, 0, 1];
    match ip {
        std::net::IpAddr::V4(v4) => req.extend_from_slice(&v4.octets()),
        std::net::IpAddr::V6(v6) => {
            req[3] = 4;
            req.extend_from_slice(&v6.octets());
        }
    }
    req.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&req).await?;
    let mut reply = [0u8; 10];
    stream.read_exact(&mut reply).await?;
    Ok((stream, reply))
}

async fn eventually<F>(mut predicate: F)
where
    F: FnMut() -> bool,
{
    for _ in 0..200 {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not reached within 4 s");
}

// ---------------------------------------------------------------- sessions

#[tokio::test]
async fn echo_roundtrip() {
    let echo = spawn_echo().await;
    let proxy = start_proxy(&["localhost"], 16).await;
    let (mut client, reply) = socks_connect(proxy.addr, "localhost", echo.port()).await.unwrap();
    assert_eq!(reply[1], 0, "connect should succeed");
    client.write_all(b"hello proxy").await.unwrap();
    let mut buf = vec![0u8; 64];
    let n = client.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"hello proxy");
    proxy.server.shutdown().await;
}

#[tokio::test]
async fn large_transfer() {
    let echo = spawn_echo().await;
    let proxy = start_proxy(&["localhost"], 16).await;
    let (client, _) = socks_connect(proxy.addr, "localhost", echo.port()).await.unwrap();

    // 2 MiB pattern both ways.
    let payload: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let sent = payload.clone();
    let writer = tokio::spawn(async move {
        let mut stream = client;
        stream.write_all(&sent).await.unwrap();
        stream.shutdown().await.unwrap();
        stream
    });

    // Read everything back on the same socket after writing.
    let mut stream = writer.await.unwrap();
    let mut received = Vec::with_capacity(payload.len());
    stream.read_to_end(&mut received).await.unwrap();
    assert_eq!(received.len(), payload.len() + b"<END>".len());
    assert_eq!(&received[..payload.len()], &payload[..]);
    proxy.server.shutdown().await;
}

#[tokio::test]
async fn tcp_half_close_is_propagated() {
    let echo = spawn_echo().await;
    let proxy = start_proxy(&["localhost"], 16).await;
    let (mut client, _) = socks_connect(proxy.addr, "localhost", echo.port()).await.unwrap();
    client.write_all(b"ping").await.unwrap();
    client.shutdown().await.unwrap(); // half-close: still able to read
    let mut all = Vec::new();
    client.read_to_end(&mut all).await.unwrap();
    assert_eq!(all, b"ping<END>");
    proxy.server.shutdown().await;
}

#[tokio::test]
async fn unknown_domain_is_rejected_by_policy() {
    let echo = spawn_echo().await;
    let proxy = start_proxy(&["localhost"], 16).await;
    let (_, reply) = socks_connect(proxy.addr, "bad.example", echo.port()).await.unwrap();
    assert_eq!(reply[1], 2, "not allowed");
    proxy.server.shutdown().await;
}

#[tokio::test]
async fn ip_literals_rejected_by_default_policy() {
    let echo = spawn_echo().await;
    let shared: SharedDomains = Arc::new(tokio::sync::RwLock::new(Arc::new(
        DomainList::new(["localhost"]).unwrap(),
    )));
    let (_tx, rx) = tokio::sync::watch::channel(true);
    let mut cfg = test_config("127.0.0.1:0".parse().unwrap(), 16);
    cfg.policy = DestinationPolicy::default(); // strict: no literals, no private
    let server = ProxyServer::start(cfg, shared, rx).await.unwrap();
    let (_, reply) =
        socks_connect_ip(server.local_addr, "127.0.0.1".parse().unwrap(), echo.port())
            .await
            .unwrap();
    assert_eq!(reply[1], 2, "IP literals must be rejected by default");
    server.shutdown().await;
}

#[tokio::test]
async fn allowlist_updates_apply_without_restart() {
    let echo = spawn_echo().await;
    let proxy = start_proxy(&["localhost"], 16).await;
    let (_, reply) = socks_connect(proxy.addr, "fresh.example", echo.port()).await.unwrap();
    assert_eq!(reply[1], 2, "not in allowlist");

    // Swap the allowlist under the running server.
    *proxy.domains.write().await = Arc::new(DomainList::new(["localhost", "fresh.example"]).unwrap());
    let (_, reply) = socks_connect(proxy.addr, "fresh.example", echo.port()).await.unwrap();
    // `fresh.example` does not resolve in system DNS: the allowlist passed,
    // so the failure must no longer be the policy code.
    assert_ne!(reply[1], 2, "allowlist update should be live");
    assert_eq!(reply[1], 4, "expected DNS/host-unreachable failure instead");
    proxy.server.shutdown().await;
}

// ---------------------------------------------------------------- limits

#[tokio::test]
async fn connection_capacity_is_enforced_and_frees_up() {
    let echo = spawn_echo().await;
    let proxy = start_proxy(&["localhost"], 2).await;

    // Two idle sessions hold both permits.
    let (_c1, r1) = socks_connect(proxy.addr, "localhost", echo.port()).await.unwrap();
    assert_eq!(r1[1], 0);
    let (_c2, r2) = socks_connect(proxy.addr, "localhost", echo.port()).await.unwrap();
    assert_eq!(r2[1], 0);

    // A third client connects (OS backlog) but must not get a method reply
    // while no permit is free.
    let mut c3 = TcpStream::connect(proxy.addr).await.unwrap();
    c3.write_all(&[5, 1, 0]).await.unwrap();
    let slow = tokio::time::timeout(Duration::from_millis(300), c3.read_exact(&mut [0u8; 2])).await;
    assert!(slow.is_err(), "third handshake must be pending, not served");

    // Freeing one session lets the pending handshake through.
    drop(_c2);
    let got = tokio::time::timeout(Duration::from_secs(2), c3.read_exact(&mut [0u8; 2])).await;
    assert!(got.is_ok(), "permit released should unblock pending handshake");
    proxy.server.shutdown().await;
}

#[tokio::test]
async fn pausing_rejects_new_sessions_and_closes_relays() {
    let echo = spawn_echo().await;
    let proxy = start_proxy(&["localhost"], 16).await;
    let (mut client, _) = socks_connect(proxy.addr, "localhost", echo.port()).await.unwrap();
    client.write_all(b"hi").await.unwrap();

    proxy.enabled_tx.send(false).unwrap();
    // Established relay must be closed. The echo of "hi" may still arrive
    // first; keep reading until EOF (or a Windows reset — aborting the
    // socket is equally "closed" for fail-closed purposes).
    let mut buf = [0u8; 16];
    let mut total = 0usize;
    loop {
        let result = tokio::time::timeout(Duration::from_secs(2), client.read(&mut buf))
            .await
            .expect("read should finish");
        match result {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                assert!(total <= 2, "unexpected data after pause");
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::NotConnected
                        | std::io::ErrorKind::BrokenPipe
                ) =>
            {
                break;
            }
            Err(e) => panic!("unexpected read error after pause: {e}"),
        }
    }

    // New connections must fail closed: no acceptable methods.
    let mut c2 = TcpStream::connect(proxy.addr).await.unwrap();
    c2.write_all(&[5, 1, 0]).await.unwrap();
    let mut method = [0u8; 2];
    c2.read_exact(&mut method).await.unwrap();
    assert_eq!(method, [5, 0xFF]);

    // Resuming accepts again.
    proxy.enabled_tx.send(true).unwrap();
    eventually(|| true).await;
    let (c3, r3) = socks_connect(proxy.addr, "localhost", echo.port()).await.unwrap();
    assert_eq!(r3[1], 0);
    drop(c3);
    proxy.server.shutdown().await;
}

#[tokio::test]
async fn shutdown_closes_listener_and_sessions() {
    let echo = spawn_echo().await;
    let proxy = start_proxy(&["localhost"], 16).await;
    let addr = proxy.addr;
    let (mut client, _) = socks_connect(addr, "localhost", echo.port()).await.unwrap();
    proxy.server.shutdown().await;
    let mut buf = [0u8; 8];
    let n = tokio::time::timeout(Duration::from_secs(2), client.read(&mut buf))
        .await
        .expect("read should finish")
        .unwrap_or(0);
    assert_eq!(n, 0, "session should be closed after shutdown");
    // Listener is gone too.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(TcpStream::connect(addr).await.is_err());
}

// ---------------------------------------------------------------- controller

fn controller_snapshot() -> StatusSnapshot {
    StatusSnapshot {
        state: ProxyState::Starting,
        listen_addr: "127.0.0.1:0".to_string(),
        active_connections: 0,
        tx_bytes: 0,
        rx_bytes: 0,
        domains: Vec::new(),
        autostart: false,
        show_on_startup: false,
        max_connections: 128,
        connect_timeout_ms: 5000,
        last_failure_ms: 0,
        last_error: None,
    }
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "escape-proxy-test-{}-{}",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    l.local_addr().unwrap().port()
}

#[tokio::test]
async fn controller_lifecycle_headless() {
    let dir = temp_dir("controller");
    let path = config::config_path(&dir);

    let port = free_port().await;
    let mut initial = Config::default();
    initial.listen_addr = format!("127.0.0.1:{port}");
    initial.domains = vec!["Example.COM".to_string()];
    config::save(&path, &initial).unwrap();

    let state = SharedAppState::new(controller_snapshot());
    let cmd_tx = controller::start(path.clone(), state.clone()).await;

    eventually(|| matches!(state.live_snapshot().state, ProxyState::Running)).await;
    let snap = state.live_snapshot();
    assert_eq!(snap.listen_addr, format!("127.0.0.1:{port}"));
    assert_eq!(snap.domains, vec!["example.com".to_string()]);

    // Pause: state transitions, relays close, config persisted.
    cmd_tx.send(AppCommand::SetEnabled(false)).await.unwrap();
    eventually(|| matches!(state.live_snapshot().state, ProxyState::Paused)).await;
    let reloaded: Config = config::load(&path).unwrap();
    assert!(!reloaded.enabled);

    // Resume.
    cmd_tx.send(AppCommand::SetEnabled(true)).await.unwrap();
    eventually(|| matches!(state.live_snapshot().state, ProxyState::Running)).await;

    // Domain update (normalised) applies live.
    cmd_tx
        .send(AppCommand::UpdateDomains(vec!["a.example".into(), "B.Example".into()]))
        .await
        .unwrap();
    eventually(|| state.live_snapshot().domains == vec!["a.example".to_string(), "b.example".to_string()])
        .await;

    // Invalid domain list is rejected and reported.
    cmd_tx
        .send(AppCommand::UpdateDomains(vec!["not a domain".into()]))
        .await
        .unwrap();
    eventually(|| state.live_snapshot().last_error.is_some()).await;
    assert_eq!(
        state.live_snapshot().domains,
        vec!["a.example".to_string(), "b.example".to_string()]
    );

    // Advanced apply rebinds the listener and persists.
    let port2 = free_port().await;
    cmd_tx
        .send(AppCommand::ApplyAdvanced {
            port: port2,
            max_connections: 64,
            connect_timeout_ms: 2500,
        })
        .await
        .unwrap();
    eventually(|| state.live_snapshot().listen_addr == format!("127.0.0.1:{port2}")).await;
    let reloaded: Config = config::load(&path).unwrap();
    assert_eq!(reloaded.max_connections, 64);
    assert_eq!(reloaded.connect_timeout_ms, 2500);

    // Invalid advanced change keeps the previous listener and reports an error.
    assert!(TcpListener::bind(("127.0.0.1", port2)).await.is_err(), "old port still bound");
    cmd_tx
        .send(AppCommand::ApplyAdvanced {
            port: port2,
            max_connections: 0, // invalid
            connect_timeout_ms: 2500,
        })
        .await
        .unwrap();
    eventually(|| {
        let s = state.live_snapshot();
        s.listen_addr == format!("127.0.0.1:{port2}") && s.last_error.is_some()
    })
    .await;
    assert!(
        TcpListener::bind(("127.0.0.1", port2)).await.is_err(),
        "previous listener must survive a rejected change"
    );

    // Show-on-startup persists.
    cmd_tx.send(AppCommand::SetShowOnStartup(true)).await.unwrap();
    eventually(|| state.live_snapshot().show_on_startup).await;
    assert!(config::load(&path).unwrap().ui.show_on_startup);

    // Shutdown releases the listener.
    cmd_tx.send(AppCommand::Shutdown).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        TcpListener::bind(("127.0.0.1", port2)).await.is_ok(),
        "listener should be released after shutdown"
    );
}

// ---------------------------------------------------------------- config

#[test]
fn config_validation_and_roundtrip() {
    let mut cfg = Config::default();
    cfg.domains = vec!["google.com".into(), "GOOGLE.com".into()];
    cfg.listen_addr = "127.0.0.1:1234".into();
    let validated = cfg.validate().unwrap();
    assert_eq!(validated.listen.port(), 1234);
    assert_eq!(validated.domains, vec!["google.com".to_string()]);

    let dir = temp_dir("config");
    let path = config::config_path(&dir);
    config::save(&path, &cfg).unwrap();
    let loaded = config::load(&path).unwrap();
    assert_eq!(loaded.listen_addr, "127.0.0.1:1234");
    assert_eq!(loaded.domains.len(), 2);

    // Missing file -> defaults; corrupt file -> error.
    std::fs::remove_file(&path).unwrap();
    assert!(config::load(&path).unwrap().enabled);
    std::fs::write(&path, "not [ valid toml").unwrap();
    assert!(config::load(&path).is_err());
}

#[test]
fn config_rejects_bad_values() {
    let mut cfg = Config::default();

    cfg.listen_addr = "0.0.0.0:10808".into();
    assert!(cfg.validate().is_err(), "bind IP must be fixed at 127.0.0.1");

    cfg.listen_addr = "garbage".into();
    assert!(cfg.validate().is_err());

    cfg.listen_addr = "127.0.0.1:10808".into();
    cfg.max_connections = 0;
    assert!(cfg.validate().is_err());
    cfg.max_connections = 5000;
    assert!(cfg.validate().is_err());

    cfg.max_connections = 128;
    cfg.connect_timeout_ms = 10;
    assert!(cfg.validate().is_err());

    cfg.connect_timeout_ms = 5000;
    cfg.domains = vec!["192.168.1.1".into()];
    assert!(cfg.validate().is_err(), "IP literals are not domains");
}
