// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! The broker core: destination registry, message intake, memory accounting and housekeeping.

pub mod compress;
pub mod conn;
pub mod destination;
pub mod entry;

use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::auth::Authenticator;
use crate::config::Config;
use crate::openwire::model::*;
use crate::openwire::props::{PrimitiveMap, Value};
use conn::ConnHandle;
use destination::{Dest, Effect};
use entry::{Entry, Memory, Meta, ENTRY_OVERHEAD};

pub const DLQ_NAME: &str = "ActiveMQ.DLQ";

const SHARDS: usize = 16;

fn shard_of(d: &Destination) -> usize {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    d.hash(&mut h);
    (h.finish() as usize) % SHARDS
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Why a message was not accepted.
#[derive(Debug, Clone)]
pub enum Rejection {
    /// Reported to a synchronous sender as an exception.
    Error { class: &'static str, message: String },
}

impl Rejection {
    pub fn throwable(&self) -> Throwable {
        match self {
            Rejection::Error { class, message } => Throwable::new(class, message.clone()),
        }
    }
}

/// Broker-wide counters shown by the admin console.
#[derive(Default)]
pub struct BrokerStats {
    pub messages_in: AtomicU64,
    pub dropped_async: AtomicU64,
    pub expired_on_arrival: AtomicU64,
    pub duplicates: AtomicU64,
}

pub struct Broker {
    pub cfg: Arc<Config>,
    pub memory: Arc<Memory>,
    pub auth: Authenticator,
    pub broker_id: BrokerId,
    pub started: chrono::DateTime<chrono::Local>,
    pub stats: BrokerStats,
    /// Destination registry, partitioned to keep lookups from contending.
    dests: Vec<RwLock<HashMap<Destination, Arc<Dest>>>>,
    conns: Mutex<HashMap<u64, Arc<ConnHandle>>>,
    next_conn: AtomicU64,
    seq: AtomicU64,
    limited: AtomicBool,
    dropped_while_limited: AtomicU64,
    id_counter: AtomicU64,
    advisory_subs: Mutex<Vec<AdvisorySub>>,
    advisory_seq: AtomicU64,
}

/// A client consumer on the temporary-destination advisory topics.
struct AdvisorySub {
    conn: Arc<ConnHandle>,
    consumer: ConsumerId,
    dest: Destination,
    queues: bool,
    topics: bool,
}

pub const ADVISORY_TEMP_QUEUE: &str = "ActiveMQ.Advisory.TempQueue";
pub const ADVISORY_TEMP_TOPIC: &str = "ActiveMQ.Advisory.TempTopic";

fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".into())
}

impl Broker {
    pub fn new(cfg: Arc<Config>) -> Arc<Broker> {
        let memory = Arc::new(Memory::new(cfg.max_memory_bytes));
        let auth = Authenticator::new(cfg.users.clone(), cfg.allow_anonymous);
        // Same shape as ActiveMQ's IdGenerator: ID:<host>-<port>-<timestamp>-<counter>:<n>
        let broker_id = BrokerId {
            value: Arc::from(format!("ID:{}-{}-{}-0:1", hostname(), cfg.port, now_ms())),
        };
        Arc::new(Broker {
            cfg,
            memory,
            auth,
            broker_id,
            started: chrono::Local::now(),
            stats: BrokerStats::default(),
            dests: (0..SHARDS).map(|_| RwLock::new(HashMap::new())).collect(),
            conns: Mutex::new(HashMap::new()),
            next_conn: AtomicU64::new(1),
            seq: AtomicU64::new(1),
            limited: AtomicBool::new(false),
            dropped_while_limited: AtomicU64::new(0),
            id_counter: AtomicU64::new(1),
            advisory_subs: Mutex::new(Vec::new()),
            advisory_seq: AtomicU64::new(1),
        })
    }

    /// A new broker-generated id string (`ID:<host>-<port>-<timestamp>-<n>:<m>`).
    pub fn generate_id(&self) -> String {
        let n = self.id_counter.fetch_add(1, Ordering::Relaxed);
        format!("ID:{}-{}-{}-1:{}", hostname(), self.cfg.port, self.started.timestamp_millis(), n)
    }

    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    // -- connections ----------------------------------------------------------

    pub fn new_conn_id(&self) -> u64 {
        self.next_conn.fetch_add(1, Ordering::Relaxed)
    }

    pub fn register_conn(&self, c: Arc<ConnHandle>) {
        self.conns.lock().insert(c.id, c);
    }

    pub fn unregister_conn(&self, id: u64) {
        self.conns.lock().remove(&id);
    }

    pub fn connections(&self) -> Vec<Arc<ConnHandle>> {
        let mut v: Vec<_> = self.conns.lock().values().cloned().collect();
        v.sort_by_key(|c| c.id);
        v
    }

    // -- destinations ---------------------------------------------------------

    pub fn get_dest(&self, d: &Destination) -> Option<Arc<Dest>> {
        self.dests[shard_of(d)].read().get(d).cloned()
    }

    /// Returns the destination, creating it on first use.
    pub fn get_or_create(&self, d: &Destination, owner: Option<u64>) -> Arc<Dest> {
        let shard = &self.dests[shard_of(d)];
        if let Some(x) = shard.read().get(d) {
            return x.clone();
        }
        let mut w = shard.write();
        w.entry(d.clone())
            .or_insert_with(|| {
                tracing::debug!("destination created: {d}");
                Arc::new(Dest::new(d.clone(), owner, self.cfg.topic_max_pending_per_consumer as usize))
            })
            .clone()
    }

    /// Deletes a destination and its messages. Fails if it has consumers.
    pub fn delete_dest(&self, d: &Destination) -> Result<bool, String> {
        let mut w = self.dests[shard_of(d)].write();
        match w.get(d) {
            None => Ok(false),
            Some(x) if x.has_consumers() => {
                Err(format!("Destination still has an active subscription: {d}"))
            }
            Some(_) => {
                let x = w.remove(d).unwrap();
                drop(w);
                x.clear();
                tracing::debug!("destination deleted: {d}");
                self.temp_advisory(d, dest_op::REMOVE);
                Ok(true)
            }
        }
    }

    pub fn destinations(&self) -> Vec<Arc<Dest>> {
        let mut v: Vec<_> = self.dests.iter().flat_map(|s| s.read().values().cloned().collect::<Vec<_>>()).collect();
        v.sort_by(|a, b| a.dest.name.cmp(&b.dest.name));
        v
    }

    /// Deletes the temporary destinations owned by a closed connection.
    pub fn drop_temp_destinations(&self, conn_id: u64) {
        let mut removed = Vec::new();
        for shard in &self.dests {
            shard.write().retain(|d, x| {
                let drop = d.kind.is_temporary() && x.owner == Some(conn_id);
                if drop {
                    removed.push(d.clone());
                }
                !drop
            });
        }
        for d in removed {
            self.temp_advisory(&d, dest_op::REMOVE);
        }
    }

    // -- temporary destination advisories --------------------------------------

    /// Registers a consumer on the advisory topics. Returns true when it watches temporary destinations.
    pub fn add_advisory_sub(&self, conn: Arc<ConnHandle>, consumer: ConsumerId, dest: Destination) {
        let queues = dest.name.split(',').any(|n| n == ADVISORY_TEMP_QUEUE);
        let topics = dest.name.split(',').any(|n| n == ADVISORY_TEMP_TOPIC);
        if !queues && !topics {
            return;
        }
        let sub = AdvisorySub { conn, consumer, dest, queues, topics };
        // Like ActiveMQ, a new advisory consumer first learns the existing temporary destinations.
        for d in self.destinations() {
            if (d.dest.kind == DestKind::TempQueue && sub.queues) || (d.dest.kind == DestKind::TempTopic && sub.topics) {
                self.send_advisory(&sub, &d.dest, dest_op::ADD);
            }
        }
        self.advisory_subs.lock().push(sub);
    }

    pub fn remove_advisory_sub(&self, consumer: &ConsumerId) {
        self.advisory_subs.lock().retain(|s| &s.consumer != consumer);
    }

    pub fn remove_advisory_conn(&self, conn_id: u64) {
        self.advisory_subs.lock().retain(|s| s.conn.id != conn_id);
    }

    /// Announces the creation or removal of a temporary destination.
    pub fn temp_advisory(&self, d: &Destination, op: u8) {
        if !d.kind.is_temporary() {
            return;
        }
        let subs = self.advisory_subs.lock();
        for s in subs.iter() {
            if (d.kind == DestKind::TempQueue && s.queues) || (d.kind == DestKind::TempTopic && s.topics) {
                self.send_advisory(s, d, op);
            }
        }
    }

    fn send_advisory(&self, s: &AdvisorySub, d: &Destination, op: u8) {
        let topic = if d.kind == DestKind::TempQueue { ADVISORY_TEMP_QUEUE } else { ADVISORY_TEMP_TOPIC };
        let pid = ProducerId { connection_id: self.broker_id.value.clone(), session_id: 0, value: 0 };
        let seq = self.advisory_seq.fetch_add(1, Ordering::Relaxed);
        let mut m = Message::new(crate::openwire::types::ACTIVEMQ_MESSAGE);
        m.producer_id = Some(pid.clone());
        m.destination = Some(Destination::new(DestKind::Topic, topic));
        m.message_id = Some(MessageId {
            text_view: None,
            producer_id: Some(pid),
            producer_sequence_id: seq as i64,
            broker_sequence_id: seq as i64,
        });
        m.jms_type = Some("Advisory".into());
        m.timestamp = now_ms();
        m.data_structure = Some(DataStructure::DestinationInfo(Box::new(DestinationInfo {
            header: Header::default(),
            connection_id: None,
            destination: Some(d.clone()),
            operation_type: op,
            timeout: 0,
            broker_path: None,
        })));
        s.conn.send(Command::MessageDispatch(MessageDispatch {
            header: Header::default(),
            consumer_id: Some(s.consumer.clone()),
            destination: Some(s.dest.clone()),
            message: Some(Arc::new(m)),
            redelivery_counter: 0,
        }));
    }

    // -- message intake ---------------------------------------------------------

    /// Applies the `[expiry]` options. Returns false when the message is already expired.
    pub fn apply_expiry_options(&self, msg: &mut Message, now: i64) -> bool {
        let cfg = &self.cfg;
        if cfg.use_broker_clock && msg.expiration > 0 {
            let ttl = if msg.timestamp > 0 { msg.expiration - msg.timestamp } else { 0 };
            if msg.timestamp > 0 {
                msg.timestamp = now;
                msg.expiration = now + ttl.max(0);
            }
        }
        let base = if msg.timestamp > 0 && !cfg.use_broker_clock { msg.timestamp } else { now };
        if msg.expiration == 0 && cfg.default_ttl_ms > 0 {
            msg.expiration = base + cfg.default_ttl_ms as i64;
        }
        if cfg.ttl_ceiling_ms > 0 && msg.expiration > 0 {
            let cap = base + cfg.ttl_ceiling_ms as i64;
            if msg.expiration > cap {
                msg.expiration = cap;
            }
        }
        !(msg.expiration > 0 && msg.expiration <= now)
    }

    /// True when the body is large enough that compression should run off the async runtime.
    pub fn compression_is_heavy(&self, msg: &Message) -> bool {
        self.cfg.compress_threshold_bytes > 0 && !msg.compressed && msg.content_len() > 1024 * 1024
    }

    pub fn compress(&self, msg: &mut Message) {
        compress::maybe_compress(msg, self.cfg.compress_threshold_bytes, self.cfg.compress_min_saving_pct);
    }

    /// Checks the memory limit for a message of `size` bytes.
    fn admit_memory(&self, size: u64) -> bool {
        let limit = self.memory.limit;
        if limit == 0 {
            return true;
        }
        let used = self.memory.used();
        if self.limited.load(Ordering::Relaxed) {
            if used < limit * 9 / 10 {
                self.limited.store(false, Ordering::Relaxed);
                let dropped = self.dropped_while_limited.swap(0, Ordering::Relaxed);
                tracing::warn!(
                    "memory below 90% of the limit ({} MB): accepting messages again; {} asynchronous messages were dropped",
                    limit / (1024 * 1024),
                    dropped
                );
            } else {
                return false;
            }
        }
        if used + size > limit {
            self.limited.store(true, Ordering::Relaxed);
            tracing::warn!(
                "memory limit of {} MB reached: rejecting new messages",
                limit / (1024 * 1024)
            );
            return false;
        }
        true
    }

    pub fn memory_limited(&self) -> bool {
        self.limited.load(Ordering::Relaxed)
    }

    /// Stores a prepared message in its destination. `sync` tells whether the producer waits.
    pub fn deliver(&self, msg: Message, sync: bool, now: i64) -> Result<(), Rejection> {
        self.deliver_checked(msg, sync, now, true)
    }

    /// Like `deliver`; `check_memory = false` is used for transaction commits, which are never refused.
    pub fn deliver_checked(&self, mut msg: Message, sync: bool, now: i64, check_memory: bool) -> Result<(), Rejection> {
        let Some(dest) = msg.destination.clone() else {
            return Err(Rejection::Error {
                class: "javax.jms.InvalidDestinationException",
                message: "Message has no destination".into(),
            });
        };
        let d = if dest.kind.is_temporary() {
            match self.get_dest(&dest) {
                Some(d) => d,
                None => {
                    return Err(Rejection::Error {
                        class: "javax.jms.InvalidDestinationException",
                        message: format!("Cannot publish to a deleted Destination: {dest}"),
                    })
                }
            }
        } else {
            self.get_or_create(&dest, None)
        };
        if d.is_duplicate(&msg) {
            self.stats.duplicates.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("duplicate message {} ignored", msg.message_id_text());
            return Ok(());
        }
        let size = msg.content_len() as u64 + msg.properties_len() as u64 + ENTRY_OVERHEAD;
        if check_memory && !self.admit_memory(size) {
            if !sync {
                self.dropped_while_limited.fetch_add(1, Ordering::Relaxed);
                self.stats.dropped_async.fetch_add(1, Ordering::Relaxed);
            }
            return Err(Rejection::Error {
                class: "javax.jms.ResourceAllocationException",
                message: format!(
                    "Usage Manager Memory Limit reached ({} MB). Stopping producer to {dest}",
                    self.memory.limit / (1024 * 1024)
                ),
            });
        }
        let seq = self.next_seq();
        if let Some(id) = msg.message_id.as_mut() {
            id.broker_sequence_id = seq as i64;
        }
        msg.header = Header::default();
        msg.broker_in_time = now;
        let meta = Meta::new(self.memory.clone(), &msg);
        let entry = Entry { seq, msg: Arc::new(msg), meta, redelivery: 0 };
        self.stats.messages_in.fetch_add(1, Ordering::Relaxed);
        let effects = d.enqueue(entry, now);
        self.run_effects(effects, now);
        Ok(())
    }

    pub fn run_effects(&self, effects: Vec<Effect>, now: i64) {
        for e in effects {
            match e {
                Effect::ToDlq { entry, cause } => self.to_dlq(entry, &cause, now),
            }
        }
    }

    /// Moves a poison message to `ActiveMQ.DLQ`, keeping its id and headers.
    fn to_dlq(&self, entry: Entry, cause: &str, now: i64) {
        let mut msg = (*entry.msg).clone();
        let mut props = entry.properties().map(|p| (**p).clone()).unwrap_or_default();
        props.set("dlqDeliveryFailureCause", Value::String(cause.to_string()));
        msg.marshalled_properties = Some(PrimitiveMap::encode(&props));
        if msg.original_destination.is_none() {
            msg.original_destination = msg.destination.clone();
        }
        let dlq = Destination::queue(DLQ_NAME);
        msg.destination = Some(dlq.clone());
        msg.redelivery_counter = entry.redelivery;
        let seq = self.next_seq();
        if let Some(id) = msg.message_id.as_mut() {
            id.broker_sequence_id = seq as i64;
        }
        let meta = Meta::new(self.memory.clone(), &msg);
        let e = Entry { seq, msg: Arc::new(msg), meta, redelivery: entry.redelivery };
        let d = self.get_or_create(&dlq, None);
        let effects = d.enqueue(e, now);
        self.run_effects(effects, now);
    }

    /// Total messages held in memory.
    pub fn message_count(&self) -> usize {
        self.destinations().iter().map(|d| d.message_count()).sum()
    }

    // -- housekeeping -----------------------------------------------------------

    /// Periodic expiry sweep, idle-destination removal and expiry summaries.
    pub async fn housekeeping(self: Arc<Self>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let interval = Duration::from_millis(self.cfg.expiry_check_interval_ms.max(1));
        let mut summary: HashMap<Destination, u64> = HashMap::new();
        let mut last_summary = std::time::Instant::now();
        loop {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = shutdown.changed() => return,
            }
            let now = now_ms();
            for d in self.destinations() {
                if !d.has_expiring() {
                    continue;
                }
                let n = d.sweep_expired(now, 10_000);
                if n > 0 {
                    *summary.entry(d.dest.clone()).or_default() += n as u64;
                }
            }
            if last_summary.elapsed() >= Duration::from_secs(60) {
                for (d, n) in summary.drain() {
                    tracing::info!("{d}: {n} messages expired in the last minute");
                }
                last_summary = std::time::Instant::now();
            }
            let secs = self.cfg.auto_delete_empty_after_secs;
            if secs > 0 {
                let idle: Vec<Destination> = self
                    .destinations()
                    .into_iter()
                    .filter(|d| !d.dest.kind.is_temporary() && d.dest.name.as_ref() != DLQ_NAME && d.idle_for(secs))
                    .map(|d| d.dest.clone())
                    .collect();
                for d in idle {
                    let _ = self.delete_dest(&d);
                }
            }
        }
    }
}
