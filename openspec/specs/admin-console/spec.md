# admin-console Specification

## Purpose
Defines the read-only web administration console of the broker: the admin HTTP listener, authenticated access, server-side HTML pages for overview, queues, queue contents, messages, topics and connections, and the JSON API.
## Requirements
### Requirement: Admin HTTP listener
The broker SHALL serve the admin console over HTTP on the address given by the `[admin]` keys `bind` (default `127.0.0.1`) and `port` (default `8161`), overridden by `--admin-bind` and `--admin-port`. By default the console SHALL be reachable only from the local machine; exposing it on the network SHALL require explicit configuration. At startup the broker SHALL log `admin listening on http://<bind>:<port>`. If the admin address cannot be bound, the broker SHALL log an error naming the address and SHALL keep running without the admin console: the OpenWire listener and all messaging SHALL be unaffected.

#### Scenario: Admin port already in use
- **WHEN** port 8161 is already in use at startup
- **THEN** the broker logs an error naming `127.0.0.1:8161`, starts the OpenWire listener on 61616 normally, and Java clients can connect and exchange messages

#### Scenario: Default address
- **WHEN** the broker starts without a configuration file
- **THEN** the console answers on `http://127.0.0.1:8161/` and does not accept connections on other interfaces

#### Scenario: Configured address
- **WHEN** the configuration sets `[admin] bind = "0.0.0.0"` and `port = 8200`
- **THEN** the console answers on port 8200 on all interfaces and the startup log shows `admin listening on http://0.0.0.0:8200`

#### Scenario: Command-line override
- **WHEN** the configuration sets `[admin] port = 8200` and the broker is started with `--admin-port 8300`
- **THEN** the console answers on port 8300

### Requirement: HTTP Basic authentication
Every console path, HTML and JSON, SHALL require authentication with the `[admin]` credentials (`username` with `password` or `password_hash`), or `admin`/`admin` when there is no configuration file. HTML pages SHALL authenticate with the login form and session cookie defined by `admin-login`; a request without a valid session SHALL be redirected to `/login` and receive no broker data. Paths under `/api/` SHALL accept HTTP Basic credentials or the session cookie, and without valid credentials SHALL answer `401` with no broker data and without a `WWW-Authenticate` header. Admin credentials SHALL be independent of the OpenWire `[[users]]`.

#### Scenario: No credentials
- **WHEN** a client requests `/queues` without a session cookie
- **THEN** the response is `303` to `/login?next=%2Fqueues` and the body contains no queue names

#### Scenario: No credentials on the API
- **WHEN** a client requests `/api/overview` without a session cookie or an `Authorization` header
- **THEN** the response is `401` without `WWW-Authenticate`

#### Scenario: Wrong password
- **WHEN** a client posts the admin username and a wrong password to `/login`
- **THEN** no session is created and the login page shows "Invalid username or password"

#### Scenario: Valid credentials with Argon2 hash
- **WHEN** `[admin]` uses `password_hash` and a client logs in with the matching password
- **THEN** it receives a session cookie and `/` answers `200` with the overview page

#### Scenario: OpenWire user cannot log in to the console
- **WHEN** `[[users]]` contains `app1`/`secret`, `[admin]` uses another username, and a client logs in as `app1`/`secret`, or calls `/api/overview` with Basic `app1`/`secret`
- **THEN** the login fails and the API answers `401`

### Requirement: Failed admin login logging
Every failed login, through the login form or through HTTP Basic on the API, SHALL be logged as a warning with the remote IP and the username; a refused attempt during a lockout SHALL be logged once per lockout. The password and the session token SHALL NOT appear in any log line at any level. A request without credentials SHALL NOT be logged as a failed login.

#### Scenario: Wrong credentials are logged without password
- **WHEN** a client at `127.0.0.1` posts `admin` with password `wrong-pw-123` to `/login`
- **THEN** a warning containing `127.0.0.1` and `admin` is logged, and the string `wrong-pw-123` appears in no log line

#### Scenario: First browser request is not a failed login
- **WHEN** a client requests `/` without a session
- **THEN** the response redirects to `/login` and no failed-login warning is logged

### Requirement: Read-only console
The console SHALL NOT change broker state. Only `GET` and `HEAD` SHALL be accepted, except `POST /login` and `POST /logout`, which change only the console session; any other method or path SHALL receive `405 Method Not Allowed`. Viewing queue contents or a message, in any view, SHALL NOT consume, acknowledge, reorder or redeliver messages, and SHALL NOT change any counter.

#### Scenario: Write method rejected
- **WHEN** an authenticated client sends `POST /queues/TEST.A` or `DELETE /api/queues/TEST.A`
- **THEN** the response is `405` and the queue is unchanged

#### Scenario: Browsing does not consume
- **WHEN** a queue holds 10 messages and an authenticated client views its contents page and every message detail page, raw and formatted
- **THEN** a consumer created afterwards receives all 10 messages in FIFO order with `JMSRedelivered=false`, and the queue counters are unchanged by the views

### Requirement: Server-side HTML with embedded assets
Pages SHALL be generated server-side as HTML, with CSS embedded in the executable and no JavaScript. Every value taken from broker data (destination names, IDs, properties, bodies, usernames) SHALL be HTML-escaped. Responses SHALL carry `Content-Security-Policy: default-src 'none'; style-src 'self'; form-action 'self'; frame-ancestors 'none'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin` and, on pages behind login, `Cache-Control: no-store`. Every page behind login SHALL show the logged-in username and a logout button in its top bar. Every page SHALL support optional auto-refresh every 5 seconds, enabled with the query parameter `refresh=5` and kept on the page's links.

#### Scenario: Escaped content
- **WHEN** a queue named `Q<script>` holds a TextMessage whose text and a property value are `<script>alert(1)</script>`
- **THEN** the queues page, the queue detail page and the message page contain the escaped text `&lt;script&gt;` and no `<script>` element

#### Scenario: Auto-refresh
- **WHEN** a logged-in client requests `/queues?refresh=5`
- **THEN** the page contains `<meta http-equiv="refresh" content="5">` and the links on the page keep `refresh=5`

#### Scenario: No asset files on disk
- **WHEN** the broker runs from a folder that contains only `mqrust.exe`
- **THEN** every console page renders with its stylesheet

#### Scenario: User and logout in the top bar
- **WHEN** a client logged in as `admin` opens any page
- **THEN** the top bar shows `admin` and a logout button

#### Scenario: Pages not cached
- **WHEN** a logged-in client requests `/queues`
- **THEN** the response carries `Cache-Control: no-store`

### Requirement: Overview page
`/` SHALL show: `ActiveMQRust <version>` with the crate version; the OpenWire listen address and the admin console URL as two labelled lines directly below the page title, outside the figure cards; and, as cards, uptime; the number of active connections; the number of queues and of topics; the message memory in use and the configured limit (or "no limit"); and the process memory, as Working Set (RSS) and Private Bytes, read from the operating system at request time.

#### Scenario: Product identity and counts
- **WHEN** an authenticated client requests `/` while two OpenWire connections are open and three queues exist
- **THEN** the page shows `ActiveMQRust <crate version>`, 2 active connections, 3 queues, and both listen addresses

#### Scenario: Memory values
- **WHEN** an authenticated client requests `/`
- **THEN** the page shows the process Working Set in MB within 5% of the value reported by `Get-Process -Id <pid>` at the same time, and the message memory limit as "no limit" when `max_memory_mb = 0`

#### Scenario: Addresses under the title
- **WHEN** an authenticated client requests `/` on a broker listening on `0.0.0.0:61616` with the console on `127.0.0.1:8161`
- **THEN** right below the `<h1>` the page shows one line `OpenWire tcp://0.0.0.0:61616` and one line `Admin console http://127.0.0.1:8161`, and no figure card contains either address

### Requirement: Queues page
`/queues` SHALL list every queue, including temporary queues and `ActiveMQ.DLQ`, and SHALL NOT list advisory destinations. Each row SHALL show: name (linking to the queue detail), pending messages, inflight messages, consumers, producers, total enqueued, total consumed, and expired (when message expiration is present). The table SHALL be sortable by any column through `sort=<column>` and `order=asc|desc`, with name ascending as the default.

#### Scenario: Counts per queue
- **WHEN** queue `Q1` has 5 pending messages, 2 consumers holding 3 unacknowledged messages, and 1 producer
- **THEN** the `Q1` row shows pending 5, inflight 3, consumers 2, producers 1

#### Scenario: Sort by pending
- **WHEN** an authenticated client requests `/queues?sort=pending&order=desc`
- **THEN** the rows are ordered by pending messages, highest first

#### Scenario: Empty queue remains listed
- **WHEN** a queue has no messages, no consumers and no producers
- **THEN** it is still listed with zero counts

#### Scenario: Advisory topics hidden
- **WHEN** a Java client with default settings has created its advisory consumer
- **THEN** no `ActiveMQ.Advisory.*` destination appears on any console page or API response

### Requirement: Queue detail page
`/queues/{name}` SHALL show the queue counters (as on `/queues`, plus the message memory used by the queue, each stored message counted once), the list of consumers (consumer ID, connection ID, client IP, prefetch size, inflight count), the list of producers (producer ID, connection ID, client IP), and the queue contents. `{name}` SHALL be the percent-encoded queue name. An unknown queue SHALL give `404`.

#### Scenario: Consumers and producers
- **WHEN** queue `Q1` has one consumer with prefetch 1000 holding 2 inflight messages and one producer
- **THEN** the detail page lists the consumer with its consumer ID, connection ID, client IP, prefetch 1000 and inflight 2, and lists the producer with its producer ID and client IP

#### Scenario: Unknown queue
- **WHEN** an authenticated client requests `/queues/DOES.NOT.EXIST`
- **THEN** the response is `404`

#### Scenario: Name with special characters
- **WHEN** a queue is named `orders/eu 1`
- **THEN** its link on `/queues` is percent-encoded and opens its detail page

### Requirement: Queue contents pagination
The queue detail page SHALL list the pending messages in FIFO order (`broker_seq` order), 50 per page, selected with `page=<n>` (1-based, default 1), with links to the previous and next pages and the total number of pending messages. Each row SHALL show the position, `JMSMessageID` (linking to the message page), timestamp, JMS type, correlation ID, body type and body size. A page number beyond the last page SHALL show an empty list, not an error.

#### Scenario: First page
- **WHEN** a queue holds 120 messages sent as `m-1` … `m-120` and an authenticated client requests its detail page
- **THEN** the contents list shows exactly `m-1` … `m-50` in that order and a link to page 2

#### Scenario: Last page
- **WHEN** the same client requests `page=3`
- **THEN** the list shows exactly `m-101` … `m-120`

#### Scenario: Beyond the last page
- **WHEN** the same client requests `page=9`
- **THEN** the response is `200` with an empty list

### Requirement: Message detail page
`/queues/{name}/messages/{id}`, where `{id}` is the percent-encoded `JMSMessageID`, SHALL show the JMS headers MessageID, CorrelationID, Type, ReplyTo, DeliveryMode, Priority, Timestamp, Expiration and RedeliveryCounter; the application properties with name, type and value; and the body rendered by message type. A message that has been delivered to a consumer and is waiting for its acknowledgement SHALL still be shown, with a notice that it is in flight. A message that is neither pending nor in flight (consumed and acknowledged, expired or removed) SHALL give `404`. Looking up a message SHALL use the `seq` hint of the contents links first and SHALL NOT hold the queue lock while walking more than 10,000 messages at a time.

#### Scenario: Message in flight
- **WHEN** a pending message is dispatched to a consumer that has not acknowledged it yet and its message page is requested
- **THEN** the response is `200` with its headers and body and a notice that the message is in flight

#### Scenario: Headers and properties
- **WHEN** a TextMessage with correlation ID `ORD-A`, type `order`, priority 7 and an int property `seq=3` is pending
- **THEN** its message page shows those header values and a property row `seq`, `int`, `3`

#### Scenario: Message already consumed
- **WHEN** a message is consumed and acknowledged and then its message page is requested
- **THEN** the response is `404`

### Requirement: Body rendering by message type
The message page SHALL render the body according to the message type:
- TextMessage: the text, truncated at 64 KB with a visible truncation notice;
- BytesMessage: a hex dump of the first 4 KB, with a notice when the body is longer;
- MapMessage: a table of key, type and value;
- ObjectMessage: only the size and the words "serialized Java object"; the broker SHALL NOT deserialize Java objects;
- StreamMessage: the list of decoded values with their types;
- a message with no body: "no body".

#### Scenario: Long text
- **WHEN** a TextMessage of 100 KB is pending
- **THEN** the page shows its first 64 KB and a truncation notice

#### Scenario: Bytes
- **WHEN** a BytesMessage of 10 KB is pending
- **THEN** the page shows a hex dump of exactly the first 4096 bytes and a notice that the body is 10240 bytes long

#### Scenario: Map
- **WHEN** a MapMessage with `name` (String `abc`) and `qty` (int `5`) is pending
- **THEN** the page shows a table with rows `name`, `String`, `abc` and `qty`, `int`, `5`

#### Scenario: Object not deserialized
- **WHEN** an ObjectMessage containing a serialized `java.util.Date` is pending
- **THEN** the page shows its size and "serialized Java object", and no class from the payload is named or loaded

#### Scenario: Stream
- **WHEN** a StreamMessage with values `true`, `42L` and `"x"` is pending
- **THEN** the page lists `boolean true`, `long 42`, `String x` in order

### Requirement: Compressed bodies in the console
For a message with `compressed=true`, the console SHALL inflate the body (zlib format, following the per-type compressed layout of the Java client) before rendering it, and SHALL stop inflating after 64 KB of output, showing a notice. The message page SHALL show that the message is compressed and its compressed size. Inflating for the console SHALL NOT change the stored message.

#### Scenario: Client-compressed text
- **WHEN** a Java client with `useCompression=true` sends a 50 KB TextMessage
- **THEN** the message page shows the original text (first 50 KB), the flag "compressed" and the compressed size, and a consumer still receives the message byte-for-byte as sent

#### Scenario: Zip bomb
- **WHEN** a pending message has a compressed body that inflates to 100 MB
- **THEN** the page renders the first 64 KB with a notice and the request completes without inflating the rest

### Requirement: Topics page
`/topics` SHALL list every non-advisory topic, including temporary topics, with: name, consumers, producers, published messages and discarded messages. When topic messaging is not present the page SHALL render an empty table.

#### Scenario: Topic statistics
- **WHEN** topic messaging is present, topic `T1` has 3 subscribers and 1 producer, and 10 messages are published
- **THEN** the `T1` row shows consumers 3, producers 1, published 10

### Requirement: Connections page
`/connections` SHALL list every open OpenWire connection with: ConnectionId, username, remote IP and port, negotiated OpenWire version, connection time, and the number of sessions, consumers and producers.

#### Scenario: One client
- **WHEN** a Java 6.x client connects as `app1` and opens one session with one consumer and one producer
- **THEN** `/connections` lists its ConnectionId, `app1`, its remote address, OpenWire version 12, its connection time, 1 session, 1 consumer and 1 producer

#### Scenario: Closed connection disappears
- **WHEN** that client closes its connection
- **THEN** the connection is no longer listed

### Requirement: JSON API
The console SHALL expose the same data as JSON, with the same authentication, at `/api/overview`, `/api/queues`, `/api/queues/{name}`, `/api/queues/{name}/messages?offset=<n>&limit=<n>`, `/api/topics` and `/api/connections`, with `Content-Type: application/json`. The messages endpoint SHALL return messages in FIFO order starting at `offset` (default 0), with `limit` defaulting to 50 and clamped to 1..50; each entry SHALL contain the JMS headers, the application properties, the body type, the body size as stored (`bodySize`, the compressed size for a compressed body), the compressed flag and, for a compressed body, `compressedSize`. `/api/queues/{name}` SHALL also list the consumers (consumer ID, connection ID, client, prefetch, inflight, selector) and producers. `/api/overview` SHALL also report the broker compression counters `compressed` and `compressDiscarded`, which the overview page shows too. Unknown queues SHALL give `404` with a JSON error object.

#### Scenario: Queue list as JSON
- **WHEN** an authenticated client requests `/api/queues` after sending 3 messages to a new queue `Q1` with no consumers
- **THEN** the response is valid JSON containing an entry for `Q1` with pending 3, inflight 0, consumers 0 and enqueued 3

#### Scenario: Limit capped
- **WHEN** an authenticated client requests `/api/queues/Q1/messages?offset=10&limit=500` on a queue with 1000 messages
- **THEN** the response contains exactly 50 messages, the 11th to the 60th in FIFO order

#### Scenario: Overview identity
- **WHEN** an authenticated client requests `/api/overview`
- **THEN** the JSON contains the product `ActiveMQRust`, the crate version, uptime, connection count, queue and topic counts, message memory used and limit, and process working set and private bytes

### Requirement: Snapshot consistency
The console SHALL read broker state through snapshots taken under each destination's lock and released immediately, before formatting or network I/O. A contents page or messages API call SHALL copy at most 50 messages, never the whole queue. Each page SHALL be internally consistent per destination: counts and rows of one destination come from the same snapshot. Admin requests SHALL NOT block message traffic for longer than one snapshot copy.

Queue counters SHALL NOT require walking the pending messages: the bytes held and the number of compressed messages SHALL be kept as running per-destination counters, each stored message counted once even when several topic subscriptions hold a copy. Reaching a deep page SHALL walk the pending messages from the nearer end in steps of at most 10,000 entries, releasing the lock between steps.

#### Scenario: Large queue page
- **WHEN** a queue holds 20,000 messages and a page is requested at any offset, or with `limit=100000`
- **THEN** at most 50 message references are copied for the request, and the page holds the messages at that offset in FIFO order

#### Scenario: Traffic during polling
- **WHEN** a producer sends 20,000 messages to a queue while a client polls that queue's API endpoints continuously
- **THEN** every send completes, every admin request succeeds, and afterwards the queue reports 20,000 pending and 20,000 enqueued messages with message memory equal to the broker's

### Requirement: Expiration fields when expiration is present
When message expiration is present, the console SHALL show: the `expired` counter per queue on `/queues` and `/queues/{name}`; on the queue detail page, the number of pending messages that have an expiration and the next expiration time; on the message page and in the contents list, `Expiration` as a local date and time plus the remaining time, or "expired" if it has passed. Pending messages that have expired but have not yet been removed SHALL be marked "expired" in the contents list; the console computes this itself (`expiration > 0` and not after the current time). A message with `expiration = 0` SHALL show "never". The JSON API SHALL give, per message, `expired` (boolean), `expirationText` (the same readable text) and `expiresInMs` (remaining time, or `null`), and per queue `withExpiration`, `nextExpiration` and `nextExpirationText`.

#### Scenario: Next expiration
- **WHEN** a queue holds a message with a TTL of 60 s and one without TTL
- **THEN** the detail page shows 1 message with an expiration and a next expiration about 60 s after the send time

#### Scenario: Expired counter
- **WHEN** 3 messages with a 100 ms TTL expire without being consumed
- **THEN** the queue's `expired` value on `/queues` and `/api/queues` is 3

### Requirement: Consumer selector when selectors are present
When message selectors are present, the queue detail page and `/api/queues/{name}` SHALL show each consumer's selector text, or nothing for a consumer without a selector.

#### Scenario: Selector shown
- **WHEN** a consumer is created with selector `JMSCorrelationID IN ('ORD-A','ORD-C')`
- **THEN** the queue detail page shows that selector text, HTML-escaped, in that consumer's row

### Requirement: Compression statistics when broker compression is present
When broker-side compression is present, the queue contents list and the messages API SHALL mark each compressed message and show its compressed size, and the queue counters SHALL include the number of compressed pending messages (`compressed` in the JSON API), from a running counter.

#### Scenario: Broker-compressed message
- **WHEN** broker compression is present and an uncompressed 100 KB TextMessage of repeated text is sent
- **THEN** its row in the contents list is marked compressed with a size smaller than 100 KB, and its message page shows the original text

