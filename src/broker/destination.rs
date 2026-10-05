// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! A destination (queue or topic) and its subscriptions.
//!
//! Queues keep pending messages in a `BTreeMap` keyed by sequence number, so dispatch is
//! always FIFO and a returned message goes back to its original position. Topics give
//! every subscription its own pending list.

use parking_lot::Mutex;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use super::conn::ConnHandle;
use super::entry::Entry;
use crate::openwire::model::*;
use crate::selector::Selector;

/// Work to do after the destination lock is released.
pub enum Effect {
    ToDlq { entry: Entry, cause: String },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    pub enqueued: u64,
    pub dequeued: u64,
    pub expired: u64,
    pub discarded: u64,
    pub dispatched: u64,
}

#[derive(Debug, Clone)]
pub struct ProducerMeta {
    pub conn_id: u64,
    pub remote: String,
    pub connection_id: String,
}

/// Parameters of a new subscription.
pub struct SubSpec {
    pub id: ConsumerId,
    pub conn: Arc<ConnHandle>,
    pub prefetch: i32,
    pub selector: Option<Arc<Selector>>,
    pub no_local: bool,
    pub browser: bool,
}

struct Inflight {
    entry: Entry,
    /// Covered by a DELIVERED (or uncommitted transacted) ack: no longer counts against prefetch.
    freed: bool,
    /// Acknowledged inside a transaction that has not ended yet.
    tx: bool,
}

pub struct Sub {
    pub id: ConsumerId,
    pub conn: Arc<ConnHandle>,
    pub prefetch: usize,
    pub selector: Option<Arc<Selector>>,
    pub no_local: bool,
    browser: Option<VecDeque<Entry>>,
    browse_finished: bool,
    inflight: BTreeMap<u64, Inflight>,
    by_seq: HashMap<u64, u64>,
    freed: usize,
    next_dseq: u64,
    /// Highest queue sequence known not to be dispatchable to this subscription.
    cursor: u64,
    pull: Option<u64>,
    pull_generation: u64,
    /// Topic subscriptions: messages waiting for this subscriber.
    tpending: BTreeMap<u64, Entry>,
    texpiry: BTreeSet<(i64, u64)>,
    pub dispatched: u64,
    pub created: chrono::DateTime<chrono::Local>,
}

impl Sub {
    fn new(spec: SubSpec) -> Sub {
        Sub {
            id: spec.id,
            conn: spec.conn,
            prefetch: spec.prefetch.max(0) as usize,
            selector: spec.selector,
            no_local: spec.no_local,
            browser: if spec.browser { Some(VecDeque::new()) } else { None },
            browse_finished: false,
            inflight: BTreeMap::new(),
            by_seq: HashMap::new(),
            freed: 0,
            next_dseq: 1,
            cursor: 0,
            pull: None,
            pull_generation: 0,
            tpending: BTreeMap::new(),
            texpiry: BTreeSet::new(),
            dispatched: 0,
            created: chrono::Local::now(),
        }
    }

    fn window(&self) -> usize {
        self.inflight.len() - self.freed
    }

    fn has_credit(&self) -> bool {
        if self.prefetch == 0 {
            self.pull.is_some()
        } else {
            self.window() < self.prefetch
        }
    }

    pub fn inflight_len(&self) -> usize {
        self.inflight.len()
    }

    pub fn pending_len(&self) -> usize {
        self.tpending.len()
    }

    pub fn is_browser(&self) -> bool {
        self.browser.is_some()
    }

    fn accepts(&self, e: &Entry) -> bool {
        if self.no_local {
            if let Some(p) = &e.msg.producer_id {
                if p.connection_id == self.id.connection_id {
                    return false;
                }
            }
        }
        match &self.selector {
            Some(s) => s.matches(e),
            None => true,
        }
    }

    fn send(&mut self, dest: &Destination, e: Entry) {
        let dseq = self.next_dseq;
        self.next_dseq += 1;
        let md = MessageDispatch {
            header: Header::default(),
            consumer_id: Some(self.id.clone()),
            destination: Some(dest.clone()),
            message: Some(e.dispatch_message()),
            redelivery_counter: e.redelivery,
        };
        self.conn.send(Command::MessageDispatch(md));
        self.conn.count_dispatch();
        self.by_seq.insert(e.seq, dseq);
        self.inflight.insert(dseq, Inflight { entry: e, freed: false, tx: false });
        self.pull = None;
        self.dispatched += 1;
    }

    fn send_null(&mut self, dest: &Destination) {
        let md = MessageDispatch {
            header: Header::default(),
            consumer_id: Some(self.id.clone()),
            destination: Some(dest.clone()),
            message: None,
            redelivery_counter: 0,
        };
        self.conn.send(Command::MessageDispatch(md));
        self.pull = None;
    }

    fn remove_inflight(&mut self, dseq: u64) -> Option<Entry> {
        let inf = self.inflight.remove(&dseq)?;
        self.by_seq.remove(&inf.entry.seq);
        if inf.freed {
            self.freed -= 1;
        }
        Some(inf.entry)
    }

    /// Dispatch sequence numbers of an ack range, in dispatch order.
    fn ack_range(&self, ack: &MessageAck, cumulative: bool) -> Vec<u64> {
        let Some(last) = ack.last_message_id.as_ref().and_then(|m| self.by_seq.get(&(m.broker_sequence_id as u64))) else {
            return Vec::new();
        };
        let first = if cumulative {
            None
        } else {
            ack.first_message_id.as_ref().and_then(|m| self.by_seq.get(&(m.broker_sequence_id as u64))).copied()
        };
        let start = first.unwrap_or(0).min(*last);
        self.inflight.range(start..=*last).map(|(k, _)| *k).collect()
    }
}

struct State {
    pending: BTreeMap<u64, Entry>,
    expiry: BTreeSet<(i64, u64)>,
    subs: Vec<Sub>,
    rr: usize,
    producers: HashMap<ProducerId, ProducerMeta>,
    audit: HashMap<ProducerId, Audit>,
    stats: Stats,
    idle_since: Option<Instant>,
    /// Messages acknowledged in an open transaction by consumers that have since closed.
    reserved: HashMap<ConsumerId, Vec<Entry>>,
}

/// Duplicate detection window per producer (like ActiveMQ's producer audit).
struct Audit {
    seen: HashSet<i64>,
    order: VecDeque<i64>,
}

const AUDIT_WINDOW: usize = 1024;

impl Audit {
    fn check_and_record(&mut self, seq: i64) -> bool {
        if self.seen.contains(&seq) {
            return true;
        }
        self.seen.insert(seq);
        self.order.push_back(seq);
        if self.order.len() > AUDIT_WINDOW {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
        false
    }
}

pub struct Dest {
    pub dest: Destination,
    /// Owning connection of a temporary destination.
    pub owner: Option<u64>,
    pub created: chrono::DateTime<chrono::Local>,
    topic_max_pending: usize,
    state: Mutex<State>,
}

/// Admin view of one subscription.
#[derive(Debug, Clone)]
pub struct SubSnapshot {
    pub consumer_id: String,
    pub remote: String,
    pub prefetch: usize,
    pub inflight: usize,
    pub pending: usize,
    pub selector: Option<String>,
    pub browser: bool,
    pub dispatched: u64,
}

/// Admin view of one destination.
#[derive(Debug, Clone)]
pub struct DestSnapshot {
    pub dest: Destination,
    pub pending: usize,
    pub inflight: usize,
    pub consumers: Vec<SubSnapshot>,
    pub producers: Vec<(String, ProducerMeta)>,
    pub stats: Stats,
    pub with_expiry: usize,
    pub next_expiry: Option<i64>,
    pub memory: u64,
}

impl Dest {
    pub fn new(dest: Destination, owner: Option<u64>, topic_max_pending: usize) -> Dest {
        Dest {
            dest,
            owner,
            created: chrono::Local::now(),
            topic_max_pending,
            state: Mutex::new(State {
                pending: BTreeMap::new(),
                expiry: BTreeSet::new(),
                subs: Vec::new(),
                rr: 0,
                producers: HashMap::new(),
                audit: HashMap::new(),
                stats: Stats::default(),
                idle_since: Some(Instant::now()),
                reserved: HashMap::new(),
            }),
        }
    }

    pub fn is_queue(&self) -> bool {
        self.dest.kind.is_queue()
    }

    // -- producers ----------------------------------------------------------

    pub fn add_producer(&self, id: ProducerId, meta: ProducerMeta) {
        let mut st = self.state.lock();
        st.producers.insert(id, meta);
        st.idle_since = None;
    }

    pub fn remove_producer(&self, id: &ProducerId) {
        let mut st = self.state.lock();
        st.producers.remove(id);
        st.audit.remove(id);
        Self::touch_idle(&mut st);
    }

    /// Returns true when this producer already sent this sequence id recently.
    pub fn is_duplicate(&self, msg: &Message) -> bool {
        let Some(id) = &msg.message_id else { return false };
        let Some(pid) = &id.producer_id else { return false };
        let mut st = self.state.lock();
        st.audit
            .entry(pid.clone())
            .or_insert_with(|| Audit { seen: HashSet::new(), order: VecDeque::new() })
            .check_and_record(id.producer_sequence_id)
    }

    /// Counts a message that expired before it could be stored.
    pub fn add_expired(&self) {
        self.state.lock().stats.expired += 1;
    }

    // -- enqueue ------------------------------------------------------------

    /// Stores a message (queue) or fans it out (topic), then dispatches.
    pub fn enqueue(&self, entry: Entry, now_ms: i64) -> Vec<Effect> {
        let mut st = self.state.lock();
        st.stats.enqueued += 1;
        if self.is_queue() {
            if entry.msg.expiration > 0 {
                st.expiry.insert((entry.msg.expiration, entry.seq));
            }
            st.pending.insert(entry.seq, entry);
            st.idle_since = None;
            self.dispatch_queue(&mut st, now_ms);
        } else {
            let max = self.topic_max_pending;
            let mut discarded = 0;
            for sub in st.subs.iter_mut() {
                if !sub.accepts(&entry) {
                    continue;
                }
                if entry.msg.expiration > 0 {
                    sub.texpiry.insert((entry.msg.expiration, entry.seq));
                }
                sub.tpending.insert(entry.seq, entry.clone());
                if max > 0 {
                    while sub.tpending.len() > max {
                        if let Some((seq, old)) = sub.tpending.pop_first() {
                            sub.texpiry.remove(&(old.msg.expiration, seq));
                            discarded += 1;
                        }
                    }
                }
            }
            st.stats.discarded += discarded;
            self.dispatch_topic(&mut st, now_ms);
        }
        Vec::new()
    }

    /// Puts returned messages back at their original positions (queues only).
    fn reinsert(&self, st: &mut State, entries: Vec<Entry>, now_ms: i64) {
        let mut min_seq = u64::MAX;
        for e in entries {
            if e.expired(now_ms) {
                st.stats.expired += 1;
                continue;
            }
            min_seq = min_seq.min(e.seq);
            if e.msg.expiration > 0 {
                st.expiry.insert((e.msg.expiration, e.seq));
            }
            st.pending.insert(e.seq, e);
        }
        if min_seq != u64::MAX {
            for sub in st.subs.iter_mut() {
                sub.cursor = sub.cursor.min(min_seq.saturating_sub(1));
            }
        }
    }

    // -- dispatch -----------------------------------------------------------

    fn dispatch_queue(&self, st: &mut State, now_ms: i64) {
        let State { pending, expiry, subs, rr, stats, .. } = st;
        if subs.is_empty() {
            return;
        }
        // Browsers are served from their own snapshot.
        for sub in subs.iter_mut().filter(|s| s.browser.is_some()) {
            Self::dispatch_browser(&self.dest, sub, now_ms);
        }
        let n = subs.len();
        loop {
            let mut progressed = false;
            for k in 0..n {
                let i = (*rr + k) % n;
                let sub = &mut subs[i];
                if sub.browser.is_some() || !sub.has_credit() {
                    continue;
                }
                // Earliest pending message after the cursor that this subscription accepts.
                let mut chosen = None;
                let mut expired = Vec::new();
                for (seq, e) in pending.range(sub.cursor + 1..) {
                    if e.expired(now_ms) {
                        expired.push(*seq);
                        continue;
                    }
                    if sub.accepts(e) {
                        chosen = Some(*seq);
                        break;
                    }
                    sub.cursor = *seq;
                }
                for seq in expired {
                    if let Some(e) = pending.remove(&seq) {
                        expiry.remove(&(e.msg.expiration, seq));
                        stats.expired += 1;
                    }
                }
                if let Some(seq) = chosen {
                    let e = pending.remove(&seq).unwrap();
                    if e.msg.expiration > 0 {
                        expiry.remove(&(e.msg.expiration, seq));
                    }
                    sub.cursor = seq;
                    sub.send(&self.dest, e);
                    stats.dispatched += 1;
                    *rr = (i + 1) % n;
                    progressed = true;
                    break;
                }
            }
            if !progressed || pending.is_empty() {
                break;
            }
        }
    }

    fn dispatch_browser(dest: &Destination, sub: &mut Sub, now_ms: i64) {
        loop {
            let credit = if sub.prefetch == 0 { sub.pull.is_some() } else { sub.window() < sub.prefetch };
            if !credit {
                return;
            }
            let next = sub.browser.as_mut().and_then(|b| b.pop_front());
            match next {
                Some(e) if e.expired(now_ms) => continue,
                Some(e) => sub.send(dest, e),
                None => {
                    if !sub.browse_finished {
                        sub.browse_finished = true;
                        sub.send_null(dest);
                    }
                    return;
                }
            }
        }
    }

    fn dispatch_topic(&self, st: &mut State, now_ms: i64) {
        let State { subs, stats, .. } = st;
        for sub in subs.iter_mut() {
            while sub.has_credit() {
                let Some((seq, e)) = sub.tpending.pop_first() else { break };
                if e.msg.expiration > 0 {
                    sub.texpiry.remove(&(e.msg.expiration, seq));
                }
                if e.expired(now_ms) {
                    stats.expired += 1;
                    continue;
                }
                sub.send(&self.dest, e);
                stats.dispatched += 1;
            }
        }
    }

    fn dispatch(&self, st: &mut State, now_ms: i64) {
        if self.is_queue() {
            self.dispatch_queue(st, now_ms);
        } else {
            self.dispatch_topic(st, now_ms);
        }
    }

    // -- subscriptions --------------------------------------------------------

    pub fn add_sub(&self, spec: SubSpec, now_ms: i64) {
        let mut st = self.state.lock();
        let mut sub = Sub::new(spec);
        if let Some(snapshot) = sub.browser.as_mut() {
            for e in st.pending.values() {
                if e.expired(now_ms) {
                    continue;
                }
                snapshot.push_back(e.clone());
            }
            if let Some(sel) = sub.selector.clone() {
                snapshot.retain(|e| sel.matches(e));
            }
        }
        st.subs.push(sub);
        st.idle_since = None;
        self.dispatch(&mut st, now_ms);
    }

    /// Removes a subscription. Queue messages it held return to pending.
    pub fn remove_sub(&self, id: &ConsumerId, last_delivered: i64, now_ms: i64) -> bool {
        let mut st = self.state.lock();
        let Some(pos) = st.subs.iter().position(|s| &s.id == id) else { return false };
        let sub = st.subs.remove(pos);
        if st.rr >= st.subs.len() {
            st.rr = 0;
        }
        if self.is_queue() && sub.browser.is_none() {
            let mut returned = Vec::new();
            let mut reserved = Vec::new();
            for inf in sub.inflight.into_values() {
                if inf.tx {
                    reserved.push(inf.entry);
                    continue;
                }
                let mut e = inf.entry;
                let delivered = last_delivered < 0 || e.seq <= last_delivered as u64;
                if delivered {
                    e.redelivery = e.redelivery.saturating_add(1);
                }
                returned.push(e);
            }
            if !reserved.is_empty() {
                st.reserved.entry(id.clone()).or_default().extend(reserved);
            }
            self.reinsert(&mut st, returned, now_ms);
        }
        Self::touch_idle(&mut st);
        self.dispatch(&mut st, now_ms);
        true
    }

    pub fn has_consumers(&self) -> bool {
        !self.state.lock().subs.is_empty()
    }

    /// Changes a consumer's prefetch (ConsumerControl).
    pub fn set_prefetch(&self, id: &ConsumerId, prefetch: i32, now_ms: i64) {
        let mut st = self.state.lock();
        if let Some(sub) = st.subs.iter_mut().find(|s| &s.id == id) {
            sub.prefetch = prefetch.max(0) as usize;
        }
        self.dispatch(&mut st, now_ms);
    }

    // -- acknowledgements -----------------------------------------------------

    /// Applies an ack. With `transacted`, only frees prefetch space (the ack is applied at commit).
    pub fn ack(&self, ack: &MessageAck, transacted: bool, now_ms: i64) -> Vec<Effect> {
        let mut effects = Vec::new();
        let mut st = self.state.lock();
        let Some(cid) = &ack.consumer_id else { return effects };
        let Some(pos) = st.subs.iter().position(|s| &s.id == cid) else {
            if !transacted {
                self.ack_reserved(&mut st, cid, ack, &mut effects);
                Self::touch_idle(&mut st);
                self.dispatch(&mut st, now_ms);
            }
            return effects;
        };
        let mut consumed = 0u64;
        let mut expired = 0u64;
        let mut discarded = 0u64;
        {
            let sub = &mut st.subs[pos];
            let browser = sub.browser.is_some();
            if transacted && ack.ack_type != ack_type::DELIVERED {
                let cumulative = matches!(ack.ack_type, ack_type::STANDARD | ack_type::UNMATCHED);
                for d in sub.ack_range(ack, cumulative) {
                    if let Some(inf) = sub.inflight.get_mut(&d) {
                        inf.tx = true;
                        if !inf.freed {
                            inf.freed = true;
                            sub.freed += 1;
                        }
                    }
                }
            } else {
                match ack.ack_type {
                    ack_type::DELIVERED => {
                        for d in sub.ack_range(ack, false) {
                            if let Some(inf) = sub.inflight.get_mut(&d) {
                                if !inf.freed {
                                    inf.freed = true;
                                    sub.freed += 1;
                                }
                            }
                        }
                    }
                    ack_type::STANDARD | ack_type::UNMATCHED => {
                        for d in sub.ack_range(ack, true) {
                            if sub.remove_inflight(d).is_some() {
                                consumed += 1;
                            }
                        }
                    }
                    ack_type::INDIVIDUAL => {
                        let last = ack
                            .last_message_id
                            .as_ref()
                            .and_then(|m| sub.by_seq.get(&(m.broker_sequence_id as u64)))
                            .copied();
                        if let Some(d) = last {
                            if sub.remove_inflight(d).is_some() {
                                consumed += 1;
                            }
                        }
                    }
                    ack_type::POISON => {
                        let cause = ack
                            .poison_cause
                            .as_ref()
                            .and_then(|t| t.message.clone())
                            .unwrap_or_else(|| "Delivery failure: poison ack".to_string());
                        for d in sub.ack_range(ack, false) {
                            if let Some(e) = sub.remove_inflight(d) {
                                if browser {
                                    continue;
                                }
                                if e.msg.persistent {
                                    effects.push(Effect::ToDlq { entry: e, cause: cause.clone() });
                                } else {
                                    discarded += 1;
                                }
                            }
                        }
                    }
                    ack_type::REDELIVERED => {
                        for d in sub.ack_range(ack, false) {
                            if let Some(inf) = sub.inflight.get_mut(&d) {
                                inf.entry.redelivery = inf.entry.redelivery.saturating_add(1);
                            }
                        }
                    }
                    ack_type::EXPIRED => {
                        for d in sub.ack_range(ack, false) {
                            if sub.remove_inflight(d).is_some() {
                                expired += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if browser {
                // A browser never removes queue messages.
                consumed = 0;
                expired = 0;
            }
        }
        st.stats.dequeued += consumed;
        st.stats.expired += expired;
        st.stats.discarded += discarded;
        Self::touch_idle(&mut st);
        self.dispatch(&mut st, now_ms);
        effects
    }

    /// Commits an ack of a consumer that closed while its transaction was open.
    fn ack_reserved(&self, st: &mut State, cid: &ConsumerId, ack: &MessageAck, effects: &mut Vec<Effect>) {
        let Some(list) = st.reserved.get_mut(cid) else { return };
        let Some(last) = ack.last_message_id.as_ref().map(|m| m.broker_sequence_id as u64) else { return };
        let first = ack.first_message_id.as_ref().map(|m| m.broker_sequence_id as u64);
        let mut taken = Vec::new();
        list.retain(|e| {
            let hit = match ack.ack_type {
                ack_type::STANDARD | ack_type::UNMATCHED => e.seq <= last,
                ack_type::INDIVIDUAL => e.seq == last,
                ack_type::POISON | ack_type::EXPIRED => e.seq <= last && first.map_or(true, |f| e.seq >= f),
                _ => false,
            };
            if hit {
                taken.push(e.clone());
            }
            !hit
        });
        if list.is_empty() {
            st.reserved.remove(cid);
        }
        for e in taken {
            match ack.ack_type {
                ack_type::POISON => {
                    if e.msg.persistent {
                        effects.push(Effect::ToDlq { entry: e, cause: "Delivery failure: poison ack".into() });
                    } else {
                        st.stats.discarded += 1;
                    }
                }
                ack_type::EXPIRED => st.stats.expired += 1,
                _ => st.stats.dequeued += 1,
            }
        }
    }

    /// Returns the reserved messages of a closed consumer to pending (transaction rolled back).
    pub fn release_reserved(&self, cid: &ConsumerId, now_ms: i64) {
        let mut st = self.state.lock();
        // The transaction is over: messages still inflight to an open consumer are no longer reserved.
        if let Some(sub) = st.subs.iter_mut().find(|s| &s.id == cid) {
            for inf in sub.inflight.values_mut() {
                inf.tx = false;
            }
        }
        let Some(list) = st.reserved.remove(cid) else { return };
        let returned = list
            .into_iter()
            .map(|mut e| {
                e.redelivery = e.redelivery.saturating_add(1);
                e
            })
            .collect();
        self.reinsert(&mut st, returned, now_ms);
        self.dispatch(&mut st, now_ms);
    }

    // -- pull (prefetch 0) ------------------------------------------------------

    /// Registers a pull. Returns a generation id when a timeout timer must be armed.
    pub fn pull(&self, id: &ConsumerId, timeout: i64, now_ms: i64) -> Option<u64> {
        let mut st = self.state.lock();
        let pos = st.subs.iter().position(|s| &s.id == id)?;
        {
            let sub = &mut st.subs[pos];
            if sub.prefetch > 0 {
                return None;
            }
            sub.pull_generation += 1;
            sub.pull = Some(sub.pull_generation);
        }
        self.dispatch(&mut st, now_ms);
        let dest = self.dest.clone();
        let sub = &mut st.subs[pos];
        match sub.pull {
            None => None, // satisfied immediately
            Some(gen) if timeout < 0 => {
                // receiveNoWait: answer at once.
                let _ = gen;
                sub.send_null(&dest);
                None
            }
            Some(gen) if timeout > 0 => Some(gen),
            Some(_) => None, // wait indefinitely
        }
    }

    /// Called when a pull timer fires.
    pub fn pull_timeout(&self, id: &ConsumerId, generation: u64) {
        let mut st = self.state.lock();
        let dest = self.dest.clone();
        if let Some(sub) = st.subs.iter_mut().find(|s| &s.id == id) {
            if sub.pull == Some(generation) {
                sub.send_null(&dest);
            }
        }
    }

    // -- expiry -------------------------------------------------------------

    /// Deletes expired messages, at most `max` per call. Returns how many were deleted.
    pub fn sweep_expired(&self, now_ms: i64, max: usize) -> usize {
        let mut st = self.state.lock();
        let mut removed = 0;
        while removed < max {
            let Some(&(exp, seq)) = st.expiry.first() else { break };
            if exp > now_ms {
                break;
            }
            st.expiry.pop_first();
            if st.pending.remove(&seq).is_some() {
                removed += 1;
            }
        }
        for sub in st.subs.iter_mut() {
            while removed < max {
                let Some(&(exp, seq)) = sub.texpiry.first() else { break };
                if exp > now_ms {
                    break;
                }
                sub.texpiry.pop_first();
                if sub.tpending.remove(&seq).is_some() {
                    removed += 1;
                }
            }
        }
        st.stats.expired += removed as u64;
        if removed > 0 {
            Self::touch_idle(&mut st);
        }
        removed
    }

    pub fn has_expiring(&self) -> bool {
        let st = self.state.lock();
        !st.expiry.is_empty() || st.subs.iter().any(|s| !s.texpiry.is_empty())
    }

    // -- lifecycle ------------------------------------------------------------

    fn touch_idle(st: &mut State) {
        let empty = st.pending.is_empty()
            && st.subs.is_empty()
            && st.producers.is_empty();
        if empty {
            if st.idle_since.is_none() {
                st.idle_since = Some(Instant::now());
            }
        } else {
            st.idle_since = None;
        }
    }

    /// True when the destination has been empty and unused for at least `secs`.
    pub fn idle_for(&self, secs: u64) -> bool {
        let st = self.state.lock();
        st.pending.is_empty()
            && st.subs.is_empty()
            && st.producers.is_empty()
            && st.idle_since.map_or(false, |t| t.elapsed().as_secs() >= secs)
    }

    /// Number of messages held (pending plus inflight, all subscriptions).
    pub fn message_count(&self) -> usize {
        let st = self.state.lock();
        st.pending.len() + st.subs.iter().map(|s| s.inflight.len() + s.tpending.len()).sum::<usize>()
    }

    /// Drops every message (used when the destination is deleted).
    pub fn clear(&self) -> usize {
        let mut st = self.state.lock();
        let n = st.pending.len();
        st.pending.clear();
        st.expiry.clear();
        n
    }

    // -- admin --------------------------------------------------------------

    pub fn snapshot(&self) -> DestSnapshot {
        let st = self.state.lock();
        let mut memory: u64 = st.pending.values().map(|e| e.meta.size).sum();
        let consumers = st
            .subs
            .iter()
            .map(|s| {
                memory += s.inflight.values().map(|i| i.entry.meta.size).sum::<u64>();
                memory += s.tpending.values().map(|e| e.meta.size).sum::<u64>();
                SubSnapshot {
                    consumer_id: s.id.to_string(),
                    remote: s.conn.remote.to_string(),
                    prefetch: s.prefetch,
                    inflight: s.inflight.len(),
                    pending: s.tpending.len(),
                    selector: s.selector.as_ref().map(|x| x.text().to_string()),
                    browser: s.browser.is_some(),
                    dispatched: s.dispatched,
                }
            })
            .collect();
        let with_expiry = st.expiry.len() + st.subs.iter().map(|s| s.texpiry.len()).sum::<usize>();
        let next_expiry = st
            .expiry
            .first()
            .map(|x| x.0)
            .into_iter()
            .chain(st.subs.iter().filter_map(|s| s.texpiry.first().map(|x| x.0)))
            .min();
        DestSnapshot {
            dest: self.dest.clone(),
            pending: st.pending.len() + st.subs.iter().map(|s| s.tpending.len()).sum::<usize>(),
            inflight: st.subs.iter().map(|s| s.inflight.len()).sum(),
            consumers,
            producers: st.producers.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
            stats: st.stats,
            with_expiry,
            next_expiry,
            memory,
        }
    }

    /// A page of pending messages in FIFO order: (total, entries).
    pub fn page(&self, offset: usize, limit: usize) -> (usize, Vec<Entry>) {
        let st = self.state.lock();
        let total = st.pending.len();
        (total, st.pending.values().skip(offset).take(limit).cloned().collect())
    }

    /// Finds a pending or inflight message by sequence or JMS message id.
    pub fn find(&self, seq_hint: Option<u64>, message_id: &str) -> Option<(Entry, bool)> {
        let st = self.state.lock();
        if let Some(seq) = seq_hint {
            if let Some(e) = st.pending.get(&seq) {
                if e.msg.message_id_text() == message_id {
                    return Some((e.clone(), false));
                }
            }
        }
        if let Some(e) = st.pending.values().find(|e| e.msg.message_id_text() == message_id) {
            return Some((e.clone(), false));
        }
        for s in &st.subs {
            if let Some(i) = s.inflight.values().find(|i| i.entry.msg.message_id_text() == message_id) {
                return Some((i.entry.clone(), true));
            }
            if let Some(e) = s.tpending.values().find(|e| e.msg.message_id_text() == message_id) {
                return Some((e.clone(), false));
            }
        }
        None
    }
}
