## Why

The console is read-only: to remove a poison message or empty a test queue or a full `ActiveMQ.DLQ`, an operator has to write a consumer or restart the broker (which empties every queue, since messages live in RAM). ActiveMQ's web console offers "Delete" on a message and "Purge" on a queue; operators expect the same here.

## What Changes

- **Delete a message**: a "Delete message" button on the message page removes that pending message from the queue.
- **Purge a queue**: a "Purge" button on the queue page removes every pending message of the queue.
- Both ask for confirmation on a dedicated page (no JavaScript) and run only through `POST`, from the console's own origin, with a valid session.
- Only pending messages are removed: messages already dispatched to a consumer (inflight) stay with it, consumers and producers are not touched, the queue is not deleted.
- Removed messages are counted as dequeued (the Consumed column), their memory is released, and each operation is logged at info level with the admin user and IP.
- JSON API: `DELETE /api/queues/{name}/messages/{id}` and `POST /api/queues/{name}/purge`.
- New key `admin.read_only` (default `false`): `true` brings back today's read-only console, where both operations are refused.
- **BREAKING (spec)**: the "Read-only console" requirement becomes "Console write operations".

## Capabilities

### New Capabilities

- `admin-message-removal`: delete a pending message, purge a queue, confirmation pages, security checks, counters, memory, logging, API endpoints and `admin.read_only`.

### Modified Capabilities

- `admin-console`: "Read-only console" is renamed "Console write operations" and allows the two new operations unless `admin.read_only = true`.

## Impact

- Code: `src/broker/destination.rs` (remove one pending entry by message ID, remove all pending entries; update memory accounting, expiry index and counters), `src/admin/mod.rs` (routes, `POST` handling, origin check, read-only switch), `src/admin/pages.rs` (buttons, confirmation pages, result notices), `src/admin/api.rs` (two endpoints), `src/config.rs` (`admin.read_only`), template, tests in `tests/admin_http.rs` and `tests/broker_semantics.rs`.
- No new crates.
- Topics are out of scope: topic messages are not shown in the console.
