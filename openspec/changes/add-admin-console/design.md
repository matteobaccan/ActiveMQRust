## Context

`bootstrap-broker-foundation` defines the `[admin]` configuration keys (`bind`, `port`, `username`, `password` / `password_hash`), the `--admin-bind` / `--admin-port` command-line overrides and the startup line `admin listening on http://<bind>:<port>`. `add-queue-messaging` adds the broker core: the destination registry, queues with `pending` (a `BTreeMap<broker_seq, Arc<StoredMessage>>`) and per-consumer `inflight`, consumers, producers, counters and memory accounting. Nothing yet shows this state to an operator.

ActiveMQ ships a Jetty-based web console. ActiveMQRust needs the parts operators actually use (design §8): an overview, queues with consumer and producer counts, queue contents, topics and connections, behind a login. The console must not compromise the project goals: it must add little memory, it must never hold a destination lock for long, and it must stay inside the single dependency-free `mqrust.exe`.

## Goals / Non-Goals

**Goals:**
- An authenticated, read-only HTTP console on `127.0.0.1:8161` by default, with the pages and JSON endpoints of design §8.
- Readable rendering of every JMS body type, including compressed bodies, without deserializing Java objects.
- Reads that cost O(page size) under the lock, never O(queue length).
- Fields from optional features (expiration, selectors, compression, topics) appear when those features are present, with no change to this capability.

**Non-Goals:**
- Write operations: purging or deleting queues, sending or moving messages (future work, design §12).
- HTTPS (a reverse proxy is the recommended way to expose the console remotely).
- Session handling, logout, user roles or per-page permissions.
- Advisory topics, JMX, Jolokia or any ActiveMQ console URL compatibility.

## Decisions

### D1. `axum` on the broker's Tokio runtime
The console is an `axum` router served by the same Tokio runtime as the OpenWire listener, with only the features it needs (`http1`, `tokio`, `query`, `json`).
- *Alternatives:* `hyper` directly (more boilerplate for routing, extraction and percent-decoding); a hand-written HTTP/1.1 server (smaller, but error-prone for parsing and escaping); a separate thread with a blocking server such as `tiny_http` (a second runtime model and threads that cost memory).

### D2. HTTP Basic authentication checked on every request, with a verified-credential cache
Every route, HTML and JSON, goes through one authentication layer. A missing or invalid `Authorization` header gets `401` with `WWW-Authenticate: Basic realm="ActiveMQRust"`, so the browser shows its own login prompt. There is no session. Argon2id verification takes tens of milliseconds and would run on every page load and every auto-refresh, so after a successful check the layer keeps a SHA-256 digest of the exact `username:password` pair that succeeded; a later request with the same pair is accepted by comparing digests in constant time. The cache holds one entry and is never written for failed attempts.
- *Alternatives:* verify Argon2 on every request (simple, but costly CPU per page and a cheap way to load the broker); a cookie session after login (needs session state, expiry and CSRF thinking for a read-only tool); bearer tokens (not usable from a plain browser).

### D3. Failed-login logging distinguishes "no credentials" from "wrong credentials"
A browser's first request carries no `Authorization` header; it gets `401` but is not a failed login and is not logged. A request with credentials that do not match is logged as a warning with remote IP and username, never the password, as `client-authentication` already does for OpenWire.
- *Alternative:* log every `401` (every first visit would produce a false warning).

### D4. Snapshot API on the broker core
The console never touches broker structures directly. The broker exposes snapshot functions that take the destination mutex, copy plain data (counters, consumer rows, and at most one page of `Arc<StoredMessage>` clones) and release the lock before any formatting or I/O. Pages and JSON are built from the copied data only. Cloning an `Arc` is O(1), so a page of 50 messages costs 50 pointer increments under the lock plus an O(log n) seek to the page start.
- *Alternatives:* a global read lock over the broker (blocks traffic during rendering); a periodically published copy of the whole state (memory cost proportional to queue size, against the RAM goal); an actor message to each destination (more latency and code for the same result).

### D5. Pagination by offset in FIFO order
The contents page uses `?page=<n>` (1-based, 50 messages per page); the JSON API uses `offset` and `limit` (limit capped at 50). The page start is found by walking `pending` in `broker_seq` order. For deep pages the walk is O(offset); this is accepted because it is bounded per request, and for very large offsets the walk is done in chunks of at most 10,000 entries with the lock released between chunks.
- *Alternatives:* cursor-based pagination by `broker_seq` (O(log n) seek, but links become unstable when messages are consumed and page numbers cannot be shown); an order-statistics tree (extra memory per message on the hot path for an admin-only benefit).

### D6. Message detail lookup by JMS message ID with a sequence hint
`{id}` in `/queues/{name}/messages/{id}` is the text form of the `JMSMessageID` (percent-encoded in the URL). Links from the contents page add `?seq=<broker_seq>`, which allows an O(log n) lookup that is then checked against the ID. Without the hint (a hand-typed URL), the broker scans `pending` in chunks of 10,000 entries, releasing the lock between chunks. A message no longer in `pending` gives `404`.
- *Alternatives:* use `broker_seq` as `{id}` (cheap, but not the ID operators see in their applications); keep a global `MessageId` index (memory on every message for an admin feature).

### D7. Server-side HTML without a template dependency
Pages are produced with `std::fmt::Write` into a `String`, through small helper functions for layout, tables and an HTML escaper that every piece of broker data passes through. The CSS is one `include_str!` asset. Auto-refresh is a `?refresh=5` query parameter that adds `<meta http-equiv="refresh" content="5">`; no JavaScript is needed. Sorting the queues table is done on the server with `?sort=<column>&order=asc|desc`.
- *Alternatives:* `askama` or `maud` compile-time templates (nicer syntax, one more dependency and macro compile time); a JavaScript single-page app over the JSON API (needs a frontend toolchain and embedded bundle, more bytes in the exe).

### D8. Body rendering decoded only on request
`src/openwire/message_body.rs` decodes map, stream and primitive values only when a message detail page is requested; the hot path keeps bodies opaque. A body with `compressed=true` is inflated with `flate2` in zlib format before rendering, reproducing the per-type compressed layout of the Java client (for `BytesMessage`, a 4-byte original length precedes the deflate data). Inflation stops at 64 KB of output to protect against zip bombs. Java serialized objects are never deserialized: only the size is shown.
- *Alternatives:* decode bodies on arrival (cost on every message, against the performance goal); show only hex for every type (useless for text and map messages, which are the most common).

### D9. Process memory from the Windows API
Process RSS on the overview is the Working Set from `GetProcessMemoryInfo` (`K32GetProcessMemoryInfo`), with Private Bytes shown next to it, called through `windows-sys`. Both values are read at request time.
- *Alternatives:* `sysinfo` crate (large dependency for two numbers); reading performance counters through PDH (slower and more code).

### D10. Optional features plug in through optional snapshot fields
Snapshot structs carry `Option` fields for data owned by other changes: `expired` count and next expiration, consumer selector, compressed flag and compressed size, topic statistics. When a feature is absent the field is `None`; HTML omits the column or shows `-`, and JSON omits the key. This lets this change be applied before or after those features.
- *Alternative:* make this change depend on all features (forces an ordering that is not needed and delays the console).

## Risks / Trade-offs

- [Message bodies and properties are attacker-controlled and could inject HTML or script] → Every value is escaped by one helper, the response sets `Content-Security-Policy: default-src 'none'; style-src 'self'` and `X-Content-Type-Options: nosniff`, and there is a test with `<script>` in body, property names and queue names.
- [Admin requests slow down a busy queue] → Locks are held only to copy at most 50 `Arc`s or to walk at most 10,000 entries per chunk; a test checks that send throughput during continuous admin polling stays within 5% of the baseline.
- [Basic authentication sends credentials in clear text] → Default bind is `127.0.0.1`; the README recommends a TLS reverse proxy for remote access.
- [Brute force of admin credentials] → Every failed attempt is logged with remote IP; Argon2 makes offline and online guessing costly; the verified-credential cache is never filled by failures.
- [Large text bodies inflate HTML responses] → Text is truncated at 64 KB, hex dumps at 4 KB, and decompression at 64 KB, each with a visible notice.
- [Compressed body format differs per message type] → Inflation follows the Java `storeContent()` layout per type and is covered by tests with bodies compressed by the real Java client (`useCompression=true`).

## Migration Plan

New feature with no stored state. With default settings the console starts on `127.0.0.1:8161` the next time `mqrust.exe` starts. Rollback means running the previous executable; nothing else changes.

## Open Questions

- Whether to add `/api/queues/{name}/messages/{id}` for a single message with its rendered body; the current API returns headers, properties and body metadata in the message list only.
- Whether an admin listener that fails to bind (port 8161 in use) should stop the broker (current decision, consistent with the OpenWire listener) or only log an error and keep serving OpenWire.
