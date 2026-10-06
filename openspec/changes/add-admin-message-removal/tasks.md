## 1. Broker

- [ ] 1.1 `Dest::remove_pending(message_id)`: remove one pending entry, keep FIFO, update `dequeued`, memory accounting and expiry index; `false` when not pending
- [ ] 1.2 `Dest::purge_pending()`: swap the pending map under the lock, update counters and index, drop entries after the lock; return the count
- [ ] 1.3 Released memory feeds the memory-limit state as for other releases
- [ ] 1.4 Tests in `tests/broker_semantics.rs`: FIFO kept after a delete, inflight untouched by purge, rollback after purge returns the message, counters and memory

## 2. Configuration

- [ ] 2.1 `admin.read_only` (boolean, default `false`), validation error naming the key; template entry

## 3. Console pages

- [ ] 3.1 Routes `GET`/`POST /queues/{name}/messages/{id}/delete` and `/queues/{name}/purge`; other methods `405`
- [ ] 3.2 Confirmation pages (what will be removed, inflight that stays, "cannot be undone", button, Cancel)
- [ ] 3.3 `POST` handlers: session, same-origin check, operation, info log, `303` to the queue page with the notice
- [ ] 3.4 "Delete message" link on the message page and "Purge" link on the queue page, hidden when read-only
- [ ] 3.5 Notice rendering on the queue page; styles for the destructive buttons in both themes

## 4. API

- [ ] 4.1 `DELETE /api/queues/{name}/messages/{id}` and `POST /api/queues/{name}/purge` with JSON results and `404` errors
- [ ] 4.2 Same-origin check for cookie-only requests; Basic requests without `Origin` accepted

## 5. Tests and documentation

- [ ] 5.1 `tests/admin_http.rs`: every scenario of the spec, including `403` cross-site, `401`, `405` in read-only mode, GET does not delete
- [ ] 5.2 Update existing tests that expect `405` for every write
- [ ] 5.3 README: admin console section (delete, purge, API, `read_only`)
- [ ] 5.4 CHANGELOG entry under Unreleased
- [ ] 5.5 `cargo fmt` and the test suite
