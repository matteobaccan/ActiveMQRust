## 1. Broker snapshot API

- [ ] 1.1 Define snapshot structs (overview, queue row, queue detail, consumer row, producer row, message summary, message detail, topic row, connection row) with `Option` fields for expiration, selector, compression and topic data
- [ ] 1.2 Implement overview and connection snapshots from the broker registry (uptime, listen addresses, counts, message memory used and limit)
- [ ] 1.3 Implement per-queue snapshots under the queue mutex, copying counters, consumer and producer rows, and at most 50 `Arc<StoredMessage>` for a page (offset walk in chunks of at most 10,000 with the lock released between chunks)
- [ ] 1.4 Implement single-message lookup by `JMSMessageID` with the `seq` hint and the chunked fallback scan
- [ ] 1.5 Verify the counters needed by the console (enqueued, consumed, inflight, producers) exist in the queue core; add any that are missing
- [ ] 1.6 Unit tests: page boundaries (first, last, beyond last), FIFO order, lookup with and without hint, at most 50 references copied

## 2. Body decoding and rendering

- [ ] 2.1 Implement `message_body.rs`: decoding of map, stream and primitive values per the OpenWire marshalling format
- [ ] 2.2 Implement inflation of compressed bodies per message type (Java `storeContent()` layout, `BytesMessage` length prefix) with a 64 KB output cap
- [ ] 2.3 Implement rendering for text (64 KB truncation), bytes (4 KB hex dump), map, object (size only), stream and empty bodies
- [ ] 2.4 Unit tests per message type, truncation notices, and a zip-bomb body that inflates to 100 MB

## 3. HTTP server and authentication

- [ ] 3.1 Add `axum`, `serde_json`, `base64` and `windows-sys` with minimal features; confirm `scripts\check-deps.cmd` still passes
- [ ] 3.2 Start the admin listener on `admin.bind:admin.port` with the CLI overrides, log the listen line, exit non-zero on bind failure
- [ ] 3.3 Implement the Basic authentication layer (plain and Argon2id, constant-time compare, single-entry verified-credential digest cache, `401` with `WWW-Authenticate: Basic realm="ActiveMQRust"`)
- [ ] 3.4 Log failed admin logins as warnings with remote IP and username; do not log requests without credentials
- [ ] 3.5 Reject methods other than `GET` and `HEAD` with `405`; add the `Content-Security-Policy` and `X-Content-Type-Options` headers
- [ ] 3.6 Read process Working Set and Private Bytes with `K32GetProcessMemoryInfo`

## 4. HTML pages

- [ ] 4.1 Implement the page layout helpers, the HTML escaper, the embedded stylesheet and the `refresh=5` option
- [ ] 4.2 Implement `/` overview with `ActiveMQRust <version>` and process memory
- [ ] 4.3 Implement `/queues` with server-side sorting and hidden advisory destinations
- [ ] 4.4 Implement `/queues/{name}` with counters, consumers, producers and paginated contents
- [ ] 4.5 Implement `/queues/{name}/messages/{id}` with headers, properties and rendered body
- [ ] 4.6 Implement `/topics` (empty table without topic messaging) and `/connections`
- [ ] 4.7 Show optional fields when present: expired counter, next expiration, expired marks and readable expiration; consumer selectors; compressed flag, compressed size and compressed count

## 5. JSON API

- [ ] 5.1 Implement `/api/overview`, `/api/queues`, `/api/queues/{name}`, `/api/topics`, `/api/connections`
- [ ] 5.2 Implement `/api/queues/{name}/messages?offset=&limit=` with limit capped at 50 and JSON `404` errors
- [ ] 5.3 Omit keys of absent optional features; document the JSON shapes in the README

## 6. Tests

- [ ] 6.1 Admin integration tests: `401` without and with wrong credentials on pages and API, `200` with plain and Argon2 credentials, OpenWire user rejected, failed-login log without password, no log for missing credentials
- [ ] 6.2 Read-only tests: `405` for write methods; browsing every message page leaves contents, order, redelivery flags and counters unchanged
- [ ] 6.3 Escaping tests with `<script>` in queue name, properties and body
- [ ] 6.4 JSON API content tests after a known scenario (queue counts, consumer and producer rows, message pages)
- [ ] 6.5 Java test: a client with `useCompression=true` sends a 50 KB TextMessage; the message page shows the text and the consumer receives it intact
- [ ] 6.6 Load test: 1 KB message throughput with continuous detail-page polling is at least 95% of the baseline; first page of a 1,000,000-message queue grows memory by less than 1 MB
- [ ] 6.7 Feature-present tests, run when the related change is applied: expiration fields, selector display, compression marks, topic statistics

## 7. Verification

- [ ] 7.1 `mqrust.exe` started with no arguments serves the console on `127.0.0.1:8161` with `admin`/`admin` and shows `ActiveMQRust <version>` on `/`
- [ ] 7.2 The console works from a folder that contains only `mqrust.exe`, and `scripts\check-deps.cmd` passes
- [ ] 7.3 All acceptance scenarios that passed before this change still pass against `mqrust.exe`
- [ ] 7.4 `openspec validate add-admin-console` passes
