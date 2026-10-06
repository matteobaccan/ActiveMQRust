// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Admin console over real HTTP: login and sessions, API authentication, read-only methods,
//! escaping, pagination, JSON API, sorting, XML view, optional feature fields, snapshot
//! paging and lookup, and the single version source.

use base64::Engine;
use bytes::Bytes;
use serde_json::Value as J;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, Once};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpSocket;
use tokio::sync::mpsc;

use mqrust::broker::conn::{ConnHandle, Out};
use mqrust::broker::destination::{ProducerMeta, SubSpec, PAGE_MAX, WALK_CHUNK};
use mqrust::broker::{now_ms, Broker};
use mqrust::config::{build, ConfigSource, FileConfig, Overrides, UserEntry};
use mqrust::openwire::model::*;
use mqrust::openwire::props::{PrimitiveMap, Value};
use mqrust::openwire::types as t;
use mqrust::selector::Selector;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// -- log capture --------------------------------------------------------------------

static LOGS: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static LOG_INIT: Once = Once::new();

struct LogWriter;

impl std::io::Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        LOGS.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn logs() -> String {
    String::from_utf8_lossy(&LOGS.lock().unwrap()).into_owned()
}

// -- broker and HTTP helpers ------------------------------------------------------------

struct Console {
    broker: Arc<Broker>,
    port: u16,
}

async fn start_with(f: impl FnOnce(&mut FileConfig)) -> Console {
    LOG_INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(|| LogWriter)
            .try_init();
    });
    let mut fc = FileConfig::default();
    f(&mut fc);
    let o = Overrides {
        admin_port: Some(0),
        ..Default::default()
    };
    let broker = Broker::new(Arc::new(build(fc, ConfigSource::Defaults, &o).unwrap()));
    let (tx, rx) = tokio::sync::watch::channel(false);
    std::mem::forget(tx);
    let addr = mqrust::admin::start(broker.clone(), rx)
        .await
        .expect("admin console started");
    Console {
        broker,
        port: addr.port(),
    }
}

async fn start() -> Console {
    start_with(|_| {}).await
}

#[derive(Debug)]
struct Resp {
    status: u16,
    head: String,
    body: String,
}

impl Resp {
    fn header(&self, name: &str) -> Option<String> {
        let prefix = format!("{}:", name.to_ascii_lowercase());
        self.head
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with(&prefix))
            .map(|l| l[prefix.len()..].trim().to_string())
    }

    fn json(&self) -> J {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("{e}: {}", self.body))
    }

    /// Session token from `Set-Cookie`.
    fn token(&self) -> Option<String> {
        let c = self.header("set-cookie")?;
        let v = c.strip_prefix("mqrust_session=")?.split(';').next()?.to_string();
        (!v.is_empty()).then_some(v)
    }
}

struct Req<'a> {
    method: &'a str,
    path: &'a str,
    headers: Vec<(String, String)>,
    body: Option<String>,
    from: Option<IpAddr>,
}

fn req<'a>(method: &'a str, path: &'a str) -> Req<'a> {
    Req {
        method,
        path,
        headers: Vec::new(),
        body: None,
        from: None,
    }
}

impl<'a> Req<'a> {
    fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.to_string(), v.to_string()));
        self
    }
    fn cookie(self, token: &str) -> Self {
        self.header("Cookie", &format!("mqrust_session={token}"))
    }
    fn basic(self, user_pass: &str) -> Self {
        let v = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(user_pass));
        self.header("Authorization", &v)
    }
    fn form(mut self, fields: &[(&str, &str)]) -> Self {
        let body: Vec<String> = fields.iter().map(|(k, v)| format!("{k}={}", form_enc(v))).collect();
        self.body = Some(body.join("&"));
        self.header("Content-Type", "application/x-www-form-urlencoded")
    }
    fn from(mut self, ip: &str) -> Self {
        self.from = Some(ip.parse().unwrap());
        self
    }
    async fn send(self, port: u16) -> Resp {
        let socket = TcpSocket::new_v4().unwrap();
        if let Some(ip) = self.from {
            socket.bind(SocketAddr::new(ip, 0)).unwrap();
        }
        let mut s = socket.connect(SocketAddr::from(([127, 0, 0, 1], port))).await.unwrap();
        let mut text = format!(
            "{} {} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n",
            self.method, self.path
        );
        for (k, v) in &self.headers {
            text.push_str(&format!("{k}: {v}\r\n"));
        }
        let body = self.body.unwrap_or_default();
        text.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
        s.write_all(text.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await.unwrap();
        let text = String::from_utf8_lossy(&buf).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        Resp {
            status,
            head: head.to_string(),
            body: body.to_string(),
        }
    }
}

fn form_enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

async fn get(c: &Console, path: &str, token: &str) -> Resp {
    req("GET", path).cookie(token).send(c.port).await
}

async fn api(c: &Console, path: &str) -> Resp {
    req("GET", path).basic("admin:admin").send(c.port).await
}

async fn login_as(c: &Console, user: &str, pass: &str) -> Resp {
    req("POST", "/login")
        .form(&[("username", user), ("password", pass), ("next", "/")])
        .send(c.port)
        .await
}

async fn login(c: &Console) -> String {
    let r = login_as(c, "admin", "admin").await;
    assert_eq!(r.status, 303, "{r:?}");
    r.token().expect("session cookie")
}

/// True when the loopback alias can be used as a client address on this machine.
async fn alias_works(port: u16, ip: &str) -> bool {
    let socket = TcpSocket::new_v4().unwrap();
    socket.bind(SocketAddr::new(ip.parse().unwrap(), 0)).is_ok()
        && socket.connect(SocketAddr::from(([127, 0, 0, 1], port))).await.is_ok()
}

// -- message helpers ----------------------------------------------------------------

static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

fn producer() -> ProducerId {
    ProducerId {
        connection_id: Arc::from("ID:test-1-1-1:1"),
        session_id: 1,
        value: 1,
    }
}

fn text_content(body: &str) -> Bytes {
    let mut content = (body.len() as i32).to_be_bytes().to_vec();
    content.extend_from_slice(body.as_bytes());
    Bytes::from(content)
}

fn text_msg(dest: &Destination, body: &str) -> Message {
    let pid = producer();
    let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
    m.destination = Some(dest.clone());
    m.producer_id = Some(pid.clone());
    m.message_id = Some(MessageId {
        text_view: None,
        producer_id: Some(pid),
        producer_sequence_id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        broker_sequence_id: 0,
    });
    m.content = Some(text_content(body));
    m.timestamp = now_ms();
    m
}

fn put_msg(b: &Broker, m: Message) {
    b.deliver(m, true, now_ms()).unwrap();
}

fn put(b: &Broker, queue: &str, body: &str) {
    put_msg(b, text_msg(&Destination::queue(queue), body));
}

/// Puts `n` messages with correlation ids `m-1` … `m-n`.
fn put_numbered(b: &Broker, queue: &str, n: usize) {
    for i in 1..=n {
        let mut m = text_msg(&Destination::queue(queue), &format!("body {i}"));
        m.correlation_id = Some(format!("m-{i}"));
        put_msg(b, m);
    }
}

struct Client {
    handle: Arc<ConnHandle>,
    rx: mpsc::UnboundedReceiver<Out>,
}

impl Client {
    fn new(b: &Broker) -> Client {
        let (tx, rx) = mpsc::unbounded_channel();
        Client {
            handle: Arc::new(ConnHandle::new(b.new_conn_id(), "127.0.0.1:50000".parse().unwrap(), tx)),
            rx,
        }
    }

    fn subscribe(&self, b: &Broker, n: i64, dest: &Destination, prefetch: i32, selector: Option<&str>) -> ConsumerId {
        let id = ConsumerId {
            connection_id: Arc::from("ID:client-7-1-1:1"),
            session_id: 1,
            value: n,
        };
        b.get_or_create(dest, None).add_sub(
            SubSpec {
                id: id.clone(),
                conn: self.handle.clone(),
                prefetch,
                selector: selector.map(|s| Arc::new(Selector::compile(s).unwrap().unwrap())),
                no_local: false,
                browser: false,
            },
            now_ms(),
        );
        id
    }

    /// Dispatched messages: (correlation id, redelivery counter).
    fn drain(&mut self) -> Vec<(String, i32)> {
        let mut v = Vec::new();
        while let Ok(o) = self.rx.try_recv() {
            if let Out::Cmd(Command::MessageDispatch(md)) = o {
                if let Some(m) = md.message {
                    v.push((m.correlation_id.clone().unwrap_or_default(), m.redelivery_counter));
                }
            }
        }
        v
    }
}

fn ack(b: &Broker, dest: &Destination, consumer: &ConsumerId, last: i64) {
    let mid = MessageId {
        text_view: None,
        producer_id: Some(producer()),
        producer_sequence_id: 0,
        broker_sequence_id: last,
    };
    let a = MessageAck {
        header: Header::default(),
        destination: Some(dest.clone()),
        transaction_id: None,
        consumer_id: Some(consumer.clone()),
        ack_type: ack_type::STANDARD,
        first_message_id: None,
        last_message_id: Some(mid),
        message_count: 1,
        poison_cause: None,
    };
    let effects = b.get_dest(dest).unwrap().ack(&a, false, now_ms());
    b.run_effects(effects, now_ms());
}

fn ids(page: &J) -> Vec<String> {
    page["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["correlationId"].as_str().unwrap_or("").to_string())
        .collect()
}

fn names(list: &J) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|q| q["name"].as_str().unwrap().to_string())
        .collect()
}

// -- login and sessions --------------------------------------------------------------

#[tokio::test]
async fn login_flow_sessions_and_headers() {
    let c = start().await;
    put(&c.broker, "SECRETQ", "x");

    let r = req("GET", "/queues").send(c.port).await;
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location").as_deref(), Some("/login?next=%2Fqueues"));
    assert!(r.header("www-authenticate").is_none());
    assert!(!r.body.contains("SECRETQ"));
    let r = req("GET", "/queues?refresh=5").send(c.port).await;
    assert_eq!(
        r.header("location").as_deref(),
        Some("/login?next=%2Fqueues%3Frefresh%3D5")
    );

    let r = req("GET", "/login?next=%2Fqueues%3Frefresh%3D5").send(c.port).await;
    assert_eq!(r.status, 200);
    for s in [
        "action=\"/login\"",
        "name=\"username\"",
        "name=\"password\"",
        "type=\"password\"",
        "autocomplete=\"current-password\"",
        "<label for=\"username\">",
        "<label for=\"password\">",
        "default admin credentials are in use",
        "value=\"/queues?refresh=5\"",
    ] {
        assert!(r.body.contains(s), "{s}");
    }
    assert!(!r.body.contains("SECRETQ"));
    assert!(!r.body.contains("<script"));

    // Successful login back to the requested page.
    let r = req("POST", "/login")
        .form(&[
            ("username", "admin"),
            ("password", "admin"),
            ("next", "/queues?refresh=5"),
        ])
        .send(c.port)
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location").as_deref(), Some("/queues?refresh=5"));
    let cookie = r.header("set-cookie").unwrap();
    assert!(cookie.ends_with("; HttpOnly; SameSite=Strict; Path=/"), "{cookie}");
    assert!(!cookie.contains("Expires") && !cookie.contains("Max-Age"));
    let token = r.token().unwrap();
    assert_eq!(
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&token)
            .unwrap()
            .len(),
        32
    );

    let r = get(&c, "/queues?refresh=5", &token).await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("SECRETQ"));
    assert!(r.body.contains("<meta http-equiv=\"refresh\" content=\"5\">"));
    assert!(r.body.contains("href=\"/topics?refresh=5\""));
    assert!(r
        .body
        .contains("<span class=\"user\" title=\"Logged in user\">admin</span>"));
    assert!(r.body.contains("action=\"/logout\""));
    assert!(r.body.contains("<meta name=\"color-scheme\" content=\"light dark\">"));
    assert!(r.body.contains("<html lang=\"en\">"));
    assert_eq!(r.body.matches("<h1>").count(), 1);
    assert_eq!(
        r.header("content-security-policy").as_deref(),
        Some("default-src 'none'; style-src 'self'; form-action 'self'; frame-ancestors 'none'")
    );
    assert_eq!(r.header("x-content-type-options").as_deref(), Some("nosniff"));
    assert_eq!(r.header("referrer-policy").as_deref(), Some("same-origin"));
    assert_eq!(r.header("cache-control").as_deref(), Some("no-store"));

    // API: cookie, Basic, nothing.
    assert_eq!(get(&c, "/api/queues", &token).await.status, 200);
    assert_eq!(api(&c, "/api/overview").await.status, 200);
    let r = req("GET", "/api/queues").send(c.port).await;
    assert_eq!(r.status, 401);
    assert_eq!(r.json(), serde_json::json!({ "error": "unauthorized" }));
    assert!(r.header("www-authenticate").is_none());
    assert_eq!(
        req("GET", "/api/queues").basic("admin:nope").send(c.port).await.status,
        401
    );

    // Only GET and HEAD, plus POST on /login and /logout.
    assert_eq!(
        req("POST", "/queues/SECRETQ").cookie(&token).send(c.port).await.status,
        405
    );
    assert_eq!(
        req("DELETE", "/api/queues/SECRETQ")
            .basic("admin:admin")
            .send(c.port)
            .await
            .status,
        405
    );
    assert_eq!(req("PUT", "/login").send(c.port).await.status, 405);
    assert_eq!(req("POST", "/").cookie(&token).send(c.port).await.status, 405);
    assert_eq!(req("HEAD", "/queues").cookie(&token).send(c.port).await.status, 200);
    assert_eq!(
        c.broker
            .get_dest(&Destination::queue("SECRETQ"))
            .unwrap()
            .snapshot()
            .pending,
        1
    );

    // Stylesheet: public, embedded, small.
    let r = req("GET", "/style.css").send(c.port).await;
    assert_eq!(r.status, 200);
    assert!(r.header("content-type").unwrap().starts_with("text/css"));
    assert!(r.body.len() < 20 * 1024);
    assert!(!r.body.contains("url(") && !r.body.contains("@import"));

    // Logout from a foreign origin is refused.
    let r = req("POST", "/logout")
        .cookie(&token)
        .header("Origin", "https://evil.example")
        .send(c.port)
        .await;
    assert_eq!(r.status, 403);
    assert_eq!(get(&c, "/queues", &token).await.status, 200);
    let r = req("POST", "/login")
        .header("Origin", "https://evil.example")
        .form(&[("username", "admin"), ("password", "admin")])
        .send(c.port)
        .await;
    assert_eq!(r.status, 403);
    assert!(r.token().is_none());

    // Logout ends the session.
    let r = req("POST", "/logout")
        .cookie(&token)
        .header("Origin", &format!("http://127.0.0.1:{}", c.port))
        .send(c.port)
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location").as_deref(), Some("/login"));
    assert!(r.header("set-cookie").unwrap().contains("Max-Age=0"));
    assert_eq!(get(&c, "/queues", &token).await.status, 303);
    assert_eq!(get(&c, "/api/queues", &token).await.status, 401);
}

#[tokio::test]
async fn session_fixation_and_open_redirect() {
    let c = start().await;
    let first = login(&c).await;
    // A cookie sent with the login is discarded and a new token issued.
    let r = req("POST", "/login")
        .cookie(&first)
        .form(&[("username", "admin"), ("password", "admin")])
        .send(c.port)
        .await;
    let second = r.token().unwrap();
    assert_ne!(first, second);
    assert_eq!(get(&c, "/", &first).await.status, 303);
    assert_eq!(get(&c, "/", &second).await.status, 200);

    for bad in ["//evil.example/", "https://evil.example/", "/\\evil.example"] {
        let r = req("POST", "/login")
            .form(&[("username", "admin"), ("password", "admin"), ("next", bad)])
            .send(c.port)
            .await;
        assert_eq!(r.status, 303);
        assert_eq!(r.header("location").as_deref(), Some("/"), "{bad}");
    }
}

#[tokio::test]
async fn theme_follows_the_system_only() {
    let c = start().await;
    let token = login(&c).await;
    let mut pages: Vec<Resp> = Vec::new();
    for page in [
        "/",
        "/queues",
        "/topics",
        "/connections",
        "/queues?sort=pending&order=desc&refresh=5",
    ] {
        // An old theme cookie is ignored.
        pages.push(
            req("GET", page)
                .header("Cookie", &format!("mqrust_session={token}; mqrust_theme=dark"))
                .send(c.port)
                .await,
        );
    }
    pages.push(
        req("GET", "/login")
            .header("Cookie", "mqrust_theme=dark")
            .send(c.port)
            .await,
    );
    for r in &pages {
        assert_eq!(r.status, 200, "{r:?}");
        assert!(r.body.contains("<html lang=\"en\"><head>"), "{}", r.body);
        assert!(r.body.contains("<meta name=\"color-scheme\" content=\"light dark\">"));
        assert!(!r.body.contains("data-theme"), "{}", r.body);
        assert!(
            !r.body.contains("/theme/") && !r.body.contains("class=\"theme\""),
            "{}",
            r.body
        );
        assert!(!r.body.contains("aria-label=\"Theme\""));
        assert!(
            r.header("set-cookie").is_none_or(|v| !v.contains("mqrust_theme")),
            "{r:?}"
        );
    }
    for mode in ["dark", "light", "auto"] {
        let r = get(&c, &format!("/theme/{mode}?next=%2F"), &token).await;
        assert_eq!(r.status, 404, "{mode}");
        assert!(r.header("set-cookie").is_none(), "{mode}");
    }
    let css = req("GET", "/style.css").send(c.port).await.body;
    assert!(!css.contains("data-theme"));
    assert!(css.contains(
        "@media (prefers-color-scheme: dark) {
  :root {"
    ));
}

#[tokio::test]
async fn failed_logins_are_logged_and_throttled() {
    let c = start_with(|fc| fc.admin.login_lockout_seconds = 1).await;
    let r = login_as(&c, "admin", "wrong-pw-123").await;
    assert_eq!(r.status, 200);
    assert!(r.token().is_none());
    assert!(r.body.contains("Invalid username or password"));
    assert!(r.body.contains("value=\"admin\""));
    assert!(!r.body.contains("wrong-pw-123"));
    let unknown = login_as(&c, "nobody", "wrong-pw-123").await;
    assert_eq!(unknown.status, 200);
    assert_eq!(unknown.body.replace("value=\"nobody\"", "value=\"admin\""), r.body);
    let missing = req("POST", "/login").form(&[("username", "admin")]).send(c.port).await;
    assert!(missing.body.contains("Invalid username or password"));
    let l = logs();
    assert!(l.contains("admin login failed: 127.0.0.1 user=admin"), "{l}");
    assert!(l.contains("admin login failed: 127.0.0.1 user=nobody"));
    assert!(!l.contains("wrong-pw-123"));

    // Fifth failure, then the correct password is refused without being checked.
    login_as(&c, "admin", "bad").await;
    login_as(&c, "admin", "bad").await;
    let r = login_as(&c, "admin", "admin").await;
    assert_eq!(r.status, 429);
    assert!(r.token().is_none());
    assert!(r.body.contains("Too many failed logins"));
    assert!(logs().contains("admin login refused: 127.0.0.1 is locked out"));
    // The API is refused too while the address is locked out.
    assert_eq!(api(&c, "/api/overview").await.status, 429);
    if alias_works(c.port, "127.0.0.2").await {
        let r = req("POST", "/login")
            .from("127.0.0.2")
            .form(&[("username", "admin"), ("password", "admin")])
            .send(c.port)
            .await;
        assert_eq!(r.status, 303, "another client is not locked out");
    }
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let r = login_as(&c, "admin", "admin").await;
    assert_eq!(r.status, 303);
    let token = r.token().unwrap();

    // Failed Basic attempts on the API count too.
    for _ in 0..5 {
        assert_eq!(
            req("GET", "/api/overview")
                .basic("admin:wrong")
                .send(c.port)
                .await
                .status,
            401
        );
    }
    assert_eq!(api(&c, "/api/overview").await.status, 429);
    // A logged-in browser keeps working.
    assert_eq!(get(&c, "/api/overview", &token).await.status, 200);
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(api(&c, "/api/overview").await.status, 200);

    // Requests without credentials are not failed logins; tokens are never logged.
    if alias_works(c.port, "127.0.0.9").await {
        assert_eq!(req("GET", "/").from("127.0.0.9").send(c.port).await.status, 303);
        assert_eq!(
            req("GET", "/api/overview").from("127.0.0.9").send(c.port).await.status,
            401
        );
        assert!(!logs().contains("127.0.0.9"));
    }
    assert!(!logs().contains(&token));
}

#[tokio::test]
async fn argon2_admin_and_openwire_users_are_separate() {
    let hash = mqrust::auth::hash_password("s3cret").unwrap();
    let c = start_with(|fc| {
        fc.admin.username = "boss".into();
        fc.admin.password_hash = Some(hash.clone());
        fc.users = vec![UserEntry {
            username: "app1".into(),
            password: Some("secret".into()),
            password_hash: None,
        }];
    })
    .await;
    let r = req("GET", "/login").send(c.port).await;
    assert!(!r.body.contains("default admin credentials"));
    let r = login_as(&c, "boss", "s3cret").await;
    assert_eq!(r.status, 303);
    let r = get(&c, "/", &r.token().unwrap()).await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains(&format!("ActiveMQRust {VERSION}")));
    let r = login_as(&c, "app1", "secret").await;
    assert!(r.body.contains("Invalid username or password"));
    assert!(r.token().is_none());
    assert_eq!(
        req("GET", "/api/overview")
            .basic("app1:secret")
            .send(c.port)
            .await
            .status,
        401
    );
    // The second call is served from the verified-credential cache.
    for _ in 0..2 {
        assert_eq!(
            req("GET", "/api/overview")
                .basic("boss:s3cret")
                .send(c.port)
                .await
                .status,
            200
        );
    }
}

// -- pages and API ---------------------------------------------------------------------

#[tokio::test]
async fn pages_escape_broker_data() {
    let c = start().await;
    let token = login(&c).await;
    let evil = "<script>alert(1)</script>";
    let mut m = text_msg(&Destination::queue("Q<script>"), evil);
    let mut props = PrimitiveMap::new();
    props.set(evil, Value::String(evil.into()));
    m.marshalled_properties = Some(props.encode());
    m.correlation_id = Some(evil.into());
    put_msg(&c.broker, m);

    let list = get(&c, "/queues", &token).await.body;
    let detail = get(&c, "/queues/Q%3Cscript%3E", &token).await;
    assert_eq!(detail.status, 200);
    let page = api(&c, "/api/queues/Q%3Cscript%3E/messages").await.json();
    let msg = &page["messages"][0];
    let path = format!(
        "/queues/Q%3Cscript%3E/messages/{}?seq={}",
        form_enc(msg["messageId"].as_str().unwrap()),
        msg["position"]
    );
    let message = get(&c, &path, &token).await;
    assert_eq!(message.status, 200);
    for body in [&list, &detail.body, &message.body] {
        assert!(body.contains("&lt;script&gt;"));
        assert!(!body.contains("<script"), "{body}");
    }
    assert!(message.body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
}

#[tokio::test]
async fn browsing_does_not_consume() {
    let c = start().await;
    let token = login(&c).await;
    put_numbered(&c.broker, "BROWSE", 10);
    let q = Destination::queue("BROWSE");
    let before = c.broker.get_dest(&q).unwrap().snapshot();
    assert_eq!(get(&c, "/queues/BROWSE", &token).await.status, 200);
    let page = api(&c, "/api/queues/BROWSE/messages").await.json();
    for m in page["messages"].as_array().unwrap() {
        let id = form_enc(m["messageId"].as_str().unwrap());
        for view in ["", "&view=xml"] {
            let p = format!("/queues/BROWSE/messages/{id}?seq={}{view}", m["position"]);
            assert_eq!(get(&c, &p, &token).await.status, 200);
            assert_eq!(
                api(
                    &c,
                    &format!("/api/queues/BROWSE/messages/{id}?seq={}{view}", m["position"])
                )
                .await
                .status,
                200
            );
        }
        // Without the sequence hint the lookup scans the queue.
        assert_eq!(
            get(&c, &format!("/queues/BROWSE/messages/{id}"), &token).await.status,
            200
        );
    }
    let after = c.broker.get_dest(&q).unwrap().snapshot();
    assert_eq!(after.pending, 10);
    assert_eq!(
        (after.stats.enqueued, after.stats.dequeued, after.stats.dispatched),
        (before.stats.enqueued, 0, 0)
    );
    let mut client = Client::new(&c.broker);
    client.subscribe(&c.broker, 1, &q, 100, None);
    let got = client.drain();
    assert_eq!(
        got.iter().map(|g| g.0.clone()).collect::<Vec<_>>(),
        (1..=10).map(|i| format!("m-{i}")).collect::<Vec<_>>()
    );
    assert!(got.iter().all(|g| g.1 == 0), "not redelivered");
}

#[tokio::test]
async fn contents_pagination_and_api_limit() {
    let c = start().await;
    let token = login(&c).await;
    put_numbered(&c.broker, "PAGED", 120);
    let ids_on = |body: &str| -> Vec<usize> { (1..=120).filter(|i| body.contains(&format!(">m-{i}<"))).collect() };
    let p1 = get(&c, "/queues/PAGED", &token).await.body;
    assert_eq!(ids_on(&p1), (1..=50).collect::<Vec<_>>());
    let first = p1.find(">m-1<").unwrap();
    assert!(first < p1.find(">m-2<").unwrap() && p1.find(">m-49<").unwrap() < p1.find(">m-50<").unwrap());
    assert!(p1.contains("href=\"/queues/PAGED?page=2\""));
    assert!(p1.contains("Messages (120 pending)"));
    let p3 = get(&c, "/queues/PAGED?page=3", &token).await.body;
    assert_eq!(ids_on(&p3), (101..=120).collect::<Vec<_>>());
    let p9 = get(&c, "/queues/PAGED?page=9", &token).await;
    assert_eq!(p9.status, 200);
    assert!(p9.body.contains("No messages on this page"));

    put_numbered(&c.broker, "BIG", 1000);
    let r = api(&c, "/api/queues/BIG/messages?offset=10&limit=500").await.json();
    assert_eq!(ids(&r), (11..=60).map(|i| format!("m-{i}")).collect::<Vec<_>>());
    assert_eq!((r["total"].as_u64(), r["limit"].as_u64()), (Some(1000), Some(50)));
    let r = api(&c, "/api/queues/BIG/messages?offset=990&limit=0").await.json();
    assert_eq!(ids(&r), vec!["m-991".to_string()]);
    let r = api(&c, "/api/queues/NOPE/messages").await;
    assert_eq!(r.status, 404);
    assert!(r.json()["error"].is_string());
    assert_eq!(get(&c, "/queues/NOPE", &token).await.status, 404);
    // A queue name with special characters.
    put(&c.broker, "orders/eu 1", "x");
    let list = get(&c, "/queues", &token).await.body;
    assert!(list.contains("href=\"/queues/orders%2Feu%201\""));
    assert_eq!(get(&c, "/queues/orders%2Feu%201", &token).await.status, 200);
}

#[tokio::test]
async fn json_api_contents_consumers_producers() {
    let c = start().await;
    let token = login(&c).await;
    for i in 0..3 {
        put(&c.broker, "Q1", &format!("x{i}"));
    }
    let list = api(&c, "/api/queues").await.json();
    let q1 = list.as_array().unwrap().iter().find(|q| q["name"] == "Q1").unwrap();
    assert_eq!((q1["pending"].as_u64(), q1["inflight"].as_u64()), (Some(3), Some(0)));
    assert_eq!((q1["consumers"].as_u64(), q1["enqueued"].as_u64()), (Some(0), Some(3)));
    assert!(q1["memory"].as_u64().unwrap() > 0);

    let q2 = Destination::queue("Q2");
    put_numbered(&c.broker, "Q2", 5);
    let d = c.broker.get_dest(&q2).unwrap();
    d.add_producer(
        producer(),
        ProducerMeta {
            conn_id: 9,
            remote: "127.0.0.1:50001".into(),
            connection_id: "ID:test-1-1-1:1".into(),
        },
    );
    let mut client = Client::new(&c.broker);
    client.subscribe(&c.broker, 1, &q2, 2, Some("JMSCorrelationID IN ('m-1','m-2','m-3')"));
    assert_eq!(client.drain().len(), 2);
    let v = api(&c, "/api/queues/Q2").await.json();
    assert_eq!((v["pending"].as_u64(), v["inflight"].as_u64()), (Some(3), Some(2)));
    let cons = &v["consumers"][0];
    assert_eq!(cons["prefetch"], 2);
    assert_eq!(cons["inflight"], 2);
    assert_eq!(cons["connectionId"], "ID:client-7-1-1:1");
    assert_eq!(cons["client"], "127.0.0.1:50000");
    assert_eq!(cons["selector"], "JMSCorrelationID IN ('m-1','m-2','m-3')");
    let prod = &v["producers"][0];
    assert_eq!(prod["producerId"], "ID:test-1-1-1:1:1:1");
    assert_eq!(prod["client"], "127.0.0.1:50001");
    let html = get(&c, "/queues/Q2", &token).await.body;
    assert!(html.contains("JMSCorrelationID IN (&#39;m-1&#39;,&#39;m-2&#39;,&#39;m-3&#39;)"));
    assert!(html.contains("ID:client-7-1-1:1"));
    let ov = api(&c, "/api/overview").await.json();
    assert_eq!(ov["product"], "ActiveMQRust");
    assert_eq!(ov["version"], VERSION);
    assert_eq!(ov["queues"], 2);
    for k in [
        "uptimeSeconds",
        "connections",
        "topics",
        "messageMemory",
        "workingSet",
        "privateBytes",
        "compressed",
        "compressDiscarded",
    ] {
        assert!(ov[k].is_number(), "{k}");
    }
    assert!(ov["memoryLimit"].is_null());
    assert_eq!(api(&c, "/api/topics").await.json(), serde_json::json!([]));
    assert_eq!(api(&c, "/api/connections").await.json(), serde_json::json!([]));
}

#[tokio::test]
async fn inflight_and_consumed_message_pages() {
    let c = start().await;
    let token = login(&c).await;
    let q = Destination::queue("INF");
    put_numbered(&c.broker, "INF", 2);
    let page = api(&c, "/api/queues/INF/messages").await.json();
    let first = page["messages"][0].clone();
    let id = form_enc(first["messageId"].as_str().unwrap());
    let seq = first["position"].as_i64().unwrap();
    let client = Client::new(&c.broker);
    let cid = client.subscribe(&c.broker, 1, &q, 1, None);
    let r = get(&c, &format!("/queues/INF/messages/{id}?seq={seq}"), &token).await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("in flight"));
    let j = api(&c, &format!("/api/queues/INF/messages/{id}")).await.json();
    assert_eq!(j["inflight"], true);
    ack(&c.broker, &q, &cid, seq);
    assert_eq!(
        get(&c, &format!("/queues/INF/messages/{id}?seq={seq}"), &token)
            .await
            .status,
        404
    );
    assert_eq!(api(&c, &format!("/api/queues/INF/messages/{id}")).await.status, 404);
}

#[tokio::test]
async fn queues_table_sorting() {
    let c = start().await;
    let token = login(&c).await;
    for (name, n) in [("A", 9), ("b", 100), ("C", 10)] {
        put_numbered(&c.broker, name, n);
    }
    let order = |body: &str| -> Vec<&'static str> {
        let mut v: Vec<(usize, &'static str)> = ["A", "b", "C"]
            .iter()
            .map(|n| (body.find(&format!("href=\"/queues/{n}\"")).unwrap(), *n))
            .collect();
        v.sort();
        v.into_iter().map(|x| x.1).collect()
    };
    let r = get(&c, "/queues", &token).await.body;
    assert_eq!(order(&r), ["A", "b", "C"]);
    for col in [
        "name",
        "pending",
        "inflight",
        "consumers",
        "producers",
        "enqueued",
        "consumed",
        "expired",
    ] {
        assert!(r.contains(&format!("<a href=\"/queues?sort={col}&amp;order=")), "{col}");
    }
    assert!(r.contains("aria-sort=\"ascending\""));
    assert_eq!(
        order(&get(&c, "/queues?sort=pending&order=asc", &token).await.body),
        ["A", "C", "b"]
    );
    assert_eq!(
        order(&get(&c, "/queues?sort=pending&order=desc", &token).await.body),
        ["b", "C", "A"]
    );
    assert_eq!(
        order(&get(&c, "/queues?sort=name&order=desc", &token).await.body),
        ["C", "b", "A"]
    );
    // Ties by name ascending, whatever the direction.
    assert_eq!(
        order(&get(&c, "/queues?sort=consumers&order=desc", &token).await.body),
        ["A", "b", "C"]
    );
    assert_eq!(
        order(&get(&c, "/queues?sort=bogus", &token).await.body),
        ["A", "b", "C"]
    );
    // Toggle and arrows; the sort survives auto-refresh.
    let r = get(&c, "/queues?sort=enqueued&order=asc", &token).await.body;
    assert!(r.contains("href=\"/queues?sort=enqueued&amp;order=desc\""));
    let r = get(&c, "/queues?sort=enqueued&order=desc&refresh=5", &token).await.body;
    assert!(r.contains("aria-sort=\"descending\""));
    assert!(r.contains("&#9660;"));
    assert!(r.contains("href=\"/queues?sort=enqueued&amp;order=asc&amp;refresh=5\""));
    assert!(
        r.contains("href=\"/queues?sort=enqueued&amp;order=desc\""),
        "refresh toggle keeps the sort"
    );
    let j = api(&c, "/api/queues?sort=pending&order=desc").await.json();
    assert_eq!(names(&j), ["b", "C", "A"]);
    let j = api(&c, "/api/queues?sort=consumed&order=desc").await.json();
    assert_eq!(names(&j), ["A", "b", "C"]);
}

#[tokio::test]
async fn xml_view_raw_and_formatted() {
    let c = start().await;
    let token = login(&c).await;
    let xml_body = "<order id=\"7\"><item qty=\"2\">A</item><item qty=\"1\">B</item></order>";
    for body in [xml_body, "hello <world>", "<order><item></order>"] {
        put(&c.broker, "XML", body);
    }
    let mut big = String::from("<?xml version=\"1.0\"?><r>");
    while big.len() < 2 * 1024 * 1024 {
        big.push_str("<i>0123456789</i>");
    }
    big.push_str("</r>");
    put(&c.broker, "XML", &big);
    let page = api(&c, "/api/queues/XML/messages").await.json();
    let path = |i: usize| {
        let m = &page["messages"][i];
        format!(
            "/queues/XML/messages/{}?seq={}",
            form_enc(m["messageId"].as_str().unwrap()),
            m["position"]
        )
    };

    let raw = get(&c, &path(0), &token).await.body;
    assert!(raw.contains(">Raw</a>") && raw.contains(">Formatted</a>"));
    assert!(raw.contains("&lt;order id=&quot;7&quot;&gt;&lt;item"));
    let fmt = get(&c, &format!("{}&view=xml&refresh=5", path(0)), &token).await.body;
    assert!(fmt.contains("<span class=\"x-tag\">&lt;order</span>"));
    assert!(fmt.contains("<span class=\"x-attr\">qty</span>"));
    assert!(fmt.contains("\n  <span class=\"x-tag\">&lt;item</span>"));
    assert!(
        fmt.contains("&amp;refresh=5\" aria-current=\"true\">Formatted</a>"),
        "links keep the view and refresh"
    );
    assert!(fmt.contains("?view=xml&amp;seq="));
    assert!(fmt.contains("<meta http-equiv=\"refresh\" content=\"5\">"));

    let plain = get(&c, &path(1), &token).await.body;
    assert!(!plain.contains(">Formatted</a>"));
    assert!(plain.contains("hello &lt;world&gt;"));
    let bad = get(&c, &path(2), &token).await.body;
    assert!(bad.contains("Not well-formed XML: line 1, column"));
    assert!(!bad.contains(">Formatted</a>"));
    let large = get(&c, &path(3), &token).await.body;
    assert!(large.contains("Too large to format (limit 1 MB)"));
    assert!(large.contains("Text truncated at 64 KB."));

    let api_path = |i: usize| path(i).replacen("/queues/", "/api/queues/", 1);
    let j = api(&c, &format!("{}&view=xml", api_path(0))).await.json();
    assert_eq!(
        j["formattedBody"],
        "<order id=\"7\">\n  <item qty=\"2\">A</item>\n  <item qty=\"1\">B</item>\n</order>"
    );
    assert_eq!(j["body"]["kind"], "text");
    assert_eq!(j["body"]["text"], xml_body);
    let j = api(&c, &format!("{}&view=xml", api_path(2))).await.json();
    assert!(j["formattedBody"].is_null());
    assert!(j["formatError"].as_str().unwrap().starts_with("Not well-formed XML"));
    let j = api(&c, &api_path(1)).await.json();
    assert!(j.get("formattedBody").is_none());
}

#[tokio::test]
async fn optional_feature_fields() {
    let c = start().await;
    let token = login(&c).await;
    let now = now_ms();
    // Expiration: one message with a 60 s TTL, one without.
    let mut m = text_msg(&Destination::queue("EXP"), "ttl");
    m.expiration = now + 60_000;
    put_msg(&c.broker, m);
    put(&c.broker, "EXP", "no ttl");
    let v = api(&c, "/api/queues/EXP").await.json();
    assert_eq!(v["withExpiration"], 1);
    let next = v["nextExpiration"].as_i64().unwrap();
    assert!((next - (now + 60_000)).abs() < 1000);
    let html = get(&c, "/queues/EXP", &token).await.body;
    assert!(html.contains("Pending with expiration"));
    assert!(
        html.contains("(in 59s)") || html.contains("(in 1m 0s)"),
        "next expiration shown"
    );
    assert!(html.contains("never"));
    let page = api(&c, "/api/queues/EXP/messages").await.json();
    assert_eq!(page["messages"][0]["expired"], false);
    assert!(page["messages"][0]["expirationText"].as_str().unwrap().contains("(in "));
    assert!(page["messages"][0]["expiresInMs"].as_i64().unwrap() > 50_000);
    assert_eq!(page["messages"][1]["expirationText"], "never");

    // Expired but not yet removed: marked; then removed and counted.
    for _ in 0..3 {
        let mut m = text_msg(&Destination::queue("SHORT"), "short");
        m.expiration = now_ms() + 100;
        put_msg(&c.broker, m);
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let page = api(&c, "/api/queues/SHORT/messages").await.json();
    assert_eq!(page["messages"][0]["expired"], true);
    let html = get(&c, "/queues/SHORT", &token).await.body;
    assert!(html.contains("<span class=\"badge warn\">expired</span>"));
    let m0 = &page["messages"][0];
    let msg = get(
        &c,
        &format!(
            "/queues/SHORT/messages/{}?seq={}",
            form_enc(m0["messageId"].as_str().unwrap()),
            m0["position"]
        ),
        &token,
    )
    .await
    .body;
    assert!(msg.contains("(expired)"));
    c.broker
        .get_dest(&Destination::queue("SHORT"))
        .unwrap()
        .sweep_expired(now_ms(), 100);
    let list = api(&c, "/api/queues").await.json();
    let short = list.as_array().unwrap().iter().find(|q| q["name"] == "SHORT").unwrap();
    assert_eq!(short["expired"], 3);
    assert!(get(&c, "/queues", &token)
        .await
        .body
        .contains("<td class=\"num\">3</td></tr>"));

    // Compression: a client-compressed text message.
    let text = "compressible text ".repeat(6000);
    let mut m = text_msg(&Destination::queue("ZIP"), "");
    m.content = Some(Bytes::from(mqrust::broker::compress::compress_content(
        t::ACTIVEMQ_TEXT_MESSAGE,
        &text_content(&text),
    )));
    m.compressed = true;
    let stored = m.content.as_ref().unwrap().len();
    put_msg(&c.broker, m);
    put(&c.broker, "ZIP", "plain");
    let v = api(&c, "/api/queues/ZIP").await.json();
    assert_eq!(v["compressed"], 1);
    let page = api(&c, "/api/queues/ZIP/messages").await.json();
    assert_eq!(page["messages"][0]["compressed"], true);
    assert_eq!(page["messages"][0]["compressedSize"], stored);
    assert_eq!(page["messages"][0]["bodySize"], stored);
    assert!(page["messages"][1].get("compressedSize").is_none());
    let html = get(&c, "/queues/ZIP", &token).await.body;
    assert!(html.contains("<span class=\"badge info\">compressed</span>"));
    let m0 = &page["messages"][0];
    let msg = get(
        &c,
        &format!(
            "/queues/ZIP/messages/{}?seq={}",
            form_enc(m0["messageId"].as_str().unwrap()),
            m0["position"]
        ),
        &token,
    )
    .await
    .body;
    assert!(msg.contains(&format!("{stored} bytes stored")));
    assert!(msg.contains("compressible text compressible text"));

    // Topic statistics: 3 subscribers, 1 producer, 10 published; memory counted once.
    let c2 = start().await;
    let topic = Destination::new(DestKind::Topic, "T1");
    let client = Client::new(&c2.broker);
    for n in 1..=3 {
        client.subscribe(&c2.broker, n, &topic, 1000, None);
    }
    c2.broker.get_dest(&topic).unwrap().add_producer(
        producer(),
        ProducerMeta {
            conn_id: 1,
            remote: "127.0.0.1:1".into(),
            connection_id: "ID:test-1-1-1:1".into(),
        },
    );
    for i in 0..10 {
        put_msg(&c2.broker, text_msg(&topic, &format!("t{i}")));
    }
    let topics = api(&c2, "/api/topics").await.json();
    let t1 = &topics[0];
    assert_eq!((t1["name"].as_str(), t1["consumers"].as_u64()), (Some("T1"), Some(3)));
    assert_eq!(
        (t1["producers"].as_u64(), t1["published"].as_u64()),
        (Some(1), Some(10))
    );
    assert_eq!(t1["memory"].as_u64(), Some(c2.broker.memory.used()));
    let html = get(&c2, "/topics", &login(&c2).await).await.body;
    assert!(html.contains(">T1<"));
}

#[tokio::test]
async fn overview_footer_and_version() {
    let c = start().await;
    let token = login(&c).await;
    let r = get(&c, "/", &token).await.body;
    assert!(r.contains(&format!("<h1>ActiveMQRust {VERSION}</h1>")));
    for s in [
        "Uptime",
        "Active connections",
        "Working Set",
        "Private Bytes",
        "no limit",
        "Compressed by the broker",
        "Compressions discarded",
    ] {
        assert!(r.contains(s), "{s}");
    }
    // Listen addresses: two labelled lines right below the title, outside the cards.
    let admin = format!("http://{}:{}", c.broker.cfg.admin_bind, c.broker.cfg.admin_port);
    let h1_end = r.find("</h1>").expect("h1") + "</h1>".len();
    let cards = r.find("<div class=\"cards\">").expect("cards");
    let between = &r[h1_end..cards];
    assert!(between.starts_with("<p class=\"addresses\">"), "{between}");
    assert!(
        between.contains("<span class=\"label\">OpenWire</span> <code>tcp://0.0.0.0:61616</code>"),
        "{between}"
    );
    assert!(
        between.contains(&format!(
            "<span class=\"label\">Admin console</span> <code>{admin}</code>"
        )),
        "{between}"
    );
    let cards_html = &r[cards..r.find("</main>").expect("main")];
    for addr in ["tcp://", "http://"] {
        assert!(!cards_html.contains(addr), "{addr} in a card");
    }
    assert!(!cards_html.contains(">OpenWire<") && !cards_html.contains(">Admin<"));
    // The JSON API keeps the addresses.
    let j = api(&c, "/api/overview").await.json();
    assert_eq!(j["openwire"], "tcp://0.0.0.0:61616");
    for page in ["/", "/queues", "/topics", "/connections"] {
        let body = get(&c, page, &token).await.body;
        let footer = &body[body.find("<footer").expect("footer")..];
        assert!(footer.contains(&format!("ActiveMQRust {VERSION}")));
        assert!(
            footer.contains("<a href=\"https://github.com/matteobaccan/ActiveMQRust\" rel=\"noopener noreferrer\">")
        );
        assert!(footer.contains("by Matteo Baccan"));
    }
    let login_page = req("GET", "/login").send(c.port).await.body;
    assert!(login_page.contains(&format!("ActiveMQRust {VERSION}")) && login_page.contains("by Matteo Baccan"));
    assert_eq!(
        env!("CARGO_PKG_REPOSITORY"),
        "https://github.com/matteobaccan/ActiveMQRust"
    );
}

/// The version lives only in Cargo.toml: no literal in the sources, and every place that
/// shows it reads `CARGO_PKG_VERSION`.
#[test]
fn single_version_source() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut stack = vec![root.join("src"), root.join("build.rs")];
    let quoted = format!("\"{VERSION}\"");
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            stack.extend(std::fs::read_dir(&p).unwrap().map(|e| e.unwrap().path()));
        } else {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            assert!(!text.contains(&quoted), "version literal in {}", p.display());
            assert!(
                !text.contains(&format!("ActiveMQRust {VERSION}")),
                "version literal in {}",
                p.display()
            );
        }
    }
    assert_eq!(mqrust::openwire::wireformat::PROVIDER_VERSION, VERSION);
    let wf = mqrust::openwire::wireformat::broker_wire_format(
        &WireFormatInfo {
            magic: *b"ActiveMQ",
            version: 12,
            properties: PrimitiveMap::new(),
        },
        1024,
    );
    assert_eq!(wf.properties.get_string("ProviderVersion"), Some(VERSION));
    assert_eq!(wf.properties.get_string("ProviderName"), Some("ActiveMQRust"));
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mqrust"))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("ActiveMQRust {VERSION}")
    );
    // Windows file version resource.
    if !cfg!(windows) {
        return;
    }
    let ps = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "(Get-Item '{}').VersionInfo.ProductVersion",
                env!("CARGO_BIN_EXE_mqrust")
            ),
        ])
        .output();
    if let Ok(o) = ps {
        if o.status.success() {
            assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), VERSION);
        }
    }
}

#[tokio::test]
async fn defaults_serve_console_with_admin_admin() {
    // The default address and credentials come from the built-in configuration; the
    // listener here uses a free port so tests do not clash with a running broker.
    let cfg = build(FileConfig::default(), ConfigSource::Defaults, &Overrides::default()).unwrap();
    assert_eq!(cfg.admin_bind.to_string(), "127.0.0.1");
    assert_eq!(cfg.admin_port, 8161);
    let c = start().await;
    assert_eq!(c.broker.cfg.admin_bind.to_string(), "127.0.0.1");
    let token = login(&c).await;
    let r = get(&c, "/", &token).await;
    assert!(r.body.contains(&format!("ActiveMQRust {VERSION}")));
}

// -- snapshot paging and lookup ---------------------------------------------------------

#[test]
fn page_and_find_on_a_large_queue() {
    let b = Broker::new(Arc::new(
        build(FileConfig::default(), ConfigSource::Defaults, &Overrides::default()).unwrap(),
    ));
    let n = WALK_CHUNK * 2 + 500;
    put_numbered(&b, "LARGE", n);
    let d = b.get_dest(&Destination::queue("LARGE")).unwrap();
    let corr = |v: &[mqrust::broker::entry::Entry]| -> Vec<String> {
        v.iter().map(|e| e.msg.correlation_id.clone().unwrap()).collect()
    };
    let (total, first) = d.page(0, 50);
    assert_eq!(total, n);
    assert_eq!(corr(&first), (1..=50).map(|i| format!("m-{i}")).collect::<Vec<_>>());
    // Never more than PAGE_MAX references, whatever the limit.
    assert_eq!(d.page(0, 100_000).1.len(), PAGE_MAX);
    // Forward walk across chunks, backward walk near the end, last page and beyond.
    let (_, mid) = d.page(WALK_CHUNK + 7, 50);
    assert_eq!(corr(&mid)[0], format!("m-{}", WALK_CHUNK + 8));
    assert_eq!(corr(&mid)[49], format!("m-{}", WALK_CHUNK + 57));
    let (_, late) = d.page(n - 3000, 50);
    assert_eq!(corr(&late)[0], format!("m-{}", n - 2999));
    let (_, last) = d.page(n - 20, 50);
    assert_eq!(
        corr(&last),
        ((n - 19)..=n).map(|i| format!("m-{i}")).collect::<Vec<_>>()
    );
    assert!(d.page(n, 50).1.is_empty());
    assert!(d.page(n + 1000, 50).1.is_empty());

    // Lookup with the sequence hint, without it (scan across chunks), with a wrong hint.
    let target = &last[10];
    let id = target.msg.message_id_text();
    let (e, inflight) = d.find(Some(target.seq), &id).unwrap();
    assert_eq!((e.seq, inflight), (target.seq, false));
    assert_eq!(d.find(None, &id).unwrap().0.seq, target.seq);
    assert_eq!(d.find(Some(first[0].seq), &id).unwrap().0.seq, target.seq);
    assert!(d.find(None, "ID:nope-1-1-1:1:1:1").is_none());
    // Paging copies only Arc references: the queue still owns the same messages.
    assert!(Arc::ptr_eq(&d.page(0, 1).1[0].msg, &first[0].msg));
}

#[test]
fn memory_and_compressed_counters_track_held_messages() {
    let b = Broker::new(Arc::new(
        build(FileConfig::default(), ConfigSource::Defaults, &Overrides::default()).unwrap(),
    ));
    let q = Destination::queue("COUNT");
    for i in 0..4 {
        let mut m = text_msg(&q, &format!("{i}"));
        m.compressed = i % 2 == 0;
        put_msg(&b, m);
    }
    let d = b.get_dest(&q).unwrap();
    let s = d.snapshot();
    assert_eq!(s.memory, b.memory.used());
    assert_eq!(s.compressed, 2);
    let mut client = Client::new(&b);
    let cid = client.subscribe(&b, 1, &q, 1, None);
    let got = client.drain();
    assert_eq!(got.len(), 1);
    // The first message (compressed) is in flight: no longer pending, still held.
    let s = d.snapshot();
    assert_eq!((s.pending, s.inflight, s.compressed), (3, 1, 1));
    assert_eq!(s.memory, b.memory.used());
    let seq = d.page(0, 1).1[0].seq - 1;
    ack(&b, &q, &cid, seq as i64);
    let s = d.snapshot();
    assert_eq!(s.memory, b.memory.used());
    assert!(s.memory > 0);
}

// -- polling while producing (functional, no timing) ----------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn polling_while_producing_keeps_counts_right() {
    const N: usize = 20_000;
    let c = start().await;
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let poller = {
        let (stop, polls, port) = (stop.clone(), polls.clone(), c.port);
        tokio::spawn(async move {
            loop {
                for p in [
                    "/api/queues/LOAD",
                    "/api/queues/LOAD/messages?offset=15000",
                    "/api/queues",
                ] {
                    let r = req("GET", p).basic("admin:admin").send(port).await;
                    assert!(r.status == 200 || r.status == 404, "{p}: {}", r.status);
                    polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
            }
        })
    };
    let b = c.broker.clone();
    let producer = tokio::task::spawn_blocking(move || {
        for i in 1..=N {
            let mut m = text_msg(&Destination::queue("LOAD"), "x");
            m.correlation_id = Some(format!("m-{i}"));
            put_msg(&b, m);
        }
    });
    producer.await.unwrap();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    poller.await.unwrap();
    assert!(
        polls.load(std::sync::atomic::Ordering::Relaxed) >= 3,
        "the API was polled"
    );
    let v = api(&c, "/api/queues/LOAD").await.json();
    assert_eq!(
        (v["pending"].as_u64(), v["enqueued"].as_u64(), v["consumed"].as_u64()),
        (Some(N as u64), Some(N as u64), Some(0))
    );
    let d = c.broker.get_dest(&Destination::queue("LOAD")).unwrap();
    assert_eq!(d.snapshot().memory, c.broker.memory.used());
    // Every page holds at most 50 messages, in FIFO order, wherever it starts.
    for offset in [0, 9_999, 10_000, N - 50] {
        let r = api(&c, &format!("/api/queues/LOAD/messages?offset={offset}&limit=50"))
            .await
            .json();
        assert_eq!(
            ids(&r),
            ((offset + 1)..=(offset + 50))
                .map(|i| format!("m-{i}"))
                .collect::<Vec<_>>()
        );
    }
}

// -- table sorting tests (OpenSpec add-admin-table-sorting) ----------------------------

#[tokio::test]
async fn natural_name_order_scenario() {
    let c = start().await;
    let token = login(&c).await;
    for name in ["Q10", "q2", "Q1"] {
        put(&c.broker, name, "data");
    }
    let order_natural = |body: &str| -> Vec<String> {
        let mut v: Vec<(usize, &str)> = ["Q10", "q2", "Q1"]
            .iter()
            .map(|n| (body.find(&format!("href=\"/queues/{n}\"")).unwrap(), *n))
            .collect();
        v.sort();
        v.into_iter().map(|x| x.1.to_string()).collect()
    };
    let r = get(&c, "/queues?sort=name&order=asc", &token).await.body;
    assert_eq!(order_natural(&r), ["Q1", "q2", "Q10"]);
}

#[tokio::test]
async fn connections_sorting_scenarios() {
    let c = start().await;
    let token = login(&c).await;

    let (tx1, _) = mpsc::unbounded_channel();
    let (tx2, _) = mpsc::unbounded_channel();
    let (tx3, _) = mpsc::unbounded_channel();

    let mut h1 = ConnHandle::new(c.broker.new_conn_id(), "10.0.0.10:5000".parse().unwrap(), tx1);
    h1.connected_at = chrono::Local::now() - chrono::Duration::seconds(30);
    h1.info.lock().connection_id = "ID:conn-1".into();
    h1.info.lock().consumers = 0;

    let mut h2 = ConnHandle::new(c.broker.new_conn_id(), "10.0.0.9:6000".parse().unwrap(), tx2);
    h2.connected_at = chrono::Local::now() - chrono::Duration::seconds(10);
    h2.info.lock().connection_id = "ID:conn-2".into();
    h2.info.lock().consumers = 12;

    let mut h3 = ConnHandle::new(c.broker.new_conn_id(), "10.0.0.9:5001".parse().unwrap(), tx3);
    h3.connected_at = chrono::Local::now() - chrono::Duration::seconds(20);
    h3.info.lock().connection_id = "ID:conn-3".into();
    h3.info.lock().consumers = 3;

    c.broker.register_conn(Arc::new(h1));
    c.broker.register_conn(Arc::new(h2));
    c.broker.register_conn(Arc::new(h3));

    // Scenario: Connections by consumers (12, 3, 0)
    let r = get(&c, "/connections?sort=consumers&order=desc", &token).await.body;
    assert!(r.contains("aria-sort=\"descending\""));
    assert!(r.contains("&#9660;"));
    let pos_h2 = r.find("ID:conn-2").unwrap();
    let pos_h3 = r.find("ID:conn-3").unwrap();
    let pos_h1 = r.find("ID:conn-1").unwrap();
    assert!(pos_h2 < pos_h3 && pos_h3 < pos_h1);
    assert!(r[pos_h2..pos_h3].contains("<td class=\"num\">12</td>"));
    assert!(r[pos_h3..pos_h1].contains("<td class=\"num\">3</td>"));

    // Scenario: Connections by client address (10.0.0.9:5001, 10.0.0.9:6000, 10.0.0.10:5000)
    let r = get(&c, "/connections?sort=client&order=asc", &token).await.body;
    let pos_5001 = r.find("10.0.0.9:5001").unwrap();
    let pos_6000 = r.find("10.0.0.9:6000").unwrap();
    let pos_5000 = r.find("10.0.0.10:5000").unwrap();
    assert!(pos_5001 < pos_6000 && pos_6000 < pos_5000);

    // Scenario: Connections by time (connected desc -> most recent first: h2, h3, h1)
    let r = get(&c, "/connections?sort=connected&order=desc", &token).await.body;
    let pos_h2 = r.find("ID:conn-2").unwrap();
    let pos_h3 = r.find("ID:conn-3").unwrap();
    let pos_h1 = r.find("ID:conn-1").unwrap();
    assert!(pos_h2 < pos_h3 && pos_h3 < pos_h1);

    // Scenario: Unknown column falls back to Connected ascending
    let r = get(&c, "/connections?sort=bogus", &token).await.body;
    let pos_h1 = r.find("ID:conn-1").unwrap();
    let pos_h3 = r.find("ID:conn-3").unwrap();
    let pos_h2 = r.find("ID:conn-2").unwrap();
    assert!(pos_h1 < pos_h3 && pos_h3 < pos_h2);

    // Scenario: Connections API sort
    let j = api(&c, "/api/connections?sort=consumers&order=desc").await.json();
    let consumers_list: Vec<u64> = j
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["consumers"].as_u64().unwrap())
        .collect();
    assert_eq!(consumers_list, vec![12, 3, 0]);
}

#[tokio::test]
async fn topics_sorting_scenarios() {
    let c = start().await;
    let token = login(&c).await;
    for (name, published) in [("TOPIC-C", 5), ("TOPIC-A", 20), ("TOPIC-B", 10)] {
        let dest = Destination::new(DestKind::Topic, name);
        for i in 0..published {
            put_msg(&c.broker, text_msg(&dest, &format!("msg {i}")));
        }
    }

    // Scenario: Topics API sort by published desc
    let j = api(&c, "/api/topics?sort=published&order=desc").await.json();
    let pub_names: Vec<&str> = j
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(pub_names, vec!["TOPIC-A", "TOPIC-B", "TOPIC-C"]);

    // HTML sort by Name asc (default)
    let r = get(&c, "/topics", &token).await.body;
    let pos_a = r.find(">TOPIC-A<").unwrap();
    let pos_b = r.find(">TOPIC-B<").unwrap();
    let pos_c = r.find(">TOPIC-C<").unwrap();
    assert!(pos_a < pos_b && pos_b < pos_c);
}

#[tokio::test]
async fn queue_detail_sorting_and_kept_page() {
    let c = start().await;
    let token = login(&c).await;
    let q = Destination::queue("MULTI");
    put_numbered(&c.broker, "MULTI", 180);

    let client1 = Client::new(&c.broker);
    let client2 = Client::new(&c.broker);

    // Subscribe consumers: ID:h-1:1:1:2 and ID:h-1:1:1:10
    let cid2 = ConsumerId {
        connection_id: Arc::from("ID:h-1:1"),
        session_id: 1,
        value: 2,
    };
    c.broker.get_dest(&q).unwrap().add_sub(
        SubSpec {
            id: cid2,
            conn: client1.handle.clone(),
            prefetch: 10,
            selector: None,
            no_local: false,
            browser: false,
        },
        now_ms(),
    );

    let cid10 = ConsumerId {
        connection_id: Arc::from("ID:h-1:1"),
        session_id: 1,
        value: 10,
    };
    c.broker.get_dest(&q).unwrap().add_sub(
        SubSpec {
            id: cid10,
            conn: client2.handle.clone(),
            prefetch: 50,
            selector: None,
            no_local: false,
            browser: false,
        },
        now_ms(),
    );

    // Add producers
    let d = c.broker.get_dest(&q).unwrap();
    let pid1 = ProducerId {
        connection_id: Arc::from("ID:pconn-1"),
        session_id: 1,
        value: 1,
    };
    d.add_producer(
        pid1,
        ProducerMeta {
            conn_id: 101,
            remote: "10.0.0.2:5000".into(),
            connection_id: "ID:pconn-1".into(),
        },
    );
    let pid2 = ProducerId {
        connection_id: Arc::from("ID:pconn-2"),
        session_id: 1,
        value: 2,
    };
    d.add_producer(
        pid2,
        ProducerMeta {
            conn_id: 102,
            remote: "10.0.0.1:5000".into(),
            connection_id: "ID:pconn-2".into(),
        },
    );

    // Scenario: Natural order of IDs (ID:h-1:1:1:2 before ID:h-1:1:1:10)
    let r = get(&c, "/queues/MULTI?csort=consumerId&corder=asc", &token).await.body;
    let pos_c2 = r.find("ID:h-1:1:1:2").unwrap();
    let pos_c10 = r.find("ID:h-1:1:1:10").unwrap();
    assert!(pos_c2 < pos_c10);

    // Scenario: Two tables on one page (csort=prefetch&corder=desc and psort=client&porder=asc)
    let r = get(
        &c,
        "/queues/MULTI?csort=prefetch&corder=desc&psort=client&porder=asc",
        &token,
    )
    .await
    .body;
    // Consumers: prefetch 50 before 10
    let pos_p50 = r.find("<td class=\"num\">50</td>").unwrap();
    let pos_p10 = r.find("<td class=\"num\">10</td>").unwrap();
    assert!(pos_p50 < pos_p10);
    // Producers: 10.0.0.1:5000 before 10.0.0.2:5000
    let pos_pr1 = r.find("10.0.0.1:5000").unwrap();
    let pos_pr2 = r.find("10.0.0.2:5000").unwrap();
    assert!(pos_pr1 < pos_pr2);

    // Check header links preserve both parameters
    assert!(r.contains("csort=prefetch&amp;corder=desc"));
    assert!(r.contains("psort=client&amp;porder=asc"));

    // Scenario: Contents page kept (page 3 still shows page 3 when sorting consumers)
    let r = get(&c, "/queues/MULTI?page=3&csort=prefetch&corder=desc", &token)
        .await
        .body;
    assert!(r.contains("<span class=\"muted\">Page 3 of 3</span>"));
    assert!(r.contains("m-161"));
    assert!(r.contains("csort=prefetch&amp;corder=desc"));

    // Scenario: Contents not sortable (Messages table headers are not links and messages are in FIFO order)
    let messages_start = r.find("<h2>Messages").unwrap();
    let messages_table = &r[messages_start..r[messages_start..].find("</tbody>").unwrap() + messages_start];
    assert!(
        !messages_table.contains("<a href="),
        "Messages headers must not be links"
    );
}

#[tokio::test]
async fn message_detail_properties_and_map_sorting() {
    let c = start().await;
    let token = login(&c).await;
    let q = Destination::queue("PROPS");

    let mut m = Message::new(t::ACTIVEMQ_MAP_MESSAGE);
    m.destination = Some(q.clone());
    let mut props = PrimitiveMap::new();
    props.set("beta", Value::String("b-val".into()));
    props.set("alpha", Value::String("a-val".into()));
    props.set("gamma", Value::String("g-val".into()));
    m.marshalled_properties = Some(props.encode());

    // Map body
    let mut map_body = PrimitiveMap::new();
    map_body.set("z_key", Value::String("z_val".into()));
    map_body.set("a_key", Value::String("a_val".into()));
    map_body.set("m_key", Value::String("m_val".into()));
    m.content = Some(map_body.encode());

    let pid = producer();
    m.producer_id = Some(pid.clone());
    let mid_val = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    m.message_id = Some(MessageId {
        text_view: None,
        producer_id: Some(pid),
        producer_sequence_id: mid_val,
        broker_sequence_id: 0,
    });
    put_msg(&c.broker, m);

    let page = api(&c, "/api/queues/PROPS/messages").await.json();
    let msg_item = &page["messages"][0];
    let msg_id = form_enc(msg_item["messageId"].as_str().unwrap());
    let seq = msg_item["position"].as_i64().unwrap();

    // Sort properties descending
    let r = get(
        &c,
        &format!("/queues/PROPS/messages/{msg_id}?seq={seq}&prsort=name&prorder=desc"),
        &token,
    )
    .await
    .body;
    let pos_gamma = r.find(">gamma<").unwrap();
    let pos_beta = r.find(">beta<").unwrap();
    let pos_alpha = r.find(">alpha<").unwrap();
    assert!(pos_gamma < pos_beta && pos_beta < pos_alpha);

    // Sort map body ascending
    let r = get(
        &c,
        &format!("/queues/PROPS/messages/{msg_id}?seq={seq}&msort=key&morder=asc"),
        &token,
    )
    .await
    .body;
    let pos_a = r.find(">a_key<").unwrap();
    let pos_m = r.find(">m_key<").unwrap();
    let pos_z = r.find(">z_key<").unwrap();
    assert!(pos_a < pos_m && pos_m < pos_z);
}
