// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Broker startup, listeners and graceful shutdown.

use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::broker::Broker;
use crate::config::{Config, ConfigSource};
use crate::openwire::types::MAX_VERSION;
use crate::openwire::wireformat::{PROVIDER_NAME, PROVIDER_VERSION};

/// Signals that ask the broker to stop.
pub type StopSignal = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

/// Runs the broker until `stop` resolves. `on_ready` is called once the OpenWire port is bound.
pub async fn run(cfg: Config, stop: StopSignal, on_ready: impl FnOnce()) -> Result<(), String> {
    tracing::info!("{PROVIDER_NAME} {PROVIDER_VERSION} starting");
    match &cfg.source {
        ConfigSource::File(p) => tracing::info!("config: {} ({} users)", p.display(), cfg.users.len()),
        ConfigSource::Defaults => tracing::info!("config: no file, using built-in defaults"),
    }
    if cfg.default_credentials {
        tracing::warn!("default credentials admin/admin in use: configure [[users]] and [admin] in mqrust.toml");
    } else if cfg.plain_text_in_use() {
        tracing::warn!("plain-text passwords in the configuration: use password_hash (mqrust.exe hash-password)");
    }

    let cfg = Arc::new(cfg);
    let addr = std::net::SocketAddr::new(cfg.bind, cfg.port);
    let listener = listen(addr, cfg.socket_buffer_bytes).map_err(|e| format!("cannot listen on {addr}: {e}"))?;
    tracing::info!("OpenWire listening on {addr} (max version {MAX_VERSION})");

    let broker = Broker::new(cfg.clone());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    crate::admin::start(broker.clone(), shutdown_rx.clone()).await;

    tokio::spawn(broker.clone().housekeeping(shutdown_rx.clone()));
    tracing::info!("ready");
    on_ready();

    let accept_broker = broker.clone();
    let accept_shutdown = shutdown_rx.clone();
    let accept = tokio::spawn(async move {
        let mut stop = accept_shutdown.clone();
        loop {
            tokio::select! {
                res = listener.accept() => match res {
                    Ok((stream, remote)) => {
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
    });

    stop.await;
    tracing::info!("shutting down");
    let _ = shutdown_tx.send(true);
    let _ = accept.await;
    // Wait at most 5 seconds for connections to close.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !broker.connections().is_empty() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let discarded = broker.message_count();
    tracing::info!("stopped; {discarded} in-memory messages discarded");
    Ok(())
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
    let socket = if addr.is_ipv4() { tokio::net::TcpSocket::new_v4()? } else { tokio::net::TcpSocket::new_v6()? };
    if buffer > 0 {
        socket.set_send_buffer_size(buffer)?;
        socket.set_recv_buffer_size(buffer)?;
    }
    socket.bind(addr)?;
    socket.listen(1024)
}
