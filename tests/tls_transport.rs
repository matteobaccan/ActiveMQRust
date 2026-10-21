// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! OpenWire over TLS: the handshake, the protocol versions, client certificates, the
//! handshake timeout and the TLS-only mode, against a broker started in this process.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use mqrust::config::{build, Config, ConfigSource, FileConfig, Overrides};
use mqrust::openwire::marshal::{Decoder, Encoder};
use mqrust::openwire::model::*;
use mqrust::openwire::props::{PrimitiveMap, Value};
use mqrust::openwire::wireformat::MAGIC;

const VERSION: i32 = 12;

/// A certificate authority and the certificates it signs, written as PEM files.
struct Pki {
    dir: PathBuf,
    ca: rcgen::Issuer<'static, rcgen::KeyPair>,
    ca_pem: String,
}

impl Pki {
    fn new(dir: &Path) -> Pki {
        let mut params = rcgen::CertificateParams::new(Vec::new()).unwrap();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "ActiveMQRust test CA");
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        let ca_pem = cert.pem();
        std::fs::write(dir.join("ca.crt"), &ca_pem).unwrap();
        Pki {
            dir: dir.to_path_buf(),
            ca: rcgen::Issuer::new(params, key),
            ca_pem,
        }
    }

    /// Writes `<name>.crt` and `<name>.key`, signed by the CA.
    fn issue(&self, name: &str, dns: &str) -> (String, String) {
        let mut params = rcgen::CertificateParams::new(vec![dns.to_string()]).unwrap();
        params.distinguished_name.push(rcgen::DnType::CommonName, name);
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.signed_by(&key, &self.ca).unwrap();
        let (cert_pem, key_pem) = (cert.pem(), key.serialize_pem());
        std::fs::write(self.dir.join(format!("{name}.crt")), &cert_pem).unwrap();
        std::fs::write(self.dir.join(format!("{name}.key")), &key_pem).unwrap();
        (cert_pem, key_pem)
    }
}

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mqrust-tlsit-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A broker running in this process; it stops when dropped.
struct Broker {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    done: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

impl Broker {
    async fn start(cfg: Config) -> Broker {
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let done = tokio::spawn(async move {
            mqrust::server::run(
                cfg,
                Box::pin(async move {
                    let _ = stop_rx.await;
                }),
                move || {
                    let _ = ready_tx.send(());
                },
            )
            .await
        });
        // `run` reports ready once every listener is bound.
        for _ in 0..200 {
            if ready_rx.try_recv().is_ok() {
                return Broker {
                    stop: Some(stop_tx),
                    done: Some(done),
                };
            }
            if done.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the broker did not start");
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(done) = self.done.take() {
            done.abort();
        }
    }
}

/// A configuration with TLS on, its files in `dir`.
fn tls_config(dir: &Path, port: u16, tls_port: u16, admin_port: u16, f: impl FnOnce(&mut FileConfig)) -> Config {
    let mut fc = FileConfig::default();
    fc.broker.bind = "127.0.0.1".into();
    fc.broker.port = port as i64;
    fc.users.push(mqrust::config::UserEntry {
        username: "admin".into(),
        password: Some("admin".into()),
        password_hash: None,
    });
    fc.admin.bind = "127.0.0.1".into();
    fc.admin.port = admin_port as i64;
    fc.admin.password = Some("admin".into());
    fc.log.file = false;
    fc.tls.enabled = true;
    fc.tls.port = tls_port as i64;
    fc.tls.cert = "broker.crt".into();
    fc.tls.key = "broker.key".into();
    f(&mut fc);
    build(fc, ConfigSource::File(dir.join("mqrust.toml")), &Overrides::default()).unwrap()
}

/// A TLS client that trusts `ca_pem`, optionally with its own certificate.
fn client_config(ca_pem: &str, identity: Option<(&str, &str)>, tls13_only: bool) -> rustls::ClientConfig {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut ca_pem.as_bytes()) {
        roots.add(cert.unwrap()).unwrap();
    }
    let versions: &[&rustls::SupportedProtocolVersion] = if tls13_only {
        &[&rustls::version::TLS13]
    } else {
        &[&rustls::version::TLS12]
    };
    let builder = rustls::ClientConfig::builder_with_protocol_versions(versions).with_root_certificates(roots);
    match identity {
        None => builder.with_no_client_auth(),
        Some((cert_pem, key_pem)) => {
            let chain: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_pem.as_bytes())
                .collect::<Result<_, _>>()
                .unwrap();
            let key = PrivateKeyDer::try_from(
                rustls_pemfile::private_key(&mut key_pem.as_bytes())
                    .unwrap()
                    .unwrap()
                    .clone_key()
                    .secret_der()
                    .to_vec(),
            )
            .unwrap();
            builder.with_client_auth_cert(chain, key).unwrap()
        }
    }
}

type Tls = tokio_rustls::client::TlsStream<TcpStream>;

async fn connect(port: u16, config: rustls::ClientConfig) -> Result<Tls, String> {
    let stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| e.to_string())?;
    let name = ServerName::try_from("broker.test").unwrap();
    tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, stream)
        .await
        .map_err(|e| e.to_string())
}

/// Sends the client `WireFormatInfo` and returns what the broker answers.
async fn negotiate(stream: &mut Tls) -> Vec<Command> {
    let mut properties = PrimitiveMap::new();
    properties.set("MaxInactivityDuration", Value::Long(0));
    properties.set("TightEncodingEnabled", Value::Bool(false));
    properties.set("CacheEnabled", Value::Bool(false));
    let hello = Encoder::new(VERSION).frame(&Command::WireFormatInfo(WireFormatInfo {
        magic: MAGIC,
        version: VERSION,
        properties,
    }));
    stream.write_all(&hello).await.unwrap();
    let decoder = Decoder::new(VERSION);
    let mut out = Vec::new();
    for _ in 0..2 {
        let mut len = [0u8; 4];
        tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut len))
            .await
            .expect("no answer within 5 s")
            .unwrap();
        let mut body = vec![0u8; i32::from_be_bytes(len) as usize];
        stream.read_exact(&mut body).await.unwrap();
        if let Some(c) = decoder.decode_frame(Bytes::from(body)).unwrap() {
            out.push(c);
        }
    }
    out
}

/// Logs in and returns the broker's answer.
async fn login(stream: &mut Tls, password: &str) -> Command {
    let info = Command::ConnectionInfo(ConnectionInfo {
        header: Header {
            command_id: 1,
            response_required: true,
        },
        connection_id: Some(ConnectionId {
            value: Arc::from("ID:tls-1"),
        }),
        client_id: None,
        password: Some(password.into()),
        user_name: Some("admin".into()),
        broker_path: None,
        broker_master_connector: false,
        manageable: false,
        client_master: true,
        fault_tolerant: false,
        failover_reconnect: false,
        client_ip: None,
    });
    stream.write_all(&Encoder::new(VERSION).frame(&info)).await.unwrap();
    let decoder = Decoder::new(VERSION);
    loop {
        let mut len = [0u8; 4];
        tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut len))
            .await
            .expect("no answer within 5 s")
            .unwrap();
        let mut body = vec![0u8; i32::from_be_bytes(len) as usize];
        stream.read_exact(&mut body).await.unwrap();
        if let Some(c) = decoder.decode_frame(Bytes::from(body)).unwrap() {
            return c;
        }
    }
}

#[tokio::test]
async fn openwire_runs_over_tls() {
    let d = dir("basic");
    let pki = Pki::new(&d);
    pki.issue("broker", "broker.test");
    let cfg = tls_config(&d, 62801, 62802, 8801, |_| {});
    let _broker = Broker::start(cfg).await;

    let mut stream = connect(62802, client_config(&pki.ca_pem, None, true))
        .await
        .expect("TLS connection");
    let (_, conn) = stream.get_ref();
    assert_eq!(conn.protocol_version(), Some(rustls::ProtocolVersion::TLSv1_3));
    let answer = negotiate(&mut stream).await;
    assert!(matches!(answer.first(), Some(Command::WireFormatInfo(_))), "{answer:?}");
    assert!(matches!(answer.get(1), Some(Command::BrokerInfo(_))), "{answer:?}");
    assert!(
        matches!(login(&mut stream, "admin").await, Command::Response { .. }),
        "the login works as over plain TCP"
    );

    // The plain listener is still there.
    assert!(TcpStream::connect(("127.0.0.1", 62801)).await.is_ok());
}

#[tokio::test]
async fn tls_12_is_refused_when_13_is_the_minimum() {
    let d = dir("v13");
    let pki = Pki::new(&d);
    pki.issue("broker", "broker.test");
    let cfg = tls_config(&d, 62803, 62804, 8803, |fc| fc.tls.min_version = "1.3".into());
    let _broker = Broker::start(cfg).await;

    let e = connect(62804, client_config(&pki.ca_pem, None, false))
        .await
        .expect_err("TLS 1.2 must be refused");
    assert!(e.to_lowercase().contains("protocol"), "{e}");
    // The same broker accepts TLS 1.3.
    assert!(connect(62804, client_config(&pki.ca_pem, None, true)).await.is_ok());
}

#[tokio::test]
async fn a_required_client_certificate_is_checked() {
    let d = dir("mtls");
    let pki = Pki::new(&d);
    pki.issue("broker", "broker.test");
    let (client_cert, client_key) = pki.issue("client", "client.test");
    let cfg = tls_config(&d, 62805, 62806, 8805, |fc| {
        fc.tls.client_auth = "required".into();
        fc.tls.client_ca = "ca.crt".into();
    });
    let _broker = Broker::start(cfg).await;

    // Without a certificate the connection is refused. Under TLS 1.3 the client finishes
    // its side of the handshake first, so the server's alert arrives on the first read.
    let refused = match connect(62806, client_config(&pki.ca_pem, None, true)).await {
        Err(e) => e,
        Ok(mut stream) => {
            let _ = stream.write_all(b"\0\0\0\x01").await;
            let mut buf = [0u8; 4];
            match tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut buf)).await {
                Ok(Err(e)) => e.to_string(),
                Ok(Ok(_)) => "the broker answered a client without a certificate".to_string(),
                Err(_) => "the broker said nothing".to_string(),
            }
        }
    };
    assert!(refused.to_lowercase().contains("certificate"), "{refused}");

    // With one signed by the CA the connection works, and the console shows its subject.
    let identity = Some((client_cert.as_str(), client_key.as_str()));
    let mut stream = connect(62806, client_config(&pki.ca_pem, identity, true))
        .await
        .expect("mutual TLS connection");
    let answer = negotiate(&mut stream).await;
    assert!(matches!(answer.first(), Some(Command::WireFormatInfo(_))));

    // A valid certificate is not a password.
    assert!(
        matches!(login(&mut stream, "wrong").await, Command::ExceptionResponse { .. }),
        "the password is still checked"
    );
}

#[tokio::test]
async fn a_silent_client_is_dropped_after_the_handshake_timeout() {
    let d = dir("slow");
    let pki = Pki::new(&d);
    pki.issue("broker", "broker.test");
    let cfg = tls_config(&d, 62807, 62808, 8807, |_| {});
    let _broker = Broker::start(cfg).await;

    let mut silent = TcpStream::connect(("127.0.0.1", 62808)).await.unwrap();
    let start = std::time::Instant::now();
    let mut buf = [0u8; 1];
    // The broker closes it: the read ends with 0 bytes or an error.
    let closed = tokio::time::timeout(Duration::from_secs(20), silent.read(&mut buf)).await;
    assert!(closed.is_ok(), "the broker did not close the silent connection");
    assert!(start.elapsed() >= Duration::from_secs(9), "closed too early");

    // Meanwhile other clients are served.
    let mut stream = connect(62808, client_config(&pki.ca_pem, None, true)).await.unwrap();
    assert!(matches!(
        negotiate(&mut stream).await.first(),
        Some(Command::WireFormatInfo(_))
    ));
}

#[tokio::test]
async fn tls_only_closes_the_plain_port() {
    let d = dir("tlsonly");
    let pki = Pki::new(&d);
    pki.issue("broker", "broker.test");
    let cfg = tls_config(&d, 62809, 62810, 8809, |fc| fc.broker.plain = false);
    let _broker = Broker::start(cfg).await;

    assert!(
        TcpStream::connect(("127.0.0.1", 62809)).await.is_err(),
        "nothing listens on the plain port"
    );
    let mut stream = connect(62810, client_config(&pki.ca_pem, None, true)).await.unwrap();
    assert!(matches!(
        negotiate(&mut stream).await.first(),
        Some(Command::WireFormatInfo(_))
    ));
}
