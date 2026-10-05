<p align="center"><img src="docs/assets/logo-wordmark.svg" alt="ActiveMQRust" width="420"></p>

# ActiveMQRust

An **in-memory** message broker written in **Rust** that speaks Apache ActiveMQ's **OpenWire** protocol.
Java applications that use the ActiveMQ driver (`activemq-client`) connect without code changes: just point them at `tcp://host:61616`.

The goal is a broker that is **compatible** with ActiveMQ, **uses less RAM** and is **faster** than ActiveMQ, shipped as a single Windows executable with no dependencies.

> **Project status:** design phase. The specifications are written as [OpenSpec](https://github.com/Fission-AI/OpenSpec) changes in [`openspec/changes/`](openspec/changes/), one per feature area; implementation has not started yet. The features below are **planned**.

## What it is good at

ActiveMQRust does one job: **process messages in RAM, with no storage**. There is no journal, no database and no disk I/O, so there is no fsync or I/O wait in the message path and no storage to manage. Messages live only in memory, from the moment a producer sends them until a consumer acknowledges them or they expire.

That makes it a good fit for **transient, high-rate messaging** between Java applications, where losing in-flight messages on a broker restart is acceptable:

- **Work queues**, where producers can resend or the work can be regenerated.
- **Request/reply** between services over temporary queues.
- **Events and notifications** fanned out on topics.
- **Cache invalidation**, **telemetry** and **status updates**, where only the latest information matters.
- **Short-lived buffers** that decouple producers from consumers on the same network.

Where it pays off compared to ActiveMQ:

- **Less RAM**: no JVM, no heap headroom, and compact message storage with large bodies compressed.
- **More speed**: native code, zero-copy message handling and no disk in the path.
- **Simpler operations**: one executable and nothing to install.

### When not to use it

- Messages that **must survive** a broker restart or crash, such as orders or payments: use a persistent broker.
- **Durable subscriptions** or **XA transactions**.
- Backlogs **larger than the available RAM**: messages are never paged to disk. Use `max_memory_mb` to reject producers instead of running out of memory.

After a restart the broker starts empty. Clients that use a `failover:` URL reconnect automatically.

## Planned features

- **OpenWire compatible**: transparently recognized by the ActiveMQ Java driver 5.18.x and 6.x (protocol versions 9–12).
- **A single `mqrust.exe`** for Windows x64, with no runtime to install (no Visual C++, .NET or Java).
- **Command-line program** that can **install and uninstall itself as a Windows service**.
- **Port 61616 by default**, works even without a configuration file.
- **Everything in RAM**: no persistence; messages are lost on restart.
- **Queues created automatically** on first use; unlimited producers and consumers.
- **FIFO delivery**: consumers receive messages in arrival order, including after redelivery.
- **Message IDs** structurally identical to ActiveMQ's.
- **JMS SQL-92 selectors** (`JMSCorrelationID IN ('A','C')`, `LIKE`, `BETWEEN`, …).
- **Message expiration** (time-to-live): expired messages are deleted from memory.
- **Compression**: messages compressed by the client (`useCompression=true`) pass through untouched; the broker quickly compresses those larger than 32 KB.
- **Non-durable topics**, **local transactions**, **temporary queues** (request/reply), **QueueBrowser**.
- **Authentication** with username and password (plain text or Argon2 hash).
- **Web admin console** behind a login: queues, consumers, producers, connections and queue contents, plus a JSON API.
- The broker identifies itself as **`ActiveMQRust <version>`**.

Not planned for the first version: persistence, durable subscriptions, XA transactions, wildcards, network of brokers, protocols other than OpenWire.

## Planned usage

```
mqrust.exe
```

With no arguments the broker listens on `0.0.0.0:61616` (OpenWire) and `http://127.0.0.1:8161` (admin console), with the default credentials `admin` / `admin`. Set your own passwords before exposing it on a network.

Nothing changes on the Java side:

```java
ActiveMQConnectionFactory factory = new ActiveMQConnectionFactory("tcp://localhost:61616");
Connection connection = factory.createConnection("admin", "admin");
```

### Command line

```
mqrust.exe                                     start the broker
mqrust.exe --config <file>                     use a specific configuration file
mqrust.exe --bind <ip> --port <n>              override the OpenWire address and port
mqrust.exe --admin-bind <ip> --admin-port <n>  override the admin address and port
mqrust.exe init-config                         write a commented mqrust.toml
mqrust.exe check-config                        validate the configuration
mqrust.exe hash-password                       generate the Argon2 hash of a password
mqrust.exe --version                           print "ActiveMQRust <version>"
mqrust.exe service install [--config <file>]   install as a Windows service (administrator)
mqrust.exe service uninstall                   stop and remove the Windows service
mqrust.exe service start | stop | status       control the installed service
```

### Configuration

The optional `mqrust.toml` file is looked up next to the executable. Missing keys take their default value.

```toml
[broker]
bind = "0.0.0.0"
port = 61616

[admin]
bind = "127.0.0.1"
port = 8161
username = "admin"
password_hash = "$argon2id$..."   # generated with: mqrust.exe hash-password

[[users]]
username = "app1"
password_hash = "$argon2id$..."
```

All options (memory, compression, expiration, logging) are described in the OpenSpec changes under [`openspec/changes/`](openspec/changes/).

## Admin console

The read-only web console answers on `http://127.0.0.1:8161/` (keys `[admin] bind` / `port`, or `--admin-bind` / `--admin-port`). If the port cannot be bound, the broker logs an error and keeps running without the console.

- **Login**: pages open a login form; a successful login starts a session held in memory, in an `HttpOnly`, `SameSite=Strict` cookie. The top bar shows the user and a **Log out** button. Sessions end after `session_idle_minutes` without requests, `session_max_hours` after login, or when the broker restarts. The login page warns when the built-in `admin` / `admin` credentials are in use.
- **Throttling**: after `login_max_failures` failed logins from one IP within 15 minutes, that IP is refused (`429`) for `login_lockout_seconds`. Failed logins are logged with IP and username, never the password.
- **Pages**: overview (version, uptime, connections, memory, broker compression counters), queues (every column sortable, numeric order, ties by name), queue detail with consumers, producers and paginated contents (50 per page), message detail (headers, properties, body by JMS type), topics and connections. Add `?refresh=5` to refresh every 5 seconds.
- **XML bodies**: a TextMessage holding well-formed XML gets a **Formatted** view (indented and coloured) next to the unchanged **Raw** view. DTDs and entities are never resolved; bodies above 1 MB are not formatted.
- **Theme**: light or dark following the operating system, or forced with the Auto / Light / Dark selector (remembered in a cookie). No JavaScript and no external resources are loaded.

```toml
[admin]
session_idle_minutes = 30    # 1-1440
session_max_hours = 8        # 1-168
login_max_failures = 5       # 0 disables the lockout
login_lockout_seconds = 60   # 1-86400
```

The console uses plain HTTP and binds to `127.0.0.1` by default. To reach it from other machines, put a TLS reverse proxy in front of it rather than binding it to a public address.

### JSON API

The same data is available as JSON for scripts and monitoring. API paths accept HTTP Basic with the `[admin]` credentials (or a browser session cookie); without valid credentials they answer `401 {"error":"unauthorized"}`, with no browser pop-up.

```
curl -u admin:admin http://127.0.0.1:8161/api/queues?sort=pending&order=desc
```

| Path | Returns |
| --- | --- |
| `/api/overview` | `{product, version, uptimeSeconds, openwire, admin, connections, queues, topics, messageMemory, memoryLimit, memoryLimitReached, compressed, compressDiscarded, workingSet, privateBytes}` |
| `/api/queues?sort=&order=` | `[{name, temporary, pending, inflight, consumers, producers, enqueued, consumed, expired, discarded, memory, compressed}]` (`sort`: name, pending, inflight, consumers, producers, enqueued, consumed, expired; `order`: asc, desc) |
| `/api/queues/{name}` | the queue fields plus `consumers: [{consumerId, connectionId, client, prefetch, inflight, selector, browser}]`, `producers: [{producerId, connectionId, client}]`, `withExpiration`, `nextExpiration`, `nextExpirationText` |
| `/api/queues/{name}/messages?offset=&limit=` | `{total, offset, limit, messages: [message]}` in FIFO order; `limit` defaults to 50 and is clamped to 1..50 |
| `/api/queues/{name}/messages/{id}?view=xml` | one message plus `inflight` and `body` (`{kind: text, text, truncated}`, `{kind: bytes, hex, size, truncated}`, `{kind: map, entries}`, `{kind: stream, values}`, `{kind: object, size}`, `{kind: none}`); with `view=xml`, `formattedBody` or `formatError` |
| `/api/topics` | `[{name, temporary, consumers, producers, published, discarded, pending, memory}]` |
| `/api/connections` | `[{connectionId, clientId, user, client, openwireVersion, connectedAt, sessions, consumers, producers}]` |

A `message` is `{position, messageId, correlationId, type, replyTo, deliveryMode, priority, timestamp, expiration, expirationText, expired, expiresInMs, redeliveryCounter, properties, bodyType, bodySize, compressed}`, plus `compressedSize` for a compressed body. `bodySize` is the size as stored (compressed size for a compressed body). Unknown queues and messages answer `404 {"error": "... not found"}`.

## Build

You need stable Rust with the `x86_64-pc-windows-msvc` target and Visual Studio Build Tools. The Build Tools are needed only to compile, not to run.

```
cargo build --release
```

The output is `target\release\mqrust.exe`. The C runtime is linked statically: the executable imports only Windows system DLLs.

## Acceptance test

`tests/java-it/` will contain a Java program that uses the original ActiveMQ driver and checks:

1. connecting, creating a queue, sending and reading back 10 messages in FIFO order, with identical IDs on the producer and consumer side;
2. a consumer with a selector on `JMSCorrelationID`, without losing the messages it does not select;
3. rejection of wrong credentials.

```
java -jar mqrust-acceptance.jar --url tcp://127.0.0.1:61616 --user admin --password admin
```

The same program is also run against a real ActiveMQ to confirm that both brokers behave the same way.

## Comparison with ActiveMQ

A planned benchmark uses the same Java client against both brokers on the same machine, with ActiveMQ configured without persistence so that both process messages in RAM. It measures:

- memory at idle;
- memory while holding **100,000 messages of 10 KB** each, and while holding **10,000 messages of 50 KB** each;
- time to **produce** and time to **consume** those messages;
- throughput with 1 KB messages.

Every test message is an XML `TextMessage` with 20 fields holding random values, plus a base64 buffer that pads the document to the exact target size. Payloads are generated from a fixed seed, so both brokers receive identical messages.

Results will be published in `docs/benchmarks/`.

## License

Released under the [MIT](LICENSE) license.
