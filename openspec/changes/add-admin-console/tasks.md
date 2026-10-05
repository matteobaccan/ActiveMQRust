## 1. Broker snapshot API

- [x] 1.1 Define snapshot structs (overview, queue row, queue detail, consumer row, producer row, message summary, message detail, topic row, connection row) with `Option` fields for expiration, selector, compression and topic data
- [x] 1.2 Implement overview and connection snapshots from the broker registry (uptime, listen addresses, counts, message memory used and limit)
- [x] 1.3 Implement per-queue snapshots under the queue mutex, copying counters, consumer and producer rows, and at most 50 `Arc<StoredMessage>` for a page (offset walk in chunks of at most 10,000 with the lock released between chunks)
- [x] 1.4 Implement single-message lookup by `JMSMessageID` with the `seq` hint and the chunked fallback scan
- [x] 1.5 Verify the counters needed by the console (enqueued, consumed, inflight, producers) exist in the queue core; add any that are missing
- [x] 1.6 Unit tests: page boundaries (first, last, beyond last), FIFO order, lookup with and without hint, at most 50 references copied

## 2. Body decoding and rendering

- [x] 2.1 Implement `message_body.rs`: decoding of map, stream and primitive values per the OpenWire marshalling format
- [x] 2.2 Implement inflation of compressed bodies per message type (Java `storeContent()` layout, `BytesMessage` length prefix) with a 64 KB output cap
- [x] 2.3 Implement rendering for text (64 KB truncation), bytes (4 KB hex dump), map, object (size only), stream and empty bodies
- [x] 2.4 Unit tests per message type, truncation notices, and a zip-bomb body that inflates to 100 MB

## 3. HTTP server and authentication

- [x] 3.1 Add `axum`, `serde_json`, `base64` and `windows-sys` with minimal features; confirm `scripts\check-deps.cmd` still passes
- [x] 3.2 Start the admin listener on `admin.bind:admin.port` with the CLI overrides, log the listen line; on bind failure log an error and keep the broker running without the console
- [x] 3.3 Implement the Basic authentication layer for the JSON API (plain and Argon2id, constant-time compare, single-entry SHA-256 digest cache); HTML pages use the form login of `improve-admin-console`, and `401` carries no `WWW-Authenticate` challenge
- [x] 3.4 Log failed admin logins as warnings with remote IP and username; do not log requests without credentials
- [x] 3.5 Reject methods other than `GET` and `HEAD` with `405`; add the `Content-Security-Policy` and `X-Content-Type-Options` headers
- [x] 3.6 Read process Working Set and Private Bytes with `K32GetProcessMemoryInfo`

## 4. HTML pages

- [x] 4.1 Implement the page layout helpers, the HTML escaper, the embedded stylesheet and the `refresh=5` option
- [x] 4.2 Implement `/` overview with `ActiveMQRust <version>` and process memory
- [x] 4.3 Implement `/queues` with server-side sorting and hidden advisory destinations
- [x] 4.4 Implement `/queues/{name}` with counters, consumers, producers and paginated contents
- [x] 4.5 Implement `/queues/{name}/messages/{id}` with headers, properties and rendered body
- [x] 4.6 Implement `/topics` (empty table without topic messaging) and `/connections`
- [x] 4.7 Show optional fields when present: expired counter, next expiration, expired marks and readable expiration; consumer selectors; compressed flag, compressed size and compressed count

## 5. JSON API

- [x] 5.1 Implement `/api/overview`, `/api/queues`, `/api/queues/{name}`, `/api/topics`, `/api/connections`
- [x] 5.2 Implement `/api/queues/{name}/messages?offset=&limit=` with limit capped at 50 and JSON `404` errors
- [x] 5.3 Document the JSON shapes in the README (all optional features are present in this broker, so their keys are always given)

## 6. Tests

- [x] 6.1 Admin integration tests: redirect to login (pages) and `401` (API) without and with wrong credentials, `200` with plain and Argon2 credentials, OpenWire user rejected, failed-login log without password, no log for missing credentials
- [x] 6.2 Read-only tests: `405` for write methods; browsing every message page leaves contents, order, redelivery flags and counters unchanged
- [x] 6.3 Escaping tests with `<script>` in queue name, properties and body
- [x] 6.4 JSON API content tests after a known scenario (queue counts, consumer and producer rows, message pages)
- [x] 6.5 Java test: a client with `useCompression=true` sends a 50 KB TextMessage; the message page shows the text and the consumer receives it intact
- [x] 6.6 Functional polling test: 20,000 messages sent while the API is polled continuously; every send and request completes, counters and memory are right, pages at any offset copy at most 50 messages in FIFO order (no timing measurements)
- [x] 6.7 Feature-present tests, run when the related change is applied: expiration fields, selector display, compression marks, topic statistics

## 7. Verification

- [x] 7.1 Without a configuration file the console is configured on `127.0.0.1:8161` with `admin`/`admin` and shows `ActiveMQRust <version>` on `/` (verified by tests with the built-in defaults; the listener in tests uses a free port)
- [x] 7.2 The console works from a folder that contains only `mqrust.exe`, and `scripts\check-deps.cmd` passes
- [x] 7.3 All acceptance scenarios that passed before this change still pass against `mqrust.exe`
- [x] 7.4 `openspec validate add-admin-console` passes
