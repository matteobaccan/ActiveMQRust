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
