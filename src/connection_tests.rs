// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Tests of the connection I/O path: frame reader, writer batching, vectored writes,
//! pluggable codec and isolation from a blocked consumer socket.

use super::*;
use crate::broker::destination::SubSpec;
use crate::config::{build, ConfigSource, FileConfig, Overrides};
use crate::openwire::codec::CodecResult;
use crate::openwire::marshal::Encoder;
use crate::openwire::props::{PrimitiveMap, Value};
use parking_lot::Mutex as PlMutex;
use std::io::IoSlice;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

/// One recorded vectored write: (address, length) of every slice.
type Call = Vec<(usize, usize)>;

/// A writer that accepts everything and records each write call.
#[derive(Clone, Default)]
struct RecordingWriter {
    calls: Arc<PlMutex<Vec<Call>>>,
    data: Arc<PlMutex<Vec<u8>>>,
}

impl AsyncWrite for RecordingWriter {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        self.calls.lock().push(vec![(buf.as_ptr() as usize, buf.len())]);
        self.data.lock().extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_write_vectored(self: Pin<&mut Self>, _: &mut Context<'_>, bufs: &[IoSlice<'_>]) -> Poll<std::io::Result<usize>> {
        self.calls.lock().push(bufs.iter().map(|b| (b.as_ptr() as usize, b.len())).collect());
        let mut data = self.data.lock();
        let mut n = 0;
        for b in bufs {
            data.extend_from_slice(b);
            n += b.len();
        }
        Poll::Ready(Ok(n))
    }

    fn is_write_vectored(&self) -> bool {
        true
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// A socket whose peer never reads: every write stays pending.
struct BlockedWriter {
    polled: Arc<AtomicUsize>,
}

impl AsyncWrite for BlockedWriter {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<std::io::Result<usize>> {
        self.polled.fetch_add(1, Ordering::Relaxed);
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }
}

fn pid() -> ProducerId {
    ProducerId { connection_id: Arc::from("ID:test-1-1-1:1"), session_id: 1, value: 1 }
}

fn text_message(dest: &Destination, seq: i64, body: Bytes) -> Message {
    let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
    m.producer_id = Some(pid());
    m.destination = Some(dest.clone());
    m.message_id = Some(MessageId { text_view: None, producer_id: Some(pid()), producer_sequence_id: seq, broker_sequence_id: seq });
    m.content = Some(body);
    m
}

fn dispatch(seq: i64, body: Bytes) -> Out {
    let q = Destination::queue("W");
    Out::Cmd(Command::MessageDispatch(MessageDispatch {
        header: Header::default(),
        consumer_id: Some(ConsumerId { connection_id: Arc::from("c"), session_id: 1, value: 1 }),
        destination: Some(q.clone()),
        message: Some(Arc::new(text_message(&q, seq, body))),
        redelivery_counter: 0,
    }))
}

fn split_frames(data: &[u8]) -> Vec<Bytes> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= data.len() {
        let size = i32::from_be_bytes(data[i..i + 4].try_into().unwrap()) as usize;
        out.push(Bytes::copy_from_slice(&data[i + 4..i + 4 + size]));
        i += 4 + size;
    }
    assert_eq!(i, data.len(), "partial frame written");
    out
}

#[tokio::test]
async fn batching_under_load() {
    let (tx, rx) = mpsc::unbounded_channel();
    for i in 0..1000 {
        tx.send(dispatch(i, Bytes::from(vec![b'x'; 1024]))).unwrap();
    }
    drop(tx);
    let w = RecordingWriter::default();
    writer_task(w.clone(), rx, loose_codec(12), None, Arc::new(Notify::new())).await;
    let calls = w.calls.lock().len();
    assert!(calls <= 16, "1,000 queued dispatches written with {calls} write calls");
    let frames = split_frames(&w.data.lock());
    assert_eq!(frames.len(), 1000);
    let dec = crate::openwire::marshal::Decoder::new(12);
    for (i, f) in frames.into_iter().enumerate() {
        match dec.decode_frame(f).unwrap() {
            Some(Command::MessageDispatch(md)) => {
                assert_eq!(md.message.unwrap().message_id.as_ref().unwrap().producer_sequence_id, i as i64)
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[tokio::test]
async fn large_body_is_a_separate_slice() {
    let body = Bytes::from(vec![7u8; 1024 * 1024]);
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(dispatch(1, body.clone())).unwrap();
    drop(tx);
    let w = RecordingWriter::default();
    writer_task(w.clone(), rx, loose_codec(12), None, Arc::new(Notify::new())).await;
    let calls = w.calls.lock().clone();
    let found = calls.iter().flatten().any(|&(addr, len)| addr == body.as_ptr() as usize && len == body.len());
    assert!(found, "the body was not written from the stored buffer: {calls:?}");
    let frames = split_frames(&w.data.lock());
    match crate::openwire::marshal::Decoder::new(12).decode_frame(frames[0].clone()).unwrap() {
        Some(Command::MessageDispatch(md)) => assert_eq!(md.message.unwrap().content.as_ref().unwrap(), &body),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn single_frame_is_written_without_delay() {
    let (tx, rx) = mpsc::unbounded_channel();
    let w = RecordingWriter::default();
    let task = tokio::spawn(writer_task(w.clone(), rx, loose_codec(12), None, Arc::new(Notify::new())));
    tx.send(dispatch(1, Bytes::from_static(b"hello"))).unwrap();
    // The writer only needs to be scheduled: no timer has to fire (time never advances here).
    let start = tokio::time::Instant::now();
    for _ in 0..100 {
        if !w.calls.lock().is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(w.calls.lock().len(), 1, "a single queued frame was not written at once");
    assert!(start.elapsed() < Duration::from_millis(500));
    drop(tx);
    task.await.unwrap();
}

/// Builds the bytes of a TCP stream carrying the given frames.
fn stream_of(frames: &[Bytes]) -> Vec<u8> {
    frames.iter().flat_map(|f| f.iter().copied()).collect()
}

#[tokio::test]
async fn reader_splits_frames_and_keeps_bodies_in_their_frame() {
    let enc = Encoder::new(12);
    let q = Destination::queue("R");
    let small = Command::Message(Box::new(text_message(&q, 1, Bytes::from_static(b"\x00\x00\x00\x02hi"))));
    let ack = Command::KeepAliveInfo(Header::default());
    let big_body = Bytes::from(vec![3u8; 1024 * 1024]);
    let big = Command::Message(Box::new(text_message(&q, 2, big_body.clone())));
    let data = stream_of(&[enc.frame(&small), enc.frame(&ack), enc.frame(&big), enc.frame(&small)]);
    // A slow reader: the stream arrives in pieces of 1,000 bytes.
    let (mut client, server) = tokio::io::duplex(1000);
    let writer = tokio::spawn(async move {
        client.write_all(&data).await.unwrap();
    });
    let mut r = FrameReader::new(server);
    let dec = crate::openwire::marshal::Decoder::new(12);
    let mut kinds = Vec::new();
    while let Some(frame) = r.next(i64::MAX).await.unwrap() {
        let range = frame.as_ptr() as usize..frame.as_ptr() as usize + frame.len();
        let cmd = dec.decode_frame(frame.clone()).unwrap().unwrap();
        if let Command::Message(m) = &cmd {
            let c = m.content.as_ref().unwrap();
            assert!(range.contains(&(c.as_ptr() as usize)), "content was copied out of the frame");
            if c.len() == big_body.len() {
                assert_eq!(c, &big_body);
            }
        }
        kinds.push(cmd.type_code());
    }
    writer.await.unwrap();
    assert_eq!(kinds, vec![t::ACTIVEMQ_TEXT_MESSAGE, t::KEEP_ALIVE_INFO, t::ACTIVEMQ_TEXT_MESSAGE, t::ACTIVEMQ_TEXT_MESSAGE]);
}

#[tokio::test]
async fn reader_rejects_oversized_frames_and_truncated_streams() {
    let mut data = (2000i32).to_be_bytes().to_vec();
    data.extend_from_slice(&[0u8; 10]);
    let mut r = FrameReader::new(&data[..]);
    assert_eq!(r.next(1000).await.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    let mut r = FrameReader::new(&data[..]);
    assert_eq!(r.next(10_000).await.unwrap_err().kind(), std::io::ErrorKind::UnexpectedEof);
    let empty: &[u8] = &[];
    assert!(FrameReader::new(empty).next(10).await.unwrap().is_none());
}

fn broker() -> Arc<Broker> {
    Broker::new(Arc::new(build(FileConfig::default(), ConfigSource::Defaults, &Overrides::default()).unwrap()))
}

#[tokio::test]
async fn blocked_consumer_socket_does_not_stall_the_queue() {
    let broker = broker();
    let q = Destination::queue("BLOCKED");
    let d = broker.get_or_create(&q, None);
    // Consumer A: its writer task is stuck on a socket the client does not read.
    let (tx_a, rx_a) = mpsc::unbounded_channel();
    let a = Arc::new(ConnHandle::new(broker.new_conn_id(), "127.0.0.1:1".parse().unwrap(), tx_a));
    let polled = Arc::new(AtomicUsize::new(0));
    let blocked = BlockedWriter { polled: polled.clone() };
    let writer = tokio::spawn(writer_task(blocked, rx_a, loose_codec(12), None, Arc::new(Notify::new())));
    // Consumer B: reads normally.
    let (tx_b, mut rx_b) = mpsc::unbounded_channel();
    let b = Arc::new(ConnHandle::new(broker.new_conn_id(), "127.0.0.1:2".parse().unwrap(), tx_b));
    for (n, conn) in [(1, a), (2, b)] {
        d.add_sub(
            SubSpec {
                id: ConsumerId { connection_id: Arc::from("c"), session_id: 1, value: n },
                conn,
                prefetch: 100,
                selector: None,
                no_local: false,
                browser: false,
            },
            now_ms(),
        );
    }
    let mut received = 0;
    for i in 0..2000 {
        broker.deliver(text_message(&q, i + 1, Bytes::from(vec![b'm'; 1024])), true, now_ms()).unwrap();
        while let Ok(Out::Cmd(Command::MessageDispatch(md))) = rx_b.try_recv() {
            let m = md.message.unwrap();
            let ack = MessageAck {
                header: Header::default(),
                destination: Some(q.clone()),
                transaction_id: None,
                consumer_id: md.consumer_id.clone(),
                ack_type: ack_type::STANDARD,
                first_message_id: None,
                last_message_id: m.message_id.clone(),
                message_count: 1,
                poison_cause: None,
            };
            d.ack(&ack, false, now_ms());
            received += 1;
        }
        if i == 500 {
            tokio::task::yield_now().await;
        }
    }
    assert!(polled.load(Ordering::Relaxed) > 0, "the blocked writer never tried to write");
    assert_eq!(received, 2000 - 100, "consumer B must get everything beyond A's prefetch");
    writer.abort();
}

static DECODED: AtomicUsize = AtomicUsize::new(0);
static ENCODED: AtomicUsize = AtomicUsize::new(0);

/// A codec that counts every frame and delegates to loose encoding.
struct CountingCodec(LooseCodec);

impl WireCodec for CountingCodec {
    fn decode(&self, body: Bytes) -> CodecResult<Option<Command>> {
        DECODED.fetch_add(1, Ordering::Relaxed);
        self.0.decode(body)
    }

    fn encode(&self, cmd: &Command, out: &mut ChunkBuf) {
        ENCODED.fetch_add(1, Ordering::Relaxed);
        self.0.encode(cmd, out)
    }
}

fn counting_codec(version: i32) -> Box<dyn WireCodec> {
    Box::new(CountingCodec(LooseCodec::new(version)))
}

#[tokio::test]
async fn codec_is_pluggable() {
    let broker = broker();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (_stop, shutdown) = watch::channel(false);
    let b = broker.clone();
    tokio::spawn(async move {
        let (s, remote) = listener.accept().await.unwrap();
        serve_with_codec(s, remote, b, shutdown, counting_codec).await;
    });
    let stream = TcpStream::connect(addr).await.unwrap();
    let (r, mut w) = stream.into_split();
    let mut r = FrameReader::new(r);
    let enc = Encoder::new(12);
    let dec = crate::openwire::marshal::Decoder::new(12);
    let mut props = PrimitiveMap::new();
    props.set("MaxInactivityDuration", Value::Long(0));
    let wf = WireFormatInfo { magic: wireformat::MAGIC, version: 12, properties: props };
    let conn_id = ConnectionId { value: Arc::from("ID:codec-test-1") };
    let session = SessionId { connection_id: conn_id.value.clone(), value: 1 };
    let consumer = ConsumerId { connection_id: conn_id.value.clone(), session_id: 1, value: 1 };
    let q = Destination::queue("CODEC.PLUG");
    let mut msg = text_message(&q, 1, Bytes::from_static(b"\x00\x00\x00\x05hello"));
    msg.header = Header { command_id: 5, response_required: true };
    let sent = [
        Command::WireFormatInfo(wf),
        Command::ConnectionInfo(ConnectionInfo {
            header: Header { command_id: 1, response_required: true },
            connection_id: Some(conn_id.clone()),
            client_id: Some("codec-test".into()),
            password: Some("admin".into()),
            user_name: Some("admin".into()),
            broker_path: None,
            broker_master_connector: false,
            manageable: false,
            client_master: true,
            fault_tolerant: false,
            failover_reconnect: false,
            client_ip: None,
        }),
        Command::SessionInfo(SessionInfo { header: Header { command_id: 2, response_required: false }, session_id: Some(session) }),
        Command::ConsumerInfo(ConsumerInfo {
            header: Header { command_id: 3, response_required: true },
            consumer_id: Some(consumer),
            browser: false,
            destination: Some(q.clone()),
            prefetch_size: 10,
            maximum_pending_message_limit: 0,
            dispatch_async: true,
            selector: None,
            client_id: None,
            subscription_name: None,
            no_local: false,
            exclusive: false,
            retroactive: false,
            priority: 0,
            broker_path: None,
            additional_predicate: None,
            network_subscription: false,
            optimized_acknowledge: false,
            no_range_acks: false,
            network_consumer_path: None,
        }),
        Command::Message(Box::new(msg)),
    ];
    for c in &sent {
        w.write_all(&enc.frame(c)).await.unwrap();
    }
    let mut received = 0;
    let body = loop {
        let frame = tokio::time::timeout(Duration::from_secs(10), r.next(i64::MAX)).await.unwrap().unwrap().unwrap();
        received += 1;
        if let Some(Command::MessageDispatch(md)) = dec.decode_frame(frame).unwrap() {
            break md.message.unwrap().content.clone().unwrap();
        }
    };
    assert_eq!(&body[..], b"\x00\x00\x00\x05hello");
    // Every command after the client's WireFormatInfo went through the wrapper, and so did
    // every frame the broker wrote.
    assert_eq!(DECODED.load(Ordering::Relaxed), sent.len() - 1);
    assert!(ENCODED.load(Ordering::Relaxed) >= received, "{} encoded, {received} received", ENCODED.load(Ordering::Relaxed));
}

// -- broker-side compression over real connections -------------------------------------------

/// Starts a broker that serves every connection accepted on a loopback port.
/// A served broker with compression enabled above 32 KB (off by default).
async fn start_compressing_broker() -> (Arc<Broker>, SocketAddr, watch::Sender<bool>) {
    let mut fc = FileConfig::default();
    fc.broker.compress_threshold_kb = 32;
    let broker = Broker::new(Arc::new(build(fc, ConfigSource::Defaults, &Overrides::default()).unwrap()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, shutdown) = watch::channel(false);
    let b = broker.clone();
    tokio::spawn(async move {
        while let Ok((s, remote)) = listener.accept().await {
            tokio::spawn(serve(s, remote, b.clone(), shutdown.clone()));
        }
    });
    (broker, addr, stop)
}

/// A minimal OpenWire client (version 12) for tests.
struct TestClient {
    r: FrameReader<tokio::net::tcp::OwnedReadHalf>,
    w: tokio::net::tcp::OwnedWriteHalf,
    conn: Arc<str>,
    next_id: i32,
    seq: i64,
}

impl TestClient {
    async fn open(addr: SocketAddr, name: &str) -> TestClient {
        let (r, w) = TcpStream::connect(addr).await.unwrap().into_split();
        let mut c = TestClient { r: FrameReader::new(r), w, conn: Arc::from(name), next_id: 1, seq: 0 };
        let mut props = PrimitiveMap::new();
        props.set("MaxInactivityDuration", Value::Long(0));
        c.write(Command::WireFormatInfo(WireFormatInfo { magic: wireformat::MAGIC, version: 12, properties: props })).await;
        let info = ConnectionInfo {
            header: Header::default(),
            connection_id: Some(ConnectionId { value: c.conn.clone() }),
            client_id: Some(name.into()),
            password: Some("admin".into()),
            user_name: Some("admin".into()),
            broker_path: None,
            broker_master_connector: false,
            manageable: false,
            client_master: true,
            fault_tolerant: false,
            failover_reconnect: false,
            client_ip: None,
        };
        c.request(Command::ConnectionInfo(info)).await;
        let session = SessionId { connection_id: c.conn.clone(), value: 1 };
        c.request(Command::SessionInfo(SessionInfo { header: Header::default(), session_id: Some(session) })).await;
        c
    }

    async fn write(&mut self, cmd: Command) {
        self.w.write_all(&Encoder::new(12).frame(&cmd)).await.unwrap();
    }

    /// Sends a command with `responseRequired` set, without waiting for the response.
    async fn send_request(&mut self, mut cmd: Command) {
        let header = Header { command_id: self.next_id, response_required: true };
        self.next_id += 1;
        match &mut cmd {
            Command::ConnectionInfo(x) => x.header = header,
            Command::SessionInfo(x) => x.header = header,
            Command::ConsumerInfo(x) => x.header = header,
            Command::Message(x) => x.header = header,
            other => panic!("unexpected request {other:?}"),
        }
        self.write(cmd).await;
    }

    async fn wait_response(&mut self) -> Command {
        loop {
            match self.read(Duration::from_secs(60)).await {
                Some(c @ (Command::Response { .. } | Command::ExceptionResponse { .. })) => return c,
                Some(_) => {}
                None => panic!("no response"),
            }
        }
    }

    /// Sends a command with `responseRequired` and waits for its response.
    async fn request(&mut self, cmd: Command) -> Command {
        self.send_request(cmd).await;
        self.wait_response().await
    }

    async fn read(&mut self, timeout: Duration) -> Option<Command> {
        let frame = tokio::time::timeout(timeout, self.r.next(i64::MAX)).await.ok()?.unwrap()?;
        crate::openwire::marshal::Decoder::new(12).decode_frame(frame).unwrap()
    }

    async fn consume(&mut self, q: &Destination, prefetch: i32) {
        let ci = ConsumerInfo {
            header: Header::default(),
            consumer_id: Some(ConsumerId { connection_id: self.conn.clone(), session_id: 1, value: 1 }),
            browser: false,
            destination: Some(q.clone()),
            prefetch_size: prefetch,
            maximum_pending_message_limit: 0,
            dispatch_async: true,
            selector: None,
            client_id: None,
            subscription_name: None,
            no_local: false,
            exclusive: false,
            retroactive: false,
            priority: 0,
            broker_path: None,
            additional_predicate: None,
            network_subscription: false,
            optimized_acknowledge: false,
            no_range_acks: false,
            network_consumer_path: None,
        };
        self.request(Command::ConsumerInfo(ci)).await;
    }

    fn message(&mut self, q: &Destination, content: Bytes) -> Message {
        self.seq += 1;
        let pid = ProducerId { connection_id: self.conn.clone(), session_id: 1, value: 1 };
        let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
        m.producer_id = Some(pid.clone());
        m.destination = Some(q.clone());
        m.message_id = Some(MessageId { text_view: None, producer_id: Some(pid), producer_sequence_id: self.seq, broker_sequence_id: 0 });
        m.correlation_id = Some(format!("corr-{}", self.seq));
        m.jms_type = Some("compression-test".into());
        m.priority = 7;
        m.timestamp = 1_700_000_000_000;
        let mut p = PrimitiveMap::new();
        p.set("k", Value::Int(self.seq as i32));
        m.marshalled_properties = Some(p.encode());
        m.content = Some(content);
        m
    }

    /// Sends without waiting (asynchronous send).
    async fn send_async(&mut self, m: Message) {
        self.write(Command::Message(Box::new(m))).await;
    }

    async fn dispatch(&mut self, timeout: Duration) -> Option<Arc<Message>> {
        loop {
            if let Command::MessageDispatch(md) = self.read(timeout).await? {
                return md.message;
            }
        }
    }
}

/// A text message content (4-byte length + text) of `len` compressible bytes.
fn compressible(len: usize) -> Bytes {
    let unit = b"<order><id>42</id><status>shipped</status><note>compressible</note></order>";
    let mut v: Vec<u8> = unit.iter().copied().cycle().take(len).collect();
    v[..4].copy_from_slice(&((len - 4) as i32).to_be_bytes());
    Bytes::from(v)
}

#[tokio::test]
async fn large_compressed_message_keeps_fifo_and_headers() {
    let (broker, addr, _stop) = start_compressing_broker().await;
    let q = Destination::queue("ZIP.FIFO");
    let mut consumer = TestClient::open(addr, "ID:zip-consumer").await;
    consumer.consume(&q, 100).await;
    let mut producer = TestClient::open(addr, "ID:zip-producer").await;
    let big = compressible(5 * 1024 * 1024);
    let first = producer.message(&q, big.clone());
    let mut sent = vec![first.clone()];
    producer.send_async(first).await;
    for i in 0..10 {
        let m = producer.message(&q, Bytes::from(format!("\0\0\0\x06small{i}").into_bytes()));
        sent.push(m.clone());
        producer.send_async(m).await;
    }
    for (i, s) in sent.iter().enumerate() {
        let got = consumer.dispatch(Duration::from_secs(30)).await.expect("message not delivered");
        let id = |m: &Message| m.message_id.as_ref().map(|x| (x.producer_id.clone(), x.producer_sequence_id));
        assert_eq!(id(&got), id(s), "message {i} out of order or with a changed MessageId");
        if i == 0 {
            assert!(got.compressed, "the 5 MB body was not compressed");
            let content = got.content.as_ref().unwrap();
            assert!(content.len() < big.len() / 10);
            let back = crate::broker::compress::decompress_content(t::ACTIVEMQ_TEXT_MESSAGE, content, usize::MAX).unwrap();
            assert_eq!(&back[..], &big[..]);
        } else {
            assert!(!got.compressed);
            assert_eq!(got.content, s.content);
        }
        // Headers and properties are unchanged by compression.
        assert_eq!(got.producer_id, s.producer_id);
        assert_eq!(got.correlation_id, s.correlation_id);
        assert_eq!(got.jms_type, s.jms_type);
        assert_eq!(got.priority, s.priority);
        assert_eq!(got.timestamp, s.timestamp);
        assert_eq!(got.destination, s.destination);
        assert_eq!(got.marshalled_properties, s.marshalled_properties);
    }
    assert_eq!(broker.stats.compressed.load(Ordering::Relaxed), 1);
}

/// One runtime thread: if the 50 MB body were compressed on it, no other connection could
/// make progress between the end of its frame and its storage.
#[tokio::test(flavor = "current_thread")]
async fn other_connections_progress_during_a_large_compression() {
    let (broker, addr, _stop) = start_compressing_broker().await;
    let big_q = Destination::queue("ZIP.BIG");
    let small_q = Destination::queue("ZIP.SMALL");
    let mut other = TestClient::open(addr, "ID:zip-other").await;
    other.consume(&small_q, 1_000_000).await;
    let mut heavy = TestClient::open(addr, "ID:zip-heavy").await;
    let m = heavy.message(&big_q, compressible(50 * 1024 * 1024));
    let (written_tx, written_rx) = tokio::sync::oneshot::channel();
    let sender = tokio::spawn(async move {
        heavy.send_request(Command::Message(Box::new(m))).await;
        let _ = written_tx.send(());
        heavy.wait_response().await
    });
    written_rx.await.unwrap();
    let stored = |b: &Broker| b.get_dest(&big_q).map_or(0, |d| d.message_count()) > 0;
    let mut while_compressing = 0;
    while !stored(&broker) {
        let m = other.message(&small_q, Bytes::from_static(b"\0\0\0\x02hi"));
        other.send_async(m).await;
        other.dispatch(Duration::from_secs(10)).await.expect("small message not delivered");
        if !stored(&broker) {
            while_compressing += 1;
        }
    }
    assert!(matches!(sender.await.unwrap(), Command::Response { .. }));
    assert!(broker.get_dest(&big_q).unwrap().message_count() == 1);
    assert!(while_compressing >= 3, "only {while_compressing} round trips while the 50 MB body was compressed");
}
