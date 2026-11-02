//! SOCKS5 listener: accept loop, concurrency limits, destination policy and
//! session supervision.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use super::matcher::{self, DomainPolicy};
use super::relay;
use super::socks::{self, Address, Reply, SocksRequest};

/// Destination policy knobs. Defaults are restrictive (fail closed); the
/// relaxed options exist for integration tests and diagnostics.
#[derive(Debug, Clone, Copy)]
pub struct DestinationPolicy {
    /// Accept SOCKS5 requests addressed to literal IPs.
    pub allow_ip_literals: bool,
    /// Allow connecting to private/loopback/reserved addresses.
    pub allow_private_ips: bool,
}

impl Default for DestinationPolicy {
    fn default() -> Self {
        Self {
            allow_ip_literals: false,
            allow_private_ips: false,
        }
    }
}

/// The domain policy shared between the controller (writer) and sessions
/// (readers). Sessions clone the inner `Arc` per request, so updates are
/// picked up by new requests without a listener restart.
pub type SharedDomains = Arc<tokio::sync::RwLock<Arc<DomainPolicy>>>;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    pub max_connections: usize,
    pub connect_timeout: Duration,
    pub handshake_timeout: Duration,
    pub policy: DestinationPolicy,
}

/// Transport-level counters. Fields are `Arc` so the relay can increment the
/// byte counters directly from its session task.
#[derive(Debug, Default)]
pub struct ServerStats {
    pub active_connections: Arc<AtomicI64>,
    /// Bytes sent from the client towards the destination.
    pub tx_bytes: Arc<AtomicU64>,
    /// Bytes received from the destination towards the client.
    pub rx_bytes: Arc<AtomicU64>,
    /// Requests rejected by policy or the protocol.
    pub rejected: Arc<AtomicU64>,
    /// Outbound connection attempts that failed.
    pub failed_connections: Arc<AtomicU64>,
    /// Unix milliseconds of the most recent failure (0 = none).
    pub last_failure_ms: Arc<AtomicU64>,
}

impl ServerStats {
    pub fn note_failure(&self) {
        self.failed_connections.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.last_failure_ms.store(now, Ordering::Relaxed);
    }
}

/// A running proxy listener. Dropping the value does not stop the server;
/// call [`ProxyServer::shutdown`] for a coordinated stop.
pub struct ProxyServer {
    pub local_addr: SocketAddr,
    pub stats: Arc<ServerStats>,
    pub stop: CancellationToken,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ProxyServer {
    /// Binds the listener and spawns the accept loop. Returns once the
    /// listener is bound (or has failed), so callers can report bind errors
    /// before proceeding.
    pub async fn start(
        cfg: ServerConfig,
        domains: SharedDomains,
        enabled: watch::Receiver<bool>,
    ) -> std::io::Result<Arc<ProxyServer>> {
        let listener = TcpListener::bind(cfg.listen).await?;
        let local_addr = listener.local_addr()?;
        info!(%local_addr, "SOCKS5 listener bound");

        let stats = Arc::new(ServerStats::default());
        // Sessions started before a pause share this token; pausing cancels
        // it, resuming installs a fresh one.
        let epoch = Arc::new(tokio::sync::RwLock::new(CancellationToken::new()));
        let semaphore = Arc::new(Semaphore::new(cfg.max_connections.max(1)));
        let stop = CancellationToken::new();

        let server = Arc::new(ProxyServer {
            local_addr,
            stats: stats.clone(),
            stop: stop.clone(),
            task: Mutex::new(None),
        });

        let task = tokio::spawn(accept_loop(
            listener,
            cfg,
            domains,
            stats,
            epoch,
            semaphore,
            stop,
            enabled,
        ));
        *server.task.lock().await = Some(task);
        Ok(server)
    }

    pub async fn shutdown(&self) {
        self.stop.cancel();
        if let Some(task) = self.task.lock().await.take() {
            let _ = task.await;
        }
        info!("SOCKS5 listener stopped");
    }
}

async fn apply_enabled_change(
    enabled: &mut watch::Receiver<bool>,
    epoch: &Arc<tokio::sync::RwLock<CancellationToken>>,
) {
    if *enabled.borrow() {
        *epoch.write().await = CancellationToken::new();
        info!("proxy resumed");
    } else {
        epoch.read().await.cancel();
        info!("proxy paused; closing active sessions");
    }
}

#[allow(clippy::too_many_arguments)]
async fn accept_loop(
    listener: TcpListener,
    cfg: ServerConfig,
    domains: SharedDomains,
    stats: Arc<ServerStats>,
    epoch: Arc<tokio::sync::RwLock<CancellationToken>>,
    semaphore: Arc<Semaphore>,
    stop: CancellationToken,
    mut enabled: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = stop.cancelled() => break,
            changed = enabled.changed() => {
                if changed.is_err() {
                    break; // controller gone
                }
                apply_enabled_change(&mut enabled, &epoch).await;
            }
            // Acquire a permit before accepting so pending handshakes are
            // bounded by the same limit as active sessions.
            permit = semaphore.clone().acquire_owned() => {
                let permit = match permit {
                    Ok(p) => p,
                    Err(_) => break,
                };
                tokio::select! {
                    _ = stop.cancelled() => break,
                    // While parked on accept with a permit, pause/resume must
                    // stay selectable so active sessions are closed promptly.
                    changed = enabled.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        apply_enabled_change(&mut enabled, &epoch).await;
                    }
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((socket, _peer)) => {
                                let session = Session {
                                    cfg: cfg.clone(),
                                    domains: domains.clone(),
                                    stats: stats.clone(),
                                    stop: stop.clone(),
                                    epoch: epoch.read().await.clone(),
                                    enabled: enabled.clone(),
                                };
                                tokio::spawn(session.run(socket, permit));
                            }
                            Err(e) => {
                                warn!(error = %e, "accept failed");
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                    }
                }
            }
        }
    }
}

struct ActiveGuard {
    stats: Arc<ServerStats>,
}

impl ActiveGuard {
    fn new(stats: &Arc<ServerStats>) -> Self {
        stats.active_connections.fetch_add(1, Ordering::Relaxed);
        Self { stats: stats.clone() }
    }
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.stats.active_connections.fetch_sub(1, Ordering::Relaxed);
    }
}

struct Session {
    cfg: ServerConfig,
    domains: SharedDomains,
    stats: Arc<ServerStats>,
    /// Server-wide stop; cancels every session on coordinated shutdown.
    stop: CancellationToken,
    /// Cancellation for the pause epoch this session was accepted in.
    epoch: CancellationToken,
    enabled: watch::Receiver<bool>,
}

impl Session {
    async fn run(self, socket: TcpStream, permit: OwnedSemaphorePermit) {
        // The permit is held for the whole session (handshake + relay), which
        // bounds concurrent sessions and pending handshakes to
        // `max_connections`.
        let _permit = permit;
        let _active = ActiveGuard::new(&self.stats);
        let mut socket = socket;
        // A session accepted while paused holds an already-cancelled epoch;
        // its handshake must still run so the client receives a clean
        // method-negotiation failure instead of a dropped connection.
        let accepted_while_paused = self.epoch.is_cancelled();
        tokio::select! {
            _ = self.stop.cancelled() => {
                // Coordinated shutdown: drop the sockets.
            }
            _ = self.epoch.cancelled(), if !accepted_while_paused => {
                // Paused or restarting: fail closed.
                self.stats.rejected.fetch_add(1, Ordering::Relaxed);
            }
            _ = self.handle(&mut socket) => {}
        }
    }

    async fn handle(&self, socket: &mut TcpStream) {
        socket.set_nodelay(true).ok();
        let outcome = tokio::time::timeout(self.cfg.handshake_timeout, self.handshake(socket)).await;
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(_rejected)) => {} // handshake already replied and counted
            Err(_) => {
                debug!("handshake timeout");
                self.stats.rejected.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Performs the handshake + relay. `Ok(())` means the session ended
    /// normally; `Err(())` marks a rejected request (already accounted).
    async fn handshake(&self, socket: &mut TcpStream) -> Result<(), ()> {
        let methods = match socks::read_greeting(socket).await {
            Ok(m) => m,
            Err(e) => {
                debug!(error = %e, "malformed greeting");
                self.stats.rejected.fetch_add(1, Ordering::Relaxed);
                return Err(());
            }
        };
        if !methods.contains(&socks::NO_AUTHENTICATION) {
            let _ = socket.write_all(&socks::method_reply(socks::NO_ACCEPTABLE_METHODS)).await;
            self.stats.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(());
        }
        // Fail closed while paused: negotiate no acceptable method.
        if !*self.enabled.borrow() {
            let _ = socket.write_all(&socks::method_reply(socks::NO_ACCEPTABLE_METHODS)).await;
            self.stats.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(());
        }
        let _ = socket.write_all(&socks::method_reply(socks::NO_AUTHENTICATION)).await;

        let request = match socks::read_request(socket).await {
            Ok(r) => r,
            Err(e) => {
                if let Some(reply) = e.sendable_reply() {
                    let _ = socket.write_all(&socks::error_reply(reply)).await;
                }
                debug!(error = %e, "malformed request");
                self.stats.rejected.fetch_add(1, Ordering::Relaxed);
                return Err(());
            }
        };

        match self.connect_destination(&request).await {
            Ok(mut remote) => {
                let _ = socket.write_all(&socks::success_reply()).await;
                let dest = destination_label(&request);
                info!(%dest, "session established");
                match relay::relay(
                    socket,
                    &mut remote,
                    self.stats.tx_bytes.clone(),
                    self.stats.rx_bytes.clone(),
                )
                .await
                {
                    Ok((up, down)) => debug!(%dest, up, down, "session closed"),
                    Err(e) => debug!(%dest, error = %e, "relay error"),
                }
            }
            Err(reply) => {
                let _ = socket.write_all(&socks::error_reply(reply)).await;
                self.stats.note_failure();
                warn!(dest = %destination_label(&request), reply = reply as u8, "destination refused");
            }
        }
        Ok(())
    }

    /// Validates the destination against policy, resolves it and establishes
    /// the outbound connection.
    async fn connect_destination(&self, request: &SocksRequest) -> Result<TcpStream, Reply> {
        // Resolve through system DNS, then validate every candidate against
        // the address policy.
        let candidates: Vec<SocketAddr> = match &request.address {
            Address::Domain(host) => {
                let policy = self.domains.read().await.clone();
                if !policy.allows(host) {
                    debug!(host, restrict = policy.restrict, "destination rejected by domain filter");
                    return Err(Reply::NotAllowed);
                }
                let addrs = match tokio::net::lookup_host((host.as_str(), request.port)).await {
                    Ok(addrs) => addrs.collect::<Vec<SocketAddr>>(),
                    Err(e) => {
                        warn!(host, error = %e, "DNS resolution failed");
                        return Err(Reply::HostUnreachable);
                    }
                };
                let permitted: Vec<SocketAddr> =
                    addrs.into_iter().filter(|a| self.ip_allowed(a.ip())).collect();
                if permitted.is_empty() {
                    warn!(host, "no permitted (public) address in DNS response");
                    return Err(Reply::NotAllowed);
                }
                permitted
            }
            Address::Ip(ip) => {
                if !self.cfg.policy.allow_ip_literals {
                    return Err(Reply::NotAllowed);
                }
                vec![SocketAddr::new(*ip, request.port)]
            }
        };

        // Try each validated address in order within the overall connect
        // budget — connecting to a resolved IP directly, no second lookup.
        // The budget is split across the remaining candidates so a hanging
        // address (e.g. silently-dropped IPv6 loopback) cannot consume the
        // whole budget before the next address is tried.
        let deadline = std::time::Instant::now() + self.cfg.connect_timeout;
        let mut candidates_left = candidates.len();
        let mut last = Reply::GeneralFailure;
        for addr in candidates {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                warn!(dest = %addr, "connect budget exhausted");
                return Err(Reply::TtlExpired);
            }
            let slice = remaining / candidates_left as u32;
            candidates_left -= 1;
            match tokio::time::timeout(slice, TcpStream::connect(addr)).await {
                Ok(Ok(stream)) => {
                    stream.set_nodelay(true).ok();
                    return Ok(stream);
                }
                Ok(Err(e)) => {
                    warn!(dest = %addr, error = %e, "connect failed");
                    last = socks::io_error_reply(&e);
                }
                Err(_) => {
                    warn!(dest = %addr, "connect timeout");
                    last = Reply::TtlExpired;
                }
            }
        }
        Err(last)
    }

    fn ip_allowed(&self, ip: std::net::IpAddr) -> bool {
        self.cfg.policy.allow_private_ips || matcher::is_public_ip(ip)
    }
}

fn destination_label(request: &SocksRequest) -> String {
    match &request.address {
        Address::Domain(h) => format!("{h}:{}", request.port),
        Address::Ip(ip) => format!("{ip}:{}", request.port),
    }
}
