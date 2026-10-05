// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Handle through which the broker core talks to one client connection.

use parking_lot::Mutex;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;

use crate::openwire::model::Command;

/// Something the connection writer task must do.
///
/// `Cmd` is kept inline on purpose: every dispatched message travels as an `Out`, and boxing
/// the command would add one heap allocation per message on the hot path. The channel stores
/// its slots in preallocated blocks, so the larger enum costs no extra allocation.
#[allow(clippy::large_enum_variant)]
pub enum Out {
    Cmd(Command),
    /// Send the commands, then close the socket.
    CloseAfter(Vec<Command>),
}

/// Information about a connection shown by the admin console.
#[derive(Debug, Clone, Default)]
pub struct ConnInfo {
    pub connection_id: String,
    pub client_id: String,
    pub user: String,
    pub version: i32,
    pub sessions: usize,
    pub consumers: usize,
    pub producers: usize,
}

pub struct ConnHandle {
    /// Broker-local numeric id.
    pub id: u64,
    pub remote: SocketAddr,
    pub connected_at: chrono::DateTime<chrono::Local>,
    tx: mpsc::UnboundedSender<Out>,
    pub info: Mutex<ConnInfo>,
    pub dispatched: AtomicU64,
}

impl ConnHandle {
    pub fn new(id: u64, remote: SocketAddr, tx: mpsc::UnboundedSender<Out>) -> Self {
        ConnHandle {
            id,
            remote,
            connected_at: chrono::Local::now(),
            tx,
            info: Mutex::new(ConnInfo::default()),
            dispatched: AtomicU64::new(0),
        }
    }

    /// Queues a command for the writer. Returns false if the connection is gone.
    pub fn send(&self, cmd: Command) -> bool {
        self.tx.send(Out::Cmd(cmd)).is_ok()
    }

    pub fn close_after(&self, cmds: Vec<Command>) {
        let _ = self.tx.send(Out::CloseAfter(cmds));
    }

    pub fn count_dispatch(&self) {
        self.dispatched.fetch_add(1, Ordering::Relaxed);
    }
}
