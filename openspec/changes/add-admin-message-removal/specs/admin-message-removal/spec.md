## ADDED Requirements

### Requirement: Delete a pending message
The message page of a pending message SHALL show a "Delete message" link to `/queues/{name}/messages/{id}/delete`. A `GET` on that path SHALL show a confirmation page with the queue name, the message ID, the message type, the warning that the removal cannot be undone, a "Delete" button that sends a `POST` to the same path, and a "Cancel" link back to the message page. The `POST` SHALL remove the message from the queue if it is still pending and redirect (`303`) to the queue page with the notice "Message <id> deleted". If the message is inflight or no longer in the queue, nothing SHALL be removed and the notice SHALL be "Message <id> is no longer pending; nothing deleted". The other messages SHALL keep their FIFO order.

#### Scenario: Delete one message
- **WHEN** queue `Q1` holds messages M1, M2, M3 and an administrator confirms the deletion of M2
- **THEN** `Q1` holds M1 and M3, pending is 2, and a consumer created afterwards receives M1 then M3

#### Scenario: GET does not delete
- **WHEN** an authenticated client requests `GET /queues/Q1/messages/M2/delete`
- **THEN** a confirmation page is shown and `Q1` still holds M2

#### Scenario: Message already consumed
- **WHEN** M2 is dispatched to a consumer after the confirmation page was opened, and the administrator then confirms
- **THEN** nothing is removed, the consumer keeps M2, and the queue page shows "Message M2 is no longer pending; nothing deleted"

### Requirement: Purge a queue
The queue page SHALL show a "Purge" link to `/queues/{name}/purge`. A `GET` on that path SHALL show a confirmation page with the queue name, the current number of pending messages, the number of inflight messages that will stay with their consumers, the warning that the removal cannot be undone, a "Purge" button that sends a `POST` to the same path, and a "Cancel" link. The `POST` SHALL remove every message pending in the queue at that moment and redirect (`303`) to the queue page with the notice "Purged <n> messages from <name>". Inflight messages, consumers, producers and the queue itself SHALL be kept; messages arriving after the purge SHALL be kept. Purging SHALL be allowed on every queue, including temporary queues and `ActiveMQ.DLQ`.

#### Scenario: Purge with a consumer
- **WHEN** queue `Q1` has 100 pending messages and one consumer holding 5 unacknowledged messages, and an administrator confirms the purge
- **THEN** pending is 0, inflight is still 5, the consumer can acknowledge its 5 messages, and the notice says "Purged 100 messages from Q1"

#### Scenario: Purge the DLQ
- **WHEN** `ActiveMQ.DLQ` holds 20 messages and an administrator confirms its purge
- **THEN** `ActiveMQ.DLQ` is still listed with pending 0

#### Scenario: Rolled-back message after a purge
- **WHEN** a transacted consumer holds 1 message, the queue is purged, and the consumer rolls back
- **THEN** the message is pending again in the queue

### Requirement: Counters, memory and logging of removals
Each removed message SHALL add 1 to the queue's consumed (dequeued) counter, SHALL release its accounted message memory and SHALL be removed from any expiration index. When the released memory brings the broker below its limit, the broker SHALL leave the memory-limited state as for any other release. Each delete SHALL be logged at info level as `admin <user> from <ip> deleted message <id> from queue <name>`, and each purge as `admin <user> from <ip> purged queue <name>: <n> messages`. No body or property SHALL be logged.

#### Scenario: Counters after a purge
- **WHEN** a queue with enqueued 50, consumed 10 and pending 40 is purged
- **THEN** consumed is 50, pending is 0 and the broker's message memory drops by the size of the 40 messages

#### Scenario: Log line
- **WHEN** administrator `ops` connected from `127.0.0.1` purges `Q1` with 3 pending messages
- **THEN** the log contains `admin ops from 127.0.0.1 purged queue Q1: 3 messages`

### Requirement: Security of removals
Removals SHALL be performed only by `POST` (pages) or `DELETE`/`POST` (API) from an authenticated administrator. A page `POST`, and an API request authenticated only by the session cookie, SHALL pass the same-origin check used by logout; otherwise the response SHALL be `403` and nothing SHALL change. API requests authenticated with HTTP Basic SHALL NOT need an `Origin` header. Any other method on these paths SHALL receive `405`.

#### Scenario: Cross-site purge refused
- **WHEN** a logged-in browser sends `POST /queues/Q1/purge` with `Origin: https://evil.example`
- **THEN** the response is `403` and `Q1` is unchanged

#### Scenario: Unauthenticated purge refused
- **WHEN** a client without a session or credentials sends `POST /api/queues/Q1/purge`
- **THEN** the response is `401` and `Q1` is unchanged

### Requirement: Removal API
The API SHALL offer `DELETE /api/queues/{name}/messages/{id}`, answering `200` with `{"deleted": "<id>"}` when the pending message was removed and `404` with a JSON error object when the queue does not exist or the message is not pending, and `POST /api/queues/{name}/purge`, answering `200` with `{"purged": <n>}`, or `404` for an unknown queue.

#### Scenario: Purge by script
- **WHEN** a script with HTTP Basic credentials sends `POST /api/queues/Q1/purge` while `Q1` has 7 pending messages
- **THEN** the response is `200` with `{"purged": 7}`

#### Scenario: Delete a missing message
- **WHEN** a script sends `DELETE /api/queues/Q1/messages/ID:nope`
- **THEN** the response is `404` with a JSON error object

### Requirement: Read-only switch
The `[admin]` section SHALL accept `read_only`, a boolean, default `false`. With `read_only = true` the console SHALL NOT show the "Delete message" and "Purge" links, and the confirmation pages and every removal endpoint SHALL answer `405 Method Not Allowed` without changing anything. A non-boolean value SHALL be a configuration error naming `admin.read_only` with exit code 2.

#### Scenario: Read-only console
- **WHEN** `admin.read_only = true` and an administrator sends `POST /queues/Q1/purge`
- **THEN** the response is `405`, `Q1` is unchanged, and the queue page shows no "Purge" link
