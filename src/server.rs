// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Broker startup, listeners and graceful shutdown.

use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::broker::Broker;
use crate::config::{Config, ConfigSource, Secret, User, DEFAULT_PASSWORD, DEFAULT_USER};
use crate::openwire::types::MAX_VERSION;
use crate::openwire::wireformat::{PROVIDER_NAME, PROVIDER_VERSION};

/// Reads and checks everything the configuration points at, before anything is started:
/// the ban list and the TLS certificate. The returned warnings (a certificate close to its
/// expiry) are worth reporting but stop nothing; the error is a configuration error.
pub fn preflight(cfg: &Config) -> Result<Vec<String>, String> {
    crate::bans::load(&cfg.bans_path).map_err(|e| format!("configuration error: {e}"))?;
    if cfg.tls_enabled {
        return Ok(crate::tls::prepare(cfg)?.warnings);
    }
    Ok(Vec::new())
}

/// Signals that ask the broker to stop.
pub type StopSignal = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

/// Runs the broker until `stop` resolves. `on_ready` is called once the OpenWire port is bound.
pub async fn run(cfg: Config, stop: StopSignal, on_ready: impl FnOnce()) -> Result<(), String> {
    tracing::info!("{PROVIDER_NAME} {PROVIDER_VERSION} starting");
    startup_messages(&cfg);

    let cfg = Arc::new(cfg);
    let plain = if cfg.plain {
        let addr = std::net::SocketAddr::new(cfg.bind, cfg.port);
        let listener = listen(addr, cfg.socket_buffer_bytes).map_err(|e| format!("cannot listen on {addr}: {e}"))?;
        tracing::info!("OpenWire listening on {addr} (max version {MAX_VERSION})");
        Some(listener)
    } else {
        tracing::info!("the plain OpenWire listener is off (broker.plain = false)");
        None
    };
    let tls = if cfg.tls_enabled {
        let prepared = crate::tls::prepare(&cfg)?;
        for warning in &prepared.warnings {
            tracing::warn!("{warning}");
        }
        let addr = std::net::SocketAddr::new(cfg.tls_bind, cfg.tls_port);
        let listener = listen(addr, cfg.socket_buffer_bytes).map_err(|e| format!("cannot listen on {addr}: {e}"))?;
        tracing::info!(
            "TLS listening on {addr} (TLS {}+, client certificates: {})",
            cfg.tls_min_version,
            cfg.tls_client_auth
        );
        Some((listener, tokio_rustls::TlsAcceptor::from(prepared.config)))
    } else {
        None
    };

    let broker = Broker::new(cfg.clone());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    crate::admin::start(broker.clone(), shutdown_rx.clone()).await;

    tokio::spawn(broker.clone().housekeeping(shutdown_rx.clone()));
    tracing::info!("ready");
    on_ready();

    let mut accepting = Vec::new();
    if let Some(listener) = plain {
        let accept_broker = broker.clone();
        let accept_shutdown = shutdown_rx.clone();
        accepting.push(tokio::spawn(async move {
            let mut stop = accept_shutdown.clone();
            loop {
                tokio::select! {
                    res = listener.accept() => match res {
                        Ok((stream, remote)) => {
                            // Before any byte is read: a banned address costs one lookup.
                            if accept_broker.bans.refuse(remote.ip()) {
                                drop(stream);
                                continue;
                            }
                            tokio::spawn(crate::connection::serve(stream, remote, accept_broker.clone(), accept_shutdown.clone()));
                        }
                        Err(e) => {
                            tracing::warn!("accept failed: {e}");
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    },
                    _ = stop.changed() => break,
                }
            }
        }));
    }
    if let Some((listener, acceptor)) = tls {
        let accept_broker = broker.clone();
        let accept_shutdown = shutdown_rx.clone();
        let failures = Arc::new(HandshakeFailures::default());
        accepting.push(tokio::spawn(async move {
            let mut stop = accept_shutdown.clone();
            loop {
                tokio::select! {
                    res = listener.accept() => match res {
                        Ok((stream, remote)) => {
                            if accept_broker.bans.refuse(remote.ip()) {
                                drop(stream);
                                continue;
                            }
                            let _ = stream.set_nodelay(true);
                            // The handshake runs in the connection's own task: a slow or
                            // silent client never holds up the accept loop.
                            tokio::spawn(handshake(
                                stream,
                                remote,
                                acceptor.clone(),
                                accept_broker.clone(),
                                accept_shutdown.clone(),
                                failures.clone(),
                            ));
                        }
                        Err(e) => {
                            tracing::warn!("accept failed: {e}");
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    },
                    _ = stop.changed() => break,
                }
            }
        }));
    }

    stop.await;
    tracing::info!("shutting down");
    let _ = shutdown_tx.send(true);
    for task in accepting {
        let _ = task.await;
    }
    // Wait at most 5 seconds for connections to close.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !broker.connections().is_empty() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let discarded = broker.message_count();
    tracing::info!("stopped; {discarded} in-memory messages discarded");
    Ok(())
}

/// Logs where the configuration comes from, the console address and users, and how to fix
/// default or plain-text credentials.
fn startup_messages(cfg: &Config) {
    use std::io::IsTerminal;
    match &cfg.source {
        ConfigSource::File(p) => tracing::info!("configuration: {}", p.display()),
        ConfigSource::Defaults => {
            tracing::info!("configuration: built-in defaults (no mqrust.toml found)")
        }
    }
    let console = std::net::SocketAddr::new(cfg.admin_bind, cfg.admin_port);
    tracing::info!(
        "admin console on http://{console} (login with the [admin] user {})",
        cfg.admin_user.username
    );
    let n = cfg.users.len();
    let anonymous = if cfg.allow_anonymous {
        ", anonymous access allowed"
    } else {
        ""
    };
    tracing::info!("{n} messaging user{}{anonymous}", if n == 1 { "" } else { "s" });

    let is_default =
        |u: &User| u.username == DEFAULT_USER && matches!(&u.secret, Secret::Plain(p) if p == DEFAULT_PASSWORD);
    if cfg.default_credentials {
        tracing::warn!(
            "default credentials admin/admin in use for the admin console and the messaging clients: \
             run `mqrust.exe set-admin` (console user) and `mqrust.exe user add <name>` (messaging users)"
        );
        if cfg.source == ConfigSource::Defaults && std::io::stdout().is_terminal() {
            println!("hint: run `mqrust.exe init-config` to create a commented mqrust.toml next to the executable");
        }
        return;
    }
    if is_default(&cfg.admin_user) {
        tracing::warn!("the admin console still uses admin/admin: run `mqrust.exe set-admin`");
    }
    if cfg.users.iter().any(is_default) {
        tracing::warn!(
            "messaging user admin/admin still configured: run `mqrust.exe user add <name>`, \
             then `mqrust.exe user remove admin`"
        );
    }
    if cfg.plain_text_in_use() {
        tracing::warn!(
            "plain-text passwords in the configuration: replace them with `mqrust.exe set-admin` \
             and `mqrust.exe user passwd <name>`"
        );
    }
}

/// A TLS handshake must finish within this long, or the connection is dropped.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Failed handshakes since the last warning, so a port scan costs one line a minute.
struct HandshakeFailures {
    count: std::sync::atomic::AtomicU64,
    last_warning: parking_lot::Mutex<std::time::Instant>,
}

impl Default for HandshakeFailures {
    fn default() -> Self {
        HandshakeFailures {
            count: std::sync::atomic::AtomicU64::new(0),
            last_warning: parking_lot::Mutex::new(std::time::Instant::now()),
        }
    }
}

impl HandshakeFailures {
    /// Counts one failure and returns the number to warn about, at most once a minute.
    fn failed(&self) -> Option<u64> {
        use std::sync::atomic::Ordering::Relaxed;
        self.count.fetch_add(1, Relaxed);
        let mut last = self.last_warning.lock();
        if last.elapsed() < Duration::from_secs(60) {
            return None;
        }
        *last = std::time::Instant::now();
        Some(self.count.swap(0, Relaxed))
    }
}

/// Runs the TLS handshake and, when it succeeds, serves the connection over it.
async fn handshake(
    stream: tokio::net::TcpStream,
    remote: std::net::SocketAddr,
    acceptor: tokio_rustls::TlsAcceptor,
    broker: Arc<Broker>,
    shutdown: watch::Receiver<bool>,
    failures: Arc<HandshakeFailures>,
) {
    let accepted = match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => {
            tracing::debug!("{remote}: TLS handshake failed: {e}");
            report(&failures);
            return;
        }
        Err(_) => {
            tracing::debug!("{remote}: TLS handshake did not finish within 10 s");
            report(&failures);
            return;
        }
    };
    let negotiated = crate::tls::negotiated(accepted.get_ref().1);
    crate::connection::serve_tls(accepted, remote, broker, shutdown, negotiated).await;
}

fn report(failures: &HandshakeFailures) {
    if let Some(count) = failures.failed() {
        tracing::warn!("{count} TLS handshake(s) failed in the last minute");
    }
}

/// Resolves on Ctrl+C, console close, logoff or system shutdown.
pub fn console_stop_signal() -> StopSignal {
    Box::pin(async {
        #[cfg(windows)]
        {
            use tokio::signal::windows;
            let mut c = windows::ctrl_c().expect("ctrl-c handler");
            let mut close = windows::ctrl_close().expect("ctrl-close handler");
            let mut shutdown = windows::ctrl_shutdown().expect("ctrl-shutdown handler");
            let mut logoff = windows::ctrl_logoff().expect("ctrl-logoff handler");
            tokio::select! {
                _ = c.recv() => {}
                _ = close.recv() => {}
                _ = shutdown.recv() => {}
                _ = logoff.recv() => {}
            }
        }
        #[cfg(not(windows))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
    })
}

/// Opens the OpenWire listener. Accepted sockets inherit its buffer sizes: with the small
/// Windows default, large dispatches stall between socket writes and consumers wait.
fn listen(addr: std::net::SocketAddr, buffer: u32) -> std::io::Result<TcpListener> {
    let socket = if addr.is_ipv4() {
        tokio::net::TcpSocket::new_v4()?
    } else {
        tokio::net::TcpSocket::new_v6()?
    };
    // On Unix (macOS) a restarted broker could not bind while connections of the previous
    // one are in TIME_WAIT; `TcpListener::bind` sets this option there too. Not on Windows,
    // where SO_REUSEADDR would let another process take over the port.
    #[cfg(not(windows))]
    socket.set_reuseaddr(true)?;
    if buffer > 0 {
        socket.set_send_buffer_size(buffer)?;
        socket.set_recv_buffer_size(buffer)?;
    }
    socket.bind(addr)?;
    socket.listen(1024)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{build, ConfigSource, FileConfig, Overrides};

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mqrust-preflight-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cfg_in(dir: &std::path::Path, f: impl FnOnce(&mut FileConfig)) -> Config {
        let mut fc = FileConfig::default();
        fc.broker.allow_anonymous = true;
        fc.admin.password = Some("admin".into());
        f(&mut fc);
        build(fc, ConfigSource::File(dir.join("mqrust.toml")), &Overrides::default()).unwrap()
    }

    #[test]
    fn preflight_passes_when_there_is_nothing_to_check() {
        let d = dir("empty");
        assert_eq!(preflight(&cfg_in(&d, |_| {})).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn preflight_refuses_a_broken_ban_file() {
        let d = dir("bans");
        std::fs::write(d.join("bans.toml"), "[[ban]]\naddress = \"10.1.0.0/40\"\n").unwrap();
        let e = preflight(&cfg_in(&d, |_| {})).unwrap_err();
        assert!(e.starts_with("configuration error"), "{e}");
        assert!(e.contains("bans.toml") && e.contains("10.1.0.0/40"), "{e}");
    }

    #[test]
    fn preflight_refuses_an_unreadable_certificate() {
        let d = dir("tls");
        let cfg = cfg_in(&d, |fc| {
            fc.tls.enabled = true;
            fc.tls.cert = "missing.crt".into();
            fc.tls.key = "missing.key".into();
        });
        let e = preflight(&cfg).unwrap_err();
        assert!(e.contains("tls.cert") && e.contains("missing.crt"), "{e}");
    }
}
