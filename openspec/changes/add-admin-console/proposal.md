## Why

ActiveMQRust replaces ActiveMQ Classic for in-RAM messaging, so operators lose the ActiveMQ web console they use to see what the broker is doing. Because messages live only in memory and are lost on restart, operators need to see queue depth, consumers, producers and queue contents before they decide to restart or reconfigure a broker. The console must show this without slowing traffic or adding memory, because "less RAM and faster than ActiveMQ" is the reason the product exists. Requirement R6 and success criterion 5 of the design ask for an authenticated console with queues, consumers, producers and queue contents.

## What Changes

- New HTTP admin listener on `admin.bind:admin.port` (default `127.0.0.1:8161`, local only), built on `axum`, running on the broker's Tokio runtime.
- HTTP Basic authentication with the `[admin]` credentials on every page and API endpoint; `401` without valid credentials; failed admin logins logged as warnings with remote IP and username, never the password.
- Read-only HTML pages generated server-side, with minimal CSS embedded in the binary and no JavaScript framework: `/` (overview, including `ActiveMQRust <version>` and process RSS), `/queues`, `/queues/{name}`, `/queues/{name}/messages/{id}`, `/topics`, `/connections`. Optional auto-refresh every 5 seconds.
- Message body rendering per JMS message type (text, bytes, map, object, stream), with decompression of compressed bodies for the preview, capped at 64 KB.
- JSON API with the same data under `/api/…`, for monitoring and scripts.
- Snapshot-based reads: the console copies only what a page needs (at most 50 messages per page) under the destination lock and releases it at once.
- Fields contributed by other changes are shown when those features are present: the `expired` counter and next expiration (`add-message-expiration`), consumer selectors (`add-message-selectors`), the compressed flag and compressed size (`add-message-compression`), topic statistics (`add-topic-messaging`).

## Capabilities

### New Capabilities

- `admin-console`: admin HTTP listener, Basic authentication, read-only policy, HTML pages, body rendering, JSON API, snapshot consistency, and the display of fields contributed by other features.

### Modified Capabilities

None.

## Impact

- New module `src/admin/` (`mod.rs` for the server and authentication, `pages.rs` for HTML, `api.rs` for JSON, `render.rs` for body rendering) and a read-only snapshot API in `src/broker/` (overview, destination list, destination detail, message page, single message, connection list).
- New body decoder `src/openwire/message_body.rs` (map, stream and primitive values), used only by the console, never on the message hot path.
- New dependencies compiled into the executable: `axum` (with minimal features), `serde_json`, `base64`, and `windows-sys` (process memory counters). `flate2` is used to inflate compressed bodies; it is shared with `add-message-compression` if that change is already applied.
- The `[admin]` configuration keys and the `--admin-bind` / `--admin-port` options already exist from `bootstrap-broker-foundation`; this change makes them take effect.
- Depends on `add-queue-messaging` (queues, consumers, producers, counters, stored messages). Works with or without `add-topic-messaging`, `add-message-selectors`, `add-message-expiration` and `add-message-compression`.
