// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! One OpenWire client connection: framing, negotiation, keep-alive and command handling.

use bytes::{Bytes, BytesMut};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use bytes::Buf;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch, Notify};

use crate::auth::Login;
use crate::broker::conn::{ConnHandle, Out};
use crate::broker::destination::{Dest, ProducerMeta, SubSpec};
use crate::broker::entry::MemTicket;
use crate::broker::{now_ms, Broker};
use crate::openwire::marshal::{ChunkBuf, Decoder, LooseCodec, WireCodec};
use crate::openwire::model::*;
use crate::openwire::types as t;
use crate::openwire::wireformat::{self, Negotiated, NegotiationError};
use crate::selector::Selector;

const JMS_EXCEPTION: &str = "javax.jms.JMSException";
const INVALID_DESTINATION: &str = "javax.jms.InvalidDestinationException";
const INVALID_SELECTOR: &str = "javax.jms.InvalidSelectorException";
const SECURITY_EXCEPTION: &str = "java.lang.SecurityException";
const UNSUPPORTED: &str = "java.lang.UnsupportedOperationException";
const XA_UNSUPPORTED: &str = "XA transactions not supported";

/// Size of the reusable per-connection read buffer. Frames larger than this are read
/// directly into their own allocation.
const READ_BUF: usize = 64 * 1024;

/// Buffered frame reader of one connection. Many small frames arrive with one `recv`; each is
/// cut from the reusable buffer without allocating (`split_to`). A message frame, which the broker
/// may keep for a long time, is copied into its own exactly sized allocation so that a stored
/// message never keeps the shared read buffer (and its neighbouring frames) alive. A frame larger
/// than the buffer is read straight from the socket into its own allocation, without a copy.
/// The message `content` and `marshalledProperties` are then `Bytes` slices of that frame.
pub struct FrameReader<R> {
    r: R,
    buf: BytesMut,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub fn new(r: R) -> Self {
        FrameReader { r, buf: BytesMut::with_capacity(READ_BUF) }
    }

    /// Reads more bytes into the buffer; 0 means end of stream.
    async fn fill(&mut self) -> std::io::Result<usize> {
        if self.buf.capacity() - self.buf.len() < READ_BUF / 4 {
            // Reclaims the space of frames already handed out when they have been dropped.
            self.buf.reserve(READ_BUF);
        }
        self.r.read_buf(&mut self.buf).await
    }

    /// Reads one frame body (without the size prefix). `Ok(None)` on clean EOF.
    pub async fn next(&mut self, max: i64) -> std::io::Result<Option<Bytes>> {
        while self.buf.len() < 4 {
            if self.fill().await? == 0 {
                return Ok(None);
            }
        }
        let size = i32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]);
        if size < 0 || size as i64 > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("frame of {size} bytes exceeds the maximum frame size of {max} bytes"),
            ));
        }
        let size = size as usize;
        if size > READ_BUF && self.buf.len() < 4 + size {
            // Large frame: own allocation, the rest is read without passing through the buffer.
            self.buf.advance(4);
            let mut frame = BytesMut::with_capacity(size);
            frame.extend_from_slice(&self.buf);
            self.buf.clear();
            while frame.len() < size {
                let want = (size - frame.len()) as u64;
                if (&mut self.r).take(want).read_buf(&mut frame).await? == 0 {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
            }
            return Ok(Some(frame.freeze()));
        }
        while self.buf.len() < 4 + size {
            self.buf.reserve(4 + size - self.buf.len());
            if self.r.read_buf(&mut self.buf).await? == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
        }
        self.buf.advance(4);
        let frame = self.buf.split_to(size);
        if size > 0 && t::is_message_type(frame[0]) {
            Ok(Some(Bytes::copy_from_slice(&frame)))
        } else {
            Ok(Some(frame.freeze()))
        }
    }
}

/// Reads one frame body (without the size prefix). `Ok(None)` on clean EOF.
async fn read_frame<R: AsyncRead + Unpin>(r: &mut FrameReader<R>, max: i64) -> std::io::Result<Option<Bytes>> {
    r.next(max).await
}

/// Most slices passed to one vectored write.
const MAX_IO_SLICES: usize = 64;
/// Encoded bytes gathered from the queue before they are written.
const BATCH_BYTES: usize = 256 * 1024;

/// Writes all chunks with vectored writes (large bodies are sent without copying), at most
/// `MAX_IO_SLICES` slices per call.
async fn write_chunks<W: AsyncWrite + Unpin>(w: &mut W, chunks: &mut std::collections::VecDeque<Bytes>) -> std::io::Result<()> {
    while !chunks.is_empty() {
        let mut slices = [std::io::IoSlice::new(&[]); MAX_IO_SLICES];
        let count = chunks.len().min(MAX_IO_SLICES);
        for (s, c) in slices.iter_mut().zip(chunks.iter()) {
            *s = std::io::IoSlice::new(c);
        }
        let mut n = w.write_vectored(&slices[..count]).await?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "socket closed"));
        }
        while n > 0 {
            let front = chunks.front_mut().unwrap();
            if n >= front.len() {
                n -= front.len();
                chunks.pop_front();
            } else {
                front.advance(n);
                n = 0;
            }
        }
    }
    Ok(())
}

/// Writer task: batches queued commands into few socket writes and sends keep-alives.
/// It never waits for more commands: whatever is already queued (up to `BATCH_BYTES` of
/// encoded data) is written together, then the next batch starts.
async fn writer_task<W: AsyncWrite + Unpin>(
    mut w: W,
    mut rx: mpsc::UnboundedReceiver<Out>,
    codec: Box<dyn WireCodec>,
    keepalive: Option<Duration>,
    closed: Arc<Notify>,
) {
    let mut buf = ChunkBuf::new();
    loop {
        let first = match keepalive {
            Some(k) => match tokio::time::timeout(k, rx.recv()).await {
                Ok(v) => v,
                Err(_) => Some(Out::Cmd(Command::KeepAliveInfo(Header::default()))),
            },
            None => rx.recv().await,
        };
        let Some(first) = first else { break };
        let mut close = false;
        let push = |o: Out, buf: &mut ChunkBuf, close: &mut bool| match o {
            Out::Cmd(c) => codec.encode(&c, buf),
            Out::CloseAfter(cmds) => {
                for c in &cmds {
                    codec.encode(c, buf);
                }
                *close = true;
            }
        };
        push(first, &mut buf, &mut close);
        while !close && buf.len() < BATCH_BYTES {
            match rx.try_recv() {
                Ok(o) => push(o, &mut buf, &mut close),
                Err(_) => break,
            }
        }
        let mut chunks = buf.take();
        if write_chunks(&mut w, &mut chunks).await.is_err() {
            break;
        }
        if close {
            break;
        }
    }
    let _ = w.shutdown().await;
    closed.notify_waiters();
    closed.notify_one();
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;

struct ConsumerReg {
    dest: Option<Arc<Dest>>,
    session: Option<SessionId>,
}

struct ProducerReg {
    dest: Option<Arc<Dest>>,
    window: i32,
    session: Option<SessionId>,
}

#[derive(Default)]
struct Tx {
    sends: Vec<(Message, MemTicket)>,
    acks: Vec<(Arc<Dest>, MessageAck)>,
}

struct Conn {
    broker: Arc<Broker>,
    handle: Arc<ConnHandle>,
    remote: SocketAddr,
    authenticated: bool,
    user: String,
    connection_id: Option<ConnectionId>,
    sessions: HashSet<SessionId>,
    consumers: HashMap<ConsumerId, ConsumerReg>,
    producers: HashMap<ProducerId, ProducerReg>,
    txs: HashMap<TransactionId, Tx>,
    closing: bool,
}

/// Builds the wire codec of a connection for its negotiated version.
pub type CodecFactory = fn(i32) -> Box<dyn WireCodec>;

fn loose_codec(version: i32) -> Box<dyn WireCodec> {
    Box::new(LooseCodec::new(version))
}

/// Serves one TCP connection until it closes.
pub async fn serve(stream: TcpStream, remote: SocketAddr, broker: Arc<Broker>, shutdown: watch::Receiver<bool>) {
    serve_with_codec(stream, remote, broker, shutdown, loose_codec).await
}

/// Like `serve`, with every command after the `WireFormatInfo` exchange encoded and decoded
/// by the codec that `make_codec` builds.
pub async fn serve_with_codec(
    stream: TcpStream,
    remote: SocketAddr,
    broker: Arc<Broker>,
    mut shutdown: watch::Receiver<bool>,
    make_codec: CodecFactory,
) {
    let _ = stream.set_nodelay(true);
    let (r, w) = stream.into_split();
    let mut r = FrameReader::new(r);
    let max_frame = broker.cfg.max_frame_size;

    // 1. The client's WireFormatInfo (always loose encoding, version independent).
    let first = match tokio::time::timeout(Duration::from_secs(30), read_frame(&mut r, max_frame)).await {
        Ok(Ok(Some(f))) => f,
        Ok(Ok(None)) => return,
        Ok(Err(e)) => {
            tracing::warn!("{remote}: protocol error before negotiation: {e}");
            return;
        }
        Err(_) => {
            tracing::warn!("{remote}: no WireFormatInfo received within 30 s, closing");
            return;
        }
    };
    let client_wf = match Decoder::new(t::MAX_VERSION).decode_frame(first) {
        Ok(Some(Command::WireFormatInfo(wf))) => wf,
        Ok(_) => {
            tracing::warn!("{remote}: the first command is not WireFormatInfo, closing");
            return;
        }
        Err(e) => {
            tracing::warn!("{remote}: cannot decode WireFormatInfo: {e}");
            return;
        }
    };
    let neg: Negotiated = match wireformat::negotiate(&client_wf, max_frame) {
        Ok(n) => n,
        Err(NegotiationError::BadMagic) => {
            tracing::warn!("{remote}: not an OpenWire client (bad magic), closing");
            return;
        }
        Err(NegotiationError::VersionTooOld(v)) => {
            tracing::warn!("{remote}: OpenWire version {v} is not supported (minimum {}), closing", t::MIN_VERSION);
            return;
        }
    };

    // 2. Writer task and connection handle.
    let (tx, rx) = mpsc::unbounded_channel();
    let id = broker.new_conn_id();
    let handle = Arc::new(ConnHandle::new(id, remote, tx));
    handle.info.lock().version = neg.version;
    let keepalive = (neg.max_inactivity_ms > 0).then(|| Duration::from_millis((neg.max_inactivity_ms / 2).max(1) as u64));
    let closed = Arc::new(Notify::new());
    let writer = tokio::spawn(writer_task(w, rx, make_codec(neg.version), keepalive, closed.clone()));
    handle.send(Command::WireFormatInfo(wireformat::broker_wire_format(&client_wf, max_frame)));
    handle.send(Command::BrokerInfo(BrokerInfo {
        header: Header::default(),
        broker_id: Some(broker.broker_id.clone()),
        broker_url: Some(format!("tcp://{}:{}", broker.cfg.bind, broker.cfg.port)),
        peer_broker_infos: None,
        broker_name: Some(broker.cfg.broker_name.clone()),
        slave_broker: false,
        master_broker: false,
        fault_tolerant_configuration: false,
        duplex_connection: false,
        network_connection: false,
        connection_id: id as i64,
        broker_upload_url: None,
        network_properties: None,
    }));
    broker.register_conn(handle.clone());

    let mut conn = Conn {
        broker: broker.clone(),
        handle: handle.clone(),
        remote,
        authenticated: false,
        user: String::new(),
        connection_id: None,
        sessions: HashSet::new(),
        consumers: HashMap::new(),
        producers: HashMap::new(),
        txs: HashMap::new(),
        closing: false,
    };
    let codec = make_codec(neg.version);

    // 3. Read loop with the inactivity timeout.
    let inactivity = (neg.max_inactivity_ms > 0).then(|| Duration::from_millis(neg.max_inactivity_ms as u64));
    let mut timeout = inactivity.map(|d| d + Duration::from_millis(neg.max_inactivity_initial_delay_ms.max(0) as u64));
    let reason: String = loop {
        if conn.closing {
            break "closed by the client".into();
        }
        let read = async {
            match timeout {
                Some(d) => tokio::time::timeout(d, read_frame(&mut r, neg.max_frame_size)).await,
                None => Ok(read_frame(&mut r, neg.max_frame_size).await),
            }
        };
        let frame = tokio::select! {
            res = read => res,
            _ = closed.notified() => break "connection closed".into(),
            _ = shutdown.changed() => {
                handle.close_after(vec![Command::ShutdownInfo(Header::default())]);
                break "broker shutdown".into();
            }
        };
        timeout = inactivity;
        let body = match frame {
            Err(_) => break format!("no data received for {} ms (inactivity timeout)", neg.max_inactivity_ms),
            Ok(Ok(None)) => break "connection closed by the client".into(),
            Ok(Err(e)) => {
                if e.kind() == std::io::ErrorKind::InvalidData {
                    tracing::warn!("{remote}: {e}");
                }
                break format!("{e}");
            }
            Ok(Ok(Some(b))) => b,
        };
        match codec.decode(body) {
            Ok(Some(cmd)) => conn.handle_command(cmd).await,
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("{remote}: protocol error: {e}");
                break format!("protocol error: {e}");
            }
        }
    };

    // 4. Cleanup.
    conn.cleanup();
    broker.unregister_conn(id);
    if conn.authenticated {
        tracing::info!("connection closed: {remote} user={} ({reason})", conn.user);
    } else {
        tracing::debug!("connection closed: {remote} ({reason})");
    }
    drop(conn);
    drop(handle);
    let _ = tokio::time::timeout(Duration::from_secs(5), writer).await;
}

impl Conn {
    fn reply(&self, header: Header, result: Result<(), (&'static str, String)>) {
        if !header.response_required {
            if let Err((class, msg)) = result {
                tracing::debug!("{}: {class}: {msg}", self.remote);
            }
            return;
        }
        let cmd = match result {
            Ok(()) => Command::response(header.command_id),
            Err((class, msg)) => Command::exception(header.command_id, class, msg),
        };
        self.handle.send(cmd);
    }

    async fn handle_command(&mut self, cmd: Command) {
        let header = cmd.header();
        if !self.authenticated {
            match &cmd {
                Command::ConnectionInfo(_) | Command::KeepAliveInfo(_) | Command::ShutdownInfo(_) => {}
                Command::WireFormatInfo(_) => return,
                _ => {
                    self.reply(header, Err((SECURITY_EXCEPTION, "Not authenticated".into())));
                    return;
                }
            }
        }
        match cmd {
            Command::ConnectionInfo(ci) => self.on_connection_info(ci),
            Command::SessionInfo(si) => {
                if let Some(s) = si.session_id {
                    self.sessions.insert(s);
                }
                self.update_info();
                self.reply(header, Ok(()));
            }
            Command::ConsumerInfo(ci) => {
                let r = self.on_consumer_info(ci);
                self.update_info();
                self.reply(header, r);
            }
            Command::ProducerInfo(pi) => {
                let r = self.on_producer_info(pi);
                self.update_info();
                self.reply(header, r);
            }
            Command::Message(m) => self.on_message(*m).await,
            Command::MessageAck(ack) => {
                if !header.response_required && matches!(ack.transaction_id, Some(TransactionId::Xa { .. })) {
                    tracing::warn!("{}: XA acknowledgement discarded: {XA_UNSUPPORTED}", self.remote);
                }
                let r = self.on_ack(ack);
                self.reply(header, r);
            }
            Command::MessagePull(p) => {
                self.on_pull(p);
                self.reply(header, Ok(()));
            }
            Command::TransactionInfo(ti) => {
                let xa = matches!(ti.transaction_id, Some(TransactionId::Xa { .. }));
                let r = self.on_transaction(ti);
                if !header.response_required {
                    if let Err((class, msg)) = &r {
                        if xa || msg == XA_UNSUPPORTED {
                            tracing::warn!("{}: XA transaction command discarded: {class}: {msg}", self.remote);
                        }
                    }
                }
                self.reply(header, r);
            }
            Command::DestinationInfo(di) => {
                let r = self.on_destination_info(di);
                self.reply(header, r);
            }
            Command::RemoveInfo(ri) => {
                let r = self.on_remove(ri);
                self.update_info();
                self.reply(header, r);
            }
            Command::RemoveSubscriptionInfo(_) => {
                self.reply(header, Err((JMS_EXCEPTION, "Durable subscriptions are not supported".into())));
            }
            Command::KeepAliveInfo(h) => {
                if h.response_required {
                    self.handle.send(Command::KeepAliveInfo(Header::default()));
                }
            }
            Command::ShutdownInfo(_) => {
                self.closing = true;
            }
            Command::ConsumerControl(cc) => {
                if let Some(cid) = &cc.consumer_id {
                    if let Some(Some(d)) = self.consumers.get(cid).map(|c| c.dest.clone()) {
                        d.set_prefetch(cid, cc.prefetch, now_ms());
                    }
                }
                self.reply(header, Ok(()));
            }
            Command::WireFormatInfo(_) => {}
            Command::ConnectionError(_)
            | Command::Response { .. }
            | Command::ExceptionResponse { .. }
            | Command::IntegerResponse { .. }
            | Command::MessageDispatch(_)
            | Command::BrokerInfo(_)
            | Command::ProducerAck(_) => {}
            Command::ControlCommand(_) | Command::FlushCommand(_) | Command::ConnectionControl(_) => {
                self.reply(header, Ok(()));
            }
            Command::Unsupported { type_code, header } => {
                tracing::debug!("{}: unsupported command type {type_code} ({})", self.remote, t::command_name(type_code));
                self.reply(header, Err((UNSUPPORTED, format!("Unsupported command: {}", t::command_name(type_code)))));
            }
        }
    }

    fn update_info(&self) {
        let mut info = self.handle.info.lock();
        info.sessions = self.sessions.len();
        info.consumers = self.consumers.values().filter(|c| c.dest.is_some()).count();
        info.producers = self.producers.len();
    }

    fn on_connection_info(&mut self, ci: ConnectionInfo) {
        let header = ci.header;
        if self.authenticated {
            self.reply(header, Ok(()));
            return;
        }
        let user = ci.user_name.clone().unwrap_or_default();
        match self.broker.auth.login(ci.user_name.as_deref(), ci.password.as_deref()) {
            Login::Accepted => {
                self.authenticated = true;
                self.user = if user.is_empty() { "(anonymous)".into() } else { user };
                self.connection_id = ci.connection_id.clone();
                {
                    let mut info = self.handle.info.lock();
                    info.connection_id = ci.connection_id.as_ref().map(|c| c.to_string()).unwrap_or_default();
                    info.client_id = ci.client_id.clone().unwrap_or_default();
                    info.user = self.user.clone();
                }
                tracing::info!("connection opened: {} user={} ({})", self.remote, self.user,
                    ci.connection_id.as_ref().map(|c| c.to_string()).unwrap_or_default());
                self.reply(header, Ok(()));
            }
            Login::Rejected => {
                tracing::warn!("login failed: {} user={}", self.remote, user);
                let msg = format!("User name [{}] or password is invalid.", user);
                let mut cmds = Vec::new();
                if header.response_required {
                    cmds.push(Command::exception(header.command_id, SECURITY_EXCEPTION, msg));
                } else {
                    cmds.push(Command::ConnectionError(ConnectionError {
                        header: Header::default(),
                        exception: Some(Throwable::new(SECURITY_EXCEPTION, msg)),
                        connection_id: ci.connection_id.clone(),
                    }));
                }
                self.handle.close_after(cmds);
                self.closing = true;
            }
        }
    }

    /// Checks a destination used by a producer or consumer.
    fn check_destination(&self, d: &Destination, consuming: bool) -> Result<(), (&'static str, String)> {
        if d.is_wildcard() {
            return Err((INVALID_DESTINATION, format!("Wildcard destinations are not supported: {d}")));
        }
        if d.is_composite() {
            return Err((INVALID_DESTINATION, format!("Composite destinations are not supported: {d}")));
        }
        if d.kind.is_temporary() {
            match self.broker.get_dest(d) {
                None => return Err((INVALID_DESTINATION, format!("Cannot use a deleted Destination: {d}"))),
                Some(x) if consuming && x.owner != Some(self.handle.id) => {
                    return Err((
                        INVALID_DESTINATION,
                        format!("Cannot consume from a temporary destination of another connection: {d}"),
                    ))
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    fn on_consumer_info(&mut self, ci: ConsumerInfo) -> Result<(), (&'static str, String)> {
        let Some(cid) = ci.consumer_id.clone() else {
            return Err((JMS_EXCEPTION, "ConsumerInfo without consumerId".into()));
        };
        let Some(dest) = ci.destination.clone() else {
            return Err((INVALID_DESTINATION, "Consumer has no destination".into()));
        };
        let session = Some(SessionId::of_consumer(&cid));
        // Advisory topics (including the driver's default composite advisory consumer) are accepted silently.
        if dest.kind.is_topic() && dest.name.split(',').all(|n| n.starts_with("ActiveMQ.Advisory.")) {
            self.broker.add_advisory_sub(self.handle.clone(), cid.clone(), dest);
            self.consumers.insert(cid, ConsumerReg { dest: None, session });
            return Ok(());
        }
        self.check_destination(&dest, true)?;
        if dest.kind.is_topic() && ci.subscription_name.is_some() {
            return Err((JMS_EXCEPTION, "Durable subscriptions are not supported".into()));
        }
        let selector = match ci.selector.as_deref() {
            Some(text) => match Selector::compile(text) {
                Ok(s) => s.map(Arc::new),
                Err(e) => return Err((INVALID_SELECTOR, e.exception_message(text))),
            },
            None => None,
        };
        let d = self.broker.get_or_create(&dest, None);
        d.add_sub(
            SubSpec {
                id: cid.clone(),
                conn: self.handle.clone(),
                prefetch: ci.prefetch_size,
                selector,
                no_local: ci.no_local,
                browser: ci.browser,
            },
            now_ms(),
        );
        self.consumers.insert(cid, ConsumerReg { dest: Some(d), session });
        Ok(())
    }

    fn on_producer_info(&mut self, pi: ProducerInfo) -> Result<(), (&'static str, String)> {
        let Some(pid) = pi.producer_id.clone() else {
            return Err((JMS_EXCEPTION, "ProducerInfo without producerId".into()));
        };
        let session = Some(SessionId::of_producer(&pid));
        let dest = match &pi.destination {
            Some(d) if d.is_advisory() => None,
            Some(d) => {
                self.check_destination(d, false)?;
                let x = self.broker.get_or_create(d, None);
                x.add_producer(
                    pid.clone(),
                    ProducerMeta {
                        conn_id: self.handle.id,
                        remote: self.remote.to_string(),
                        connection_id: self.connection_id.as_ref().map(|c| c.to_string()).unwrap_or_default(),
                    },
                );
                Some(x)
            }
            None => None,
        };
        self.producers.insert(pid, ProducerReg { dest, window: pi.window_size, session });
        Ok(())
    }

    async fn on_message(&mut self, mut msg: Message) {
        let header = msg.header;
        let sync = header.response_required;
        // Size as the client computes it (Message.getSize), for ProducerAck.
        let client_size = 1024 + msg.content_len() as i32 + msg.properties_len() as i32;
        let producer_window = msg
            .producer_id
            .as_ref()
            .and_then(|p| self.producers.get(p))
            .map(|p| p.window)
            .unwrap_or(0);
        // Asynchronous messages that can never be accepted deserve a warning: a wildcard or composite
        // destination, an XA transaction or a transaction that is not open.
        let serious = msg.destination.as_ref().is_some_and(|d| d.is_wildcard() || d.is_composite())
            || match &msg.transaction_id {
                Some(TransactionId::Xa { .. }) => true,
                Some(txid) => !self.txs.contains_key(txid),
                None => false,
            };
        let transacted = msg.transaction_id.is_some();
        let result = self.accept_message(&mut msg, sync).await;
        if !sync {
            if let Err((class, m)) = &result {
                if serious || transacted {
                    tracing::warn!("{}: asynchronous message {} dropped: {class}: {m}", self.remote, msg.message_id_text());
                } else {
                    tracing::debug!("{}: asynchronous message {} dropped: {class}: {m}", self.remote, msg.message_id_text());
                }
            }
            if producer_window > 0 {
                self.handle.send(Command::ProducerAck(ProducerAck {
                    header: Header::default(),
                    producer_id: msg.producer_id.clone(),
                    size: client_size,
                }));
            }
        }
        self.reply(header, result);
    }

    async fn accept_message(&mut self, msg: &mut Message, sync: bool) -> Result<(), (&'static str, String)> {
        let Some(dest) = msg.destination.clone() else {
            return Err((INVALID_DESTINATION, "Message has no destination".into()));
        };
        if dest.is_advisory() {
            return Ok(());
        }
        self.check_destination(&dest, false)?;
        if matches!(msg.transaction_id, Some(TransactionId::Xa { .. })) {
            return Err((JMS_EXCEPTION, XA_UNSUPPORTED.into()));
        }
        let now = now_ms();
        if !self.broker.apply_expiry_options(msg, now) {
            // A missing temporary destination is refused by `target` and counts nothing.
            let d = self.broker.target(&dest).map_err(rejection)?;
            self.broker.expire_before_storing(&d, msg);
            return Ok(());
        }
        // Broker-side compression (heavy bodies off the async runtime, order preserved by awaiting).
        if self.broker.compression_is_heavy(msg) {
            let broker = self.broker.clone();
            let owned = std::mem::replace(msg, Message::new(msg.msg_type));
            *msg = tokio::task::spawn_blocking(move || {
                let mut m = owned;
                broker.compress(&mut m);
                m
            })
            .await
            .expect("compression task");
        } else {
            self.broker.compress(msg);
        }
        match msg.transaction_id.clone() {
            Some(txid) => {
                if !self.txs.contains_key(&txid) {
                    return Err((JMS_EXCEPTION, format!("Transaction '{}' has not been started.", tx_text(&txid))));
                }
                // The destination exists from the send on; the memory limit applies now, not at commit.
                let d = self.broker.target(&dest).map_err(rejection)?;
                let size = msg.content_len() as u64 + msg.properties_len() as u64 + crate::broker::entry::ENTRY_OVERHEAD;
                self.broker.check_memory(size, sync, &dest, Some(&d)).map_err(rejection)?;
                let ticket = MemTicket::new(self.broker.memory.clone(), size);
                if let Some(tx) = self.txs.get_mut(&txid) {
                    tx.sends.push((msg.clone(), ticket));
                }
                Ok(())
            }
            None => self.broker.deliver(msg.clone(), sync, now).map_err(rejection),
        }
    }

    fn on_ack(&mut self, ack: MessageAck) -> Result<(), (&'static str, String)> {
        if matches!(ack.transaction_id, Some(TransactionId::Xa { .. })) {
            return Err((JMS_EXCEPTION, XA_UNSUPPORTED.into()));
        }
        let Some(cid) = &ack.consumer_id else {
            tracing::debug!("{}: ack without consumer ignored", self.remote);
            return Ok(());
        };
        let Some(Some(d)) = self.consumers.get(cid).map(|c| c.dest.clone()) else {
            tracing::debug!("{}: ack for unknown consumer {cid} ignored", self.remote);
            return Ok(());
        };
        let now = now_ms();
        // Only acks that consume are deferred to the commit; DELIVERED, REDELIVERED and EXPIRED apply at once.
        let deferred = matches!(
            ack.ack_type,
            ack_type::STANDARD | ack_type::INDIVIDUAL | ack_type::UNMATCHED | ack_type::POISON
        );
        match &ack.transaction_id {
            Some(txid) if deferred => {
                let Some(tx) = self.txs.get_mut(txid) else {
                    return Err((JMS_EXCEPTION, format!("Transaction '{}' has not been started.", tx_text(txid))));
                };
                tx.acks.push((d.clone(), ack.clone()));
                d.ack(&ack, true, now);
                Ok(())
            }
            _ => {
                let effects = d.ack(&ack, false, now);
                self.broker.run_effects(effects, now);
                Ok(())
            }
        }
    }

    fn on_pull(&mut self, p: MessagePull) {
        let Some(cid) = p.consumer_id.clone() else { return };
        let Some(Some(d)) = self.consumers.get(&cid).map(|c| c.dest.clone()) else { return };
        if let Some(generation) = d.pull(&cid, p.timeout, now_ms()) {
            let timeout = Duration::from_millis(p.timeout.max(0) as u64);
            tokio::spawn(async move {
                tokio::time::sleep(timeout).await;
                d.pull_timeout(&cid, generation);
            });
        }
    }

    fn on_transaction(&mut self, ti: TransactionInfo) -> Result<(), (&'static str, String)> {
        let Some(txid) = ti.transaction_id.clone() else {
            return Err((JMS_EXCEPTION, "TransactionInfo without transactionId".into()));
        };
        if matches!(txid, TransactionId::Xa { .. }) {
            return Err((JMS_EXCEPTION, XA_UNSUPPORTED.into()));
        }
        match ti.tx_type {
            tx_type::BEGIN => {
                self.txs.entry(txid).or_default();
                Ok(())
            }
            tx_type::COMMIT_ONE_PHASE => {
                let Some(tx) = self.txs.remove(&txid) else {
                    return Err((JMS_EXCEPTION, format!("Transaction '{}' has not been started.", tx_text(&txid))));
                };
                let now = now_ms();
                for (mut m, ticket) in tx.sends {
                    m.transaction_id = None;
                    drop(ticket);
                    if m.expiration > 0 && m.expiration <= now {
                        if let Some(d) = m.destination.as_ref().and_then(|d| self.broker.target(d).ok()) {
                            self.broker.expire_before_storing(&d, &m);
                        }
                        continue;
                    }
                    if let Err(r) = self.broker.deliver_checked(m, true, now, false) {
                        let crate::broker::Rejection::Error { class, message } = r;
                        tracing::debug!("{}: committed message not stored: {class}: {message}", self.remote);
                    }
                }
                for (d, mut ack) in tx.acks {
                    ack.transaction_id = None;
                    let effects = d.ack(&ack, false, now);
                    self.broker.run_effects(effects, now);
                }
                Ok(())
            }
            tx_type::ROLLBACK => {
                let Some(tx) = self.txs.remove(&txid) else {
                    return Err((JMS_EXCEPTION, format!("Transaction '{}' has not been started.", tx_text(&txid))));
                };
                release_tx(tx);
                Ok(())
            }
            tx_type::END | tx_type::FORGET => Ok(()),
            _ => Err((JMS_EXCEPTION, XA_UNSUPPORTED.into())),
        }
    }

    fn on_destination_info(&mut self, di: DestinationInfo) -> Result<(), (&'static str, String)> {
        let Some(d) = di.destination.clone() else {
            return Err((INVALID_DESTINATION, "DestinationInfo without destination".into()));
        };
        match di.operation_type {
            dest_op::ADD => {
                if d.is_wildcard() || d.is_composite() {
                    return Err((INVALID_DESTINATION, format!("Unsupported destination: {d}")));
                }
                let owner = d.kind.is_temporary().then_some(self.handle.id);
                let existed = self.broker.get_dest(&d).is_some();
                self.broker.get_or_create(&d, owner);
                if d.kind.is_temporary() && !existed {
                    self.broker.temp_advisory(&d, dest_op::ADD);
                }
                Ok(())
            }
            dest_op::REMOVE if d.is_wildcard() || d.is_composite() => {
                Err((INVALID_DESTINATION, format!("Unsupported destination: {d}")))
            }
            dest_op::REMOVE => match self.broker.delete_dest(&d) {
                Ok(_) => Ok(()),
                Err(e) => Err((JMS_EXCEPTION, e)),
            },
            other => Err((JMS_EXCEPTION, format!("Unknown DestinationInfo operation {other}"))),
        }
    }

    fn remove_consumer(&mut self, cid: &ConsumerId, last_delivered: i64) {
        if let Some(reg) = self.consumers.remove(cid) {
            match reg.dest {
                Some(d) => {
                    d.remove_sub(cid, last_delivered, now_ms());
                }
                None => self.broker.remove_advisory_sub(cid),
            }
        }
    }

    fn remove_producer(&mut self, pid: &ProducerId) {
        if let Some(reg) = self.producers.remove(pid) {
            if let Some(d) = reg.dest {
                d.remove_producer(pid);
            }
        }
    }

    fn on_remove(&mut self, ri: RemoveInfo) -> Result<(), (&'static str, String)> {
        match ri.object_id {
            Some(DataStructure::ConsumerId(cid)) => self.remove_consumer(&cid, ri.last_delivered_sequence_id),
            Some(DataStructure::ProducerId(pid)) => self.remove_producer(&pid),
            Some(DataStructure::SessionId(sid)) => {
                let consumers: Vec<ConsumerId> = self
                    .consumers
                    .iter()
                    .filter(|(_, r)| r.session.as_ref() == Some(&sid))
                    .map(|(k, _)| k.clone())
                    .collect();
                for c in consumers {
                    self.remove_consumer(&c, ri.last_delivered_sequence_id);
                }
                let producers: Vec<ProducerId> = self
                    .producers
                    .iter()
                    .filter(|(_, r)| r.session.as_ref() == Some(&sid))
                    .map(|(k, _)| k.clone())
                    .collect();
                for p in producers {
                    self.remove_producer(&p);
                }
                self.sessions.remove(&sid);
            }
            Some(DataStructure::ConnectionId(_)) => {
                self.cleanup();
            }
            _ => {}
        }
        Ok(())
    }

    /// Releases everything the connection owns.
    fn cleanup(&mut self) {
        let consumers: Vec<ConsumerId> = self.consumers.keys().cloned().collect();
        for c in consumers {
            self.remove_consumer(&c, -1);
        }
        let producers: Vec<ProducerId> = self.producers.keys().cloned().collect();
        for p in producers {
            self.remove_producer(&p);
        }
        for (_, tx) in self.txs.drain() {
            release_tx(tx);
        }
        self.sessions.clear();
        self.broker.remove_advisory_conn(self.handle.id);
        self.broker.drop_temp_destinations(self.handle.id);
        if let Some(cid) = &self.connection_id {
            self.broker.release_producer_audits(&cid.value);
        }
        self.update_info();
    }
}

/// Rolls back the acks of a transaction: messages reserved for closed consumers return to their queue.
fn release_tx(tx: Tx) {
    let now = now_ms();
    let mut done = HashSet::new();
    for (d, ack) in tx.acks {
        if let Some(cid) = ack.consumer_id {
            if done.insert((d.dest.clone(), cid.clone())) {
                d.release_reserved(&cid, now);
            }
        }
    }
}

fn rejection(r: crate::broker::Rejection) -> (&'static str, String) {
    match r {
        crate::broker::Rejection::Error { class, message } => (class, message),
    }
}

fn tx_text(t: &TransactionId) -> String {
    match t {
        TransactionId::Local { value, connection_id } => format!(
            "TX:{}:{}",
            connection_id.as_ref().map(|c| c.to_string()).unwrap_or_default(),
            value
        ),
        TransactionId::Xa { format_id, .. } => format!("XID:[{format_id}]"),
    }
}
