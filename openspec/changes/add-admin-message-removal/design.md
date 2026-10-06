## Context

The console accepts only `GET`/`HEAD`, plus `POST /login` and `POST /logout`, which check `same_origin` (the `Origin` or `Referer` header must name the console host). Sessions use an `HttpOnly; SameSite=Strict` cookie; the API also accepts HTTP Basic. The CSP has `form-action 'self'` and no `script-src`. A queue keeps its pending messages in a `BTreeMap<u64, Entry>` keyed by broker sequence, with an expiry index and running memory counters; inflight messages are tracked per consumer.

## Goals / Non-Goals

**Goals:**
- Remove one pending message, or all pending messages of a queue, from the page and from the API.
- No accidental removal: explicit confirmation, `POST` only, cross-site requests refused.
- Counters and memory stay consistent; the operation is visible in the log.

**Non-Goals:**
- Removing inflight messages or taking them away from consumers.
- Moving, copying or resending messages; deleting queues; topics.
- Roles or per-user permissions (every console user is an administrator).

## Decisions

### D1. Confirmation page, then POST
"Delete message" and "Purge" are links to `GET /queues/{name}/messages/{id}/delete` and `GET /queues/{name}/purge`, which show what will be removed (message ID and type; queue name and current pending count) with a "Delete"/"Purge" button (a `POST` form to the same path) and a "Cancel" link. Only the `POST` changes state. After it, the browser is redirected (303) to the queue page with a notice ("Message … deleted", "Purged N messages from …", or "Message … is no longer pending; nothing deleted").
*Alternative*: a JavaScript `confirm()`. Rejected: the console has no JavaScript and `confirm()` is easy to click through.

### D2. Security for writes
Page `POST`s require a valid session and pass `same_origin`, like logout; `SameSite=Strict` and `form-action 'self'` add defence in depth. API calls authenticated by HTTP Basic do not need `Origin` (scripts do not send it); API calls authenticated only by the session cookie must pass `same_origin` too. A refused check gives `403` and changes nothing.
*Alternative*: a CSRF token in each form. Rejected for now: the origin check is already the console's mechanism for its other `POST`s, and with `SameSite=Strict` a cross-site form cannot carry the session.

### D3. Only pending messages
Delete looks up the message ID among the pending entries; when it is inflight or gone, nothing is removed (page notice, API `404` with an error object). Purge removes every entry pending at the moment the queue lock is taken; messages arriving afterwards stay; inflight messages stay with their consumers and follow the normal ack/redelivery path, so a rolled-back inflight message comes back to the queue after a purge.
*Alternative*: purge inflight messages too, as an ActiveMQ purge may. Rejected: taking messages from a live consumer breaks its acknowledgements; the console shows the inflight count so the operator knows what remains.

### D4. Counters and memory
Each removed message adds 1 to `dequeued` (the Consumed column), so `enqueued = consumed + pending + inflight + expired + discarded` keeps holding; it releases its accounted memory, which can bring the broker out of the memory-limited state, and it is removed from the expiry index. The pending count of the queue drops at once.
*Alternative*: a separate "removed" counter. Rejected: it adds a column to every table for a rare operation; the log records the details.

### D5. Purge in one lock
Purge swaps the pending map with an empty one under the queue lock and drops the entries after releasing it, so a large purge (millions of messages) does not hold the lock while freeing memory.
*Alternative*: remove entries one by one. Rejected: O(n) work under the lock blocks producers and consumers.

### D6. Logging
`info`: `admin <user> from <ip> deleted message <id> from queue <name>` and `admin <user> from <ip> purged queue <name>: <n> messages`. Message bodies and properties are never logged.

### D7. Read-only switch
`[admin] read_only = false` by default. With `true`, the buttons are not shown, the confirmation pages and the write endpoints answer `405`, as today.
*Alternative*: read-only by default. Rejected: the console requires an authenticated administrator; operators asked for the feature to be available.

## Risks / Trade-offs

- [An operator purges the wrong queue] → confirmation page with the queue name and count; info log with user and IP. Messages are in RAM: there is no undo, and the confirmation page says so.
- [Message removed while being viewed] → delete reports "no longer pending" instead of failing.
- [Scripts with Basic auth can remove messages] → intended for automation; `read_only = true` disables it.
