<p align="center"><img src="docs/assets/logo-wordmark.svg" alt="ActiveMQRust" width="420"></p>

<p align="center"><a href="https://github.com/matteobaccan/ActiveMQRust/actions/workflows/build.yml"><img src="https://github.com/matteobaccan/ActiveMQRust/actions/workflows/build.yml/badge.svg" alt="Build and Test ActiveMQRust"></a></p>

# ActiveMQRust

An **in-memory** message broker written in **Rust** that speaks Apache ActiveMQ's **OpenWire** protocol.
Java applications that use the ActiveMQ driver (`activemq-client`) connect without code changes: just point them at `tcp://host:61616`.

The goal is a broker that is **compatible** with ActiveMQ, **uses less RAM** and is **faster** than ActiveMQ, shipped as a single Windows executable with no dependencies.

> **Project status:** version **0.2.0**. The broker is implemented and passes every Rust and Java test suite listed in [Tests and results](#tests-and-results), which also holds all the load-test results against ActiveMQ 5.18.7 and 6.3.2. The specifications are [OpenSpec](https://github.com/Fission-AI/OpenSpec) specs in [`openspec/specs/`](openspec/specs/).

## Performance at a glance

Measured on the same Windows machine against Apache ActiveMQ 5.18.7 and 6.3.2 (both configured to keep everything in RAM), with the **same Java client** (`activemq-client` 5.18.7) for every broker: only the server changes. Every message was checked for losses, duplicates and order: ActiveMQRust delivered every message, in order and exactly once, in every run.

| | **ActiveMQRust** | ActiveMQ 5.18.7 | ActiveMQ 6.3.2 |
|---|---|---|---|
| Start-up (process start → port open) | **57–72 ms** | 1.8–2.1 s | 1.8–2.2 s |
| Memory at a steady 100 msg/s | **10.6 MB** | 182 MB | 193 MB |
| CPU per 1,000 messages at a steady 100 msg/s | **301 ms** | 561 ms | 533 ms |
| 50 KB messages, 20 + 20 clients at full speed | **529 MB/s, 153 MB RAM** | 404 MB/s, 1,080 MB | out of memory ¹ |
| 15 KB messages, 20 + 20 clients at full speed | **1.66 million delivered, 390 MB/s** | out of memory ¹ | out of memory ¹ |
| 300 KB messages, 20 + 20 clients at full speed | **547 MB/s, 202 MB RAM** | 545 MB/s, 1,237 MB | 544 MB/s, 1,097 MB |

¹ The 4 GB JVM heap filled up with queued messages and the broker closed the connections; most messages were never delivered. With an **8 GB heap** ActiveMQ still ran out of memory at 15 KB, and at 50 KB ActiveMQ 6.3.2 completed at 405 MB/s with a 7.7 GB peak (ActiveMQRust: 529 MB/s, 225 MB peak). Details, every figure and the test conditions are in [Load tests](#load-tests).


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

## Features

- **OpenWire compatible**: transparently recognized by the ActiveMQ Java driver 5.18.x, 5.19.x and 6.x (protocol versions 9–12).
- **A single `mqrust.exe`** for Windows x64, with no runtime to install (no Visual C++, .NET or Java).
- **Command-line program** that can **install and uninstall itself as a Windows service**.
- **Port 61616 by default**, works even without a configuration file.
- **Everything in RAM**: no persistence; messages are lost on restart.
- **Queues created automatically** on first use; unlimited producers and consumers.
- **FIFO delivery**: consumers receive messages in arrival order, including after redelivery.
- **Message IDs** structurally identical to ActiveMQ's.
- **JMS SQL-92 selectors** (`JMSCorrelationID IN ('A','C')`, `LIKE`, `BETWEEN`, …).
- **Message expiration** (time-to-live): expired messages are deleted from memory.
- **Compression**: messages compressed by the client (`useCompression=true`) pass through untouched. The broker can also compress large bodies itself to save RAM (`compress_threshold_kb`); this is off by default because it costs CPU and throughput (see [Load tests](#load-tests)).
- **Non-durable topics**, **local transactions**, **temporary queues** (request/reply), **QueueBrowser**.
- **Authentication** with username and password (plain text or Argon2 hash).
- **Web admin console** with a login form and sessions: queues (every column sortable), consumers, producers, connections and queue contents, a formatted view of XML bodies, a responsive layout with light and dark themes that follow the system, plus a JSON API.
- **Guided setup from the command line**: `mqrust.exe -h` shows the four steps to get started; `set-admin` and `user add` / `passwd` / `remove` / `list` manage the console user and the messaging users without editing the file by hand.
- The broker identifies itself as **`ActiveMQRust <version>`**.

Not in this version: persistence, durable subscriptions, XA transactions, wildcards, network of brokers, protocols other than OpenWire.

## Download

The [latest release](https://github.com/matteobaccan/ActiveMQRust/releases/latest) has:

| System | Download | Contents |
|---|---|---|
| Windows x86_64 | `activemq-rust-windows-x86_64-<version>.zip` | `mqrust.exe`, LICENSE, README.md, `mqrust.example.toml` |
| macOS ARM64 (Apple Silicon) | `activemq-rust-macos-arm64-<version>.tar.gz` | `mqrust`, LICENSE, README.md, `mqrust.example.toml` |

On macOS the commands are the same as below, with `./mqrust` instead of `mqrust.exe`:

```
tar -xzf activemq-rust-macos-arm64-<version>.tar.gz
xattr -d com.apple.quarantine mqrust     # the build is not signed: allow it once
./mqrust init-config
./mqrust
```

The Windows service (`mqrust.exe service ...`) is available on Windows only: on macOS the `service` commands exit with code 2 and the broker runs in the foreground (Ctrl+C stops it). In the admin console on macOS, *Working Set* is the resident size of the process and *Private Bytes* its physical footprint (the "Memory" column of Activity Monitor).

## Getting started

```
mqrust.exe init-config          1. create mqrust.toml next to the executable
mqrust.exe set-admin            2. choose the admin console user and password
mqrust.exe user add app1        3. create a user for the JMS/OpenWire clients
mqrust.exe user remove admin       (and drop the template user admin/admin)
mqrust.exe                      4. start the broker
```

`mqrust.exe -h` shows these steps; `mqrust.exe --help` adds the details, examples and exit codes.

The broker has **two kinds of users**:

| | Section | Used by | Set with |
|---|---|---|---|
| Admin console user | `[admin]` | the web console at `http://127.0.0.1:8161` and its JSON API | `mqrust.exe set-admin` |
| Messaging users | `[[users]]` | JMS/OpenWire clients (`tcp://host:61616`) | `mqrust.exe user add` / `passwd` / `remove` / `list` |

With no configuration file the broker still starts, on `0.0.0.0:61616` (OpenWire) and `http://127.0.0.1:8161` (admin console), with `admin` / `admin` for both kinds of users, and logs a warning naming the commands above. Set your own passwords before exposing it on a network.

Passwords are typed twice with hidden input and stored only as Argon2id hashes. They need at least 8 characters, must differ from the username, and cannot be `admin` or `password`. Usernames are 1–64 letters, digits, `.`, `_`, `-` or `@`.

The commands edit the file in place: comments, key order and the other settings are kept, the result is validated before it is written, and the file is replaced atomically. A changed file applies when the broker is restarted (`mqrust.exe service stop` and `service start` for the Windows service).

For unattended installs, read the password from standard input instead of the prompt:

```powershell
Get-Content admin-secret.txt | mqrust.exe set-admin --username ops --password-stdin
Get-Content app1-secret.txt  | mqrust.exe user add app1 --password-stdin
```

Nothing changes on the Java side:

```java
ActiveMQConnectionFactory factory = new ActiveMQConnectionFactory("tcp://localhost:61616");
Connection connection = factory.createConnection("app1", "<app1 password>");
```

### Command line

```
mqrust.exe                                     start the broker
mqrust.exe --config <file>                     use a specific configuration file
mqrust.exe --bind <ip> --port <n>              override the OpenWire address and port
mqrust.exe --admin-bind <ip> --admin-port <n>  override the admin address and port
mqrust.exe --processors <n>                    processors to use (0 = all available)
mqrust.exe init-config                         write a commented mqrust.toml (never overwrites)
mqrust.exe set-admin [--username <name>]       set the admin console user and password
mqrust.exe user add <name>                     add a messaging user
mqrust.exe user passwd <name>                  change the password of a messaging user
mqrust.exe user remove <name>                  remove a messaging user (not the last one)
mqrust.exe user list                           list the messaging usernames
mqrust.exe hash-password                       print the Argon2id hash of a password
mqrust.exe check-config                        validate the configuration
mqrust.exe --version                           print "ActiveMQRust <version>"
mqrust.exe service install [--config <file>]   install as a Windows service (administrator)
mqrust.exe service uninstall                   stop and remove the Windows service
mqrust.exe service start | stop | status       control the installed service
```

`set-admin`, `user add`, `user passwd` and `hash-password` accept `--password-stdin`. Every setup command accepts `--config <file>` to edit a file other than `mqrust.toml` next to the executable.

Exit codes: `0` success, `1` runtime error (for example a port already in use), `2` configuration or usage error.

### Configuration

The configuration file is searched in this order: `--config <file>`, then `mqrust.toml` next to `mqrust.exe`, then the built-in defaults. Every key is optional; missing keys take their default value.

[`mqrust.example.toml`](mqrust.example.toml) documents every key with its default; it is the same file `mqrust.exe init-config` writes. A minimal file looks like this:

```toml
[broker]
bind = "0.0.0.0"
port = 61616

[admin]
bind = "127.0.0.1"
port = 8161
username = "ops"
password_hash = "$argon2id$..."   # written by: mqrust.exe set-admin

[[users]]
username = "app1"
password_hash = "$argon2id$..."   # written by: mqrust.exe user add app1
```

Check a file before starting the broker with `mqrust.exe check-config --config <file>`.

## Admin console

The read-only web console answers on `http://127.0.0.1:8161/` (keys `[admin] bind` / `port`, or `--admin-bind` / `--admin-port`). If the port cannot be bound, the broker logs an error and keeps running without the console.

- **Login**: pages open a login form; a successful login starts a session held in memory, in an `HttpOnly`, `SameSite=Strict` cookie. The top bar shows the user and a **Log out** button. Sessions end after `session_idle_minutes` without requests, `session_max_hours` after login, or when the broker restarts. The login page warns when the built-in `admin` / `admin` credentials are in use.
- **Throttling**: after `login_max_failures` failed logins from one IP within 15 minutes, that IP is refused (`429`) for `login_lockout_seconds`. Failed logins are logged with IP and username, never the password.
- **Pages**: overview (version, uptime, connections, memory, broker compression counters), queues (every column sortable, numeric order, ties by name), queue detail with consumers, producers and paginated contents (50 per page), message detail (headers, properties, body by JMS type), topics and connections. Add `?refresh=5` to refresh every 5 seconds.
- **XML bodies**: a TextMessage holding well-formed XML gets a **Formatted** view (indented and coloured) next to the unchanged **Raw** view. DTDs and entities are never resolved; bodies above 1 MB are not formatted.
- **Theme**: follows the system light/dark setting. No JavaScript and no external resources are loaded.

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

`scripts\test.cmd` formats the sources (`cargo fmt`) and runs the tests in release mode (`cargo test --release`), as the CI does on Windows and macOS.

## Acceptance test

`tests/java-it/` contains a Java program that uses the original ActiveMQ driver and checks:

1. connecting, creating a queue, sending and reading back 10 messages in FIFO order, with identical IDs on the producer and consumer side;
2. a consumer with a selector on `JMSCorrelationID`, without losing the messages it does not select;
3. rejection of wrong credentials.

It needs a JDK (17 or later); Maven is downloaded by the Maven Wrapper. `run-acceptance.cmd` builds the program for the chosen driver (`amq5` = 5.18.x, `amq6` = 6.x) and runs it.

Against ActiveMQRust:

```
cargo build --release
target\release\mqrust.exe
tests\java-it\run-acceptance.cmd amq5 --url tcp://127.0.0.1:61616 --user admin --password admin
tests\java-it\run-acceptance.cmd amq6 --url tcp://127.0.0.1:61616 --user admin --password admin
```

Use the credentials of a messaging user if you created one (`--user app1 --password ...`), and `--only 1|2|3` to run a single scenario.

The same program is also run against a real ActiveMQ to confirm that both brokers behave the same way. Download Apache ActiveMQ 5.18.x or 6.x, unpack it, and start it in the foreground (its default configuration listens on 61616 with `admin`/`admin`):

```
pwsh scripts\start-activemq.ps1 -ActiveMQHome C:\tools\apache-activemq-6.3.2 -Config default
tests\java-it\run-acceptance.cmd amq6 --url tcp://127.0.0.1:61616 --user admin --password admin
```

`-Config reference` (the default) and `-Config tuned` start it with the non-persistent configurations used by the benchmark. Stop it with Ctrl+C; the script uses a fresh temporary data directory every time.

## Tests and results

All suites below were run on 2026-10-06 on version 0.3.0 (Windows 11, Intel Xeon W-2123, JDK 21), and all of them pass. The Java suites use the ActiveMQ client 5.19.11 (`amq5` profile) and 6.3.2 (`amq6` profile); on every push the Rust suites also run on Windows and macOS in CI.

### Functional tests

| Suite | What it checks | Result |
|---|---|---|
| Rust unit tests (`cargo test --release`, library) | codec, configuration, selectors, compression, sessions, XML formatter, CPU sizing, setup commands | 118 passed |
| `tests/broker_semantics.rs` | FIFO, round-robin, redelivery position, prefetch, acknowledgement types, DLQ, expiration, topics, browsers, transactions, memory limit, concurrent producers | 60 passed |
| `tests/connection_semantics.rs` | the broker driven over a real socket by a small OpenWire client | 14 passed |
| `tests/selector_semantics.rs` | 568 selectors checked against the results of ActiveMQ's own selector engine | 9 passed |
| `tests/admin_http.rs` | login, sessions, lockout, logout, API with Basic, headers, escaping, paging, system theme, addresses | 19 passed |
| `tests/cli_setup.rs` | help, `set-admin`, `user` commands, safe file editing, start-up messages, end to end | 17 passed |
| `tests/codec_golden.rs` | frames written by the real ActiveMQ client, OpenWire versions 9–12 | 4 passed |
| `tests/compression.rs`, `tests/hot_path.rs` | compressed golden vectors of all 5 message types, allocations per message, batching | 9 passed |
| **Rust total** | | **250 passed, 0 failed** |
| Java unit tests (`mvnw test`) | payload generation and result parsing of the bench client | 16 passed |
| Java `accept`, ActiveMQ client 5.19.11 and 6.3.2 | FIFO round trip, correlation-ID selectors, authentication | 3 + 3 passed |
| Java `integration`, 5.19.11 and 6.3.2 | 10,000-message FIFO, all message types, request/reply on temporary queues and topics, client ack, browser, topics, redelivery after a dropped connection, transactions, DLQ, client and broker compression, expiration, selectors, selector parity | 15 + 15 passed |
| Java `console`, 5.19.11 and 6.3.2 | form login, `ProviderVersion` equal to the console version, compressed message page, browsing does not consume | 4 + 4 passed |
| Java `compression`, 5.19.11 and 6.3.2 | byte-identical bodies for the 5 message types at the threshold boundaries | 40 + 40 passed |
| Java `expiry-options`, 5.19.11 and 6.3.2 | TTL ceiling, default TTL, broker clock | 5 + 5 passed |
| Java `accept` against real **ActiveMQ 5.18.7 and 6.3.2** (tuned configuration) | the same three scenarios give the same results on ActiveMQ | 3 + 3 passed |

While the soak scenario was being written, the multi-producer runs found a bug: with several producers on one queue a message could stay undelivered. It was fixed (`Fix lost queue messages with concurrent producers`) and is covered by a test.

### Load tests

**Conditions.** Windows 11 Pro for Workstations (build 26200), Intel Xeon W-2123 @ 3.60 GHz (4 cores, 8 logical processors), 31.7 GB RAM, OpenJDK 21.0.12. Brokers ran one at a time on the same machine, each restarted for every run:

- **ActiveMQRust** release build with its default configuration (no broker compression, which is the default from 0.2.0), unless a row says otherwise.
- **ActiveMQ 5.18.7 and 6.3.2** with [`scripts/activemq-bench/activemq-tuned.xml`](scripts/activemq-bench/activemq-tuned.xml): no persistence, VM queue cursor (no spooling to disk), no producer flow control, 3 GB memory limit, JVM `-Xmx4g`. No run logged a memory-limit, flow-control or spooling line, so ActiveMQ kept everything in RAM.
- **Client**: the same Java program and the same driver for every broker, `activemq-client` 5.18.7 (the ActiveMQ 6.3.2 runs were repeated with this driver so that only the server changes). XML `TextMessage`s, NON_PERSISTENT, AUTO_ACKNOWLEDGE, async send, prefetch 1000, one shared queue.
- **Checks on every message**: per-producer sequence increasing at every consumer, no message lost, none delivered twice, body length.
- **Measures**: latency from send to receive of every message (p50 / p99 / max), broker CPU time over the run (as % of one core and as CPU milliseconds per 1,000 messages), broker Working Set and Private Bytes sampled every 0.5 s (average / peak), client CPU. Start-up is the time from process start to the OpenWire port listening.
- Each row is a single run of `scripts\soak-compare.ps1` (scenario `soak` of the Java client); repeated runs vary by a few percent, except where noted.

#### Steady load: 10 producers × 10 msg/s for 240 s, 10 consumers, 1 KB (24,000 messages)

| | ActiveMQRust | ActiveMQ 5.18.7 | ActiveMQ 6.3.2 |
|---|---|---|---|
| Delivered | 24,000 / 24,000 | 24,000 / 24,000 | 24,000 / 24,000 |
| Start-up | **72 ms** | 1,838 ms | 1,998 ms |
| Latency p50 / p99 / max | **0.69 / 1.03 / 8.8 ms** | 0.74 / 1.25 / 12.0 ms | 0.73 / 1.12 / 23.2 ms |
| Broker CPU, % of one core | **3.0 %** | 5.6 % | 5.3 % |
| Broker CPU per 1,000 messages | **301 ms** | 561 ms | 533 ms |
| Working Set average / peak | **10.6 / 10.9 MB** | 182 / 235 MB | 193 / 250 MB |
| Private Bytes average / peak | **14.8 / 15.6 MB** | 642 / 667 MB | 647 / 671 MB |

#### Steady load: 20 producers × 10 msg/s for 60 s, 20 consumers, 10 KB (12,000 messages)

| | ActiveMQRust | ActiveMQ 5.18.7 | ActiveMQ 6.3.2 |
|---|---|---|---|
| Delivered | 12,000 / 12,000 | 12,000 / 12,000 | 12,000 / 12,000 |
| Start-up | **62 ms** | 1,825 ms | 2,017 ms |
| Latency p50 / p99 / max | 0.75 / **1.14** / **5.5 ms** | 0.81 / 1.26 / 13.3 ms | **0.66** / 1.55 / 19.4 ms |
| Broker CPU, % of one core | **6.2 %** | 16.2 % | 14.8 % |
| Broker CPU per 1,000 messages | **314 ms** | 832 ms | 760 ms |
| Working Set average / peak | **12.7 / 13.7 MB** | 222 / 287 MB | 227 / 303 MB |
| Private Bytes average / peak | **17.7 / 18.6 MB** | 651 / 665 MB | 652 / 668 MB |

#### Full speed: 20 producers and 20 consumers as fast as they can for 60 s

Producers send without pauses, so a backlog builds up in the broker whenever they are faster than the consumers; latency then measures that backlog, not the broker's own processing time. At these rates the machine is saturated, mostly by the Java client (5–7 cores), so throughput is partly limited by the client.

**15 KB messages**

| | ActiveMQRust | ActiveMQ 5.18.7 | ActiveMQ 6.3.2 |
|---|---|---|---|
| Result | **ok** | failed: out of memory ¹ | failed: out of memory ¹ |
| Delivered | **1,663,252 / 1,663,252** | 92,163 / 359,407 | 128,017 / 395,347 |
| Received | **26,644 msg/s, 390 MB/s** | 4,913 msg/s before failing | 6,622 msg/s before failing |
| Broker CPU per 1,000 messages | **36 ms** | 6,502 ms | 5,028 ms |
| Working Set average / peak | 1,501 / 2,434 MB | 4,037 / 4,285 MB | 3,925 / 4,289 MB |
| Latency p50 / p99 | 3.4 / 5.2 s (backlog) | 5.5 / 11.7 s | 5.0 / 10.6 s |

**50 KB messages**

| | ActiveMQRust | ActiveMQRust, broker compression on ² | ActiveMQ 5.18.7 | ActiveMQ 6.3.2 |
|---|---|---|---|---|
| Result | **ok** | ok | ok | failed: out of memory ¹ ³ |
| Delivered | **652,952** | 192,252 | 497,083 | 27,008 / 108,402 |
| Received | **10,838 msg/s, 529 MB/s** | 3,183 msg/s, 155 MB/s | 8,282 msg/s, 404 MB/s | 2,475 msg/s before failing |
| Broker CPU, % of one core | **96 %** | 433 % | 202 % | 541 % |
| Broker CPU per 1,000 messages | **91 ms** | 1,400 ms | 251 ms | 20,342 ms |
| Working Set average / peak | 153 / 225 MB | **89 / 123 MB** | 1,080 / 2,286 MB | 4,045 / 4,270 MB |
| Latency p50 / p99 | 0.14 / **0.81 s** | 0.27 / 1.77 s | **0.06** / 2.52 s | 3.8 / 6.7 s |

**300 KB messages**

| | ActiveMQRust | ActiveMQRust, broker compression above 256 KB ² | ActiveMQ 5.18.7 | ActiveMQ 6.3.2 |
|---|---|---|---|---|
| Delivered (all ok) | 112,595 | 30,622 | 111,768 | 111,569 |
| Received | **1,868 msg/s, 547 MB/s** | 505 msg/s, 148 MB/s | 1,861 msg/s, 545 MB/s | 1,858 msg/s, 544 MB/s |
| Broker CPU, % of one core | **114 %** | 478 % | 170 % | 177 % |
| Broker CPU per 1,000 messages | **635 ms** | 9,779 ms | 949 ms | 990 ms |
| Working Set average / peak | **202 / 286 MB** | 542 / 835 MB | 1,237 / 1,559 MB | 1,097 / 1,473 MB |
| Latency p50 / p99 | 0.19 / **0.85 s** | 1.81 / 7.88 s | **0.11** / 1.38 s | **0.11** / 1.05 s |

**Same full-speed runs with an 8 GB heap for ActiveMQ** (`-Xmx8g`, memory limit 7 GB), repeated for the runs that ran out of memory with 4 GB:

| | ActiveMQ 5.18.7, 15 KB | ActiveMQ 6.3.2, 15 KB | ActiveMQ 6.3.2, 50 KB |
|---|---|---|---|
| Result | failed: out of memory | failed: out of memory | ok |
| Delivered | 312,090 / 847,865 | 383,198 / 918,767 | 550,045 / 550,045 |
| Received | 6,715 msg/s before failing | 6,955 msg/s before failing | 8,297 msg/s, 405 MB/s |
| Broker CPU per 1,000 messages | 2,109 ms | 1,831 ms | 263 ms |
| Working Set average / peak | 7,066 / 8,467 MB | 6,352 / 8,468 MB | 4,112 / 7,682 MB |
| Latency p50 / p99 | 4.0 / 22.0 s | 2.2 / 20.8 s | 2.8 / 9.5 s |

With twice the heap ActiveMQ accepts more messages before failing, but at 15 KB its backlog still outgrows 8 GB; at 50 KB it completes using about 34 times the memory of ActiveMQRust (7.7 GB against 225 MB peak), at 405 MB/s against 529 MB/s.

**50 KB messages compressed by the client** (`useCompression=true`; no broker compression)

| | ActiveMQRust | ActiveMQ 5.18.7 | ActiveMQ 6.3.2 |
|---|---|---|---|
| Delivered (all ok) | 113,008 | 110,859 | 110,444 |
| Received | **1,883 msg/s** | 1,847 msg/s | 1,840 msg/s |
| Broker CPU per 1,000 messages | **171 ms** | 309 ms | 318 ms |
| Working Set average / peak | **27 / 30 MB** | 394 / 418 MB | 281 / 298 MB |
| Latency p50 / p99 | **4.5 / 79 ms** | 5.3 / 368 ms | 4.9 / 756 ms |

Here the Java client compressing every message is the bottleneck (about 7 cores), so all brokers reach the same rate; the broker's own cost and memory still differ.

¹ The 4 GB JVM heap filled up with queued messages; the broker spent its CPU in garbage collection and closed the connections, so most sent messages were never delivered. ActiveMQRust has no fixed heap: it used what the backlog needed and delivered everything.

² Broker compression is optional (`compress_threshold_kb`). It keeps memory low at a steady rate but, as these runs show, at full speed it costs 4–5 cores of zlib work and cuts throughput by about 3.5×; that is why it is off by default from version 0.2.0.

³ ActiveMQ at 50 KB full speed is at the edge of its 4 GB heap: an earlier run of ActiveMQ 6.3.2 with its own 6.3.2 driver completed (437 MB/s, 1.7 GB peak) while this run with the 5.18.7 driver ran out of memory. Single runs; the outcome depends on how fast the backlog grows.

**Reading the results.** With the same client and the same load, ActiveMQRust started 25–30 times faster, used 6–17 times less memory and 1.5–2.8 times less CPU per message, delivered every message in every run, and matched or exceeded ActiveMQ's throughput; ActiveMQ ran out of memory in three of the full-speed runs with its 4 GB heap, and in two of them even with 8 GB. Still to run: the full comparison (`scripts\compare-activemq.ps1`, measurements (a)–(f) with 3 runs each) and the criterion micro-benchmarks (`cargo bench`).

## Comparison with ActiveMQ

`scripts\compare-activemq.ps1` runs the same Java client against ActiveMQRust and Apache ActiveMQ 5.18.x and 6.x on the same machine, with ActiveMQ configured without persistence so that both process messages in RAM. It measures:

- memory at idle, and the start-up time of each broker;
- memory while holding **100,000 messages of 10 KB** each, and while holding **10,000 messages of 50 KB** each;
- time to **produce** and time to **consume** those messages;
- throughput with 1 KB messages and with 3,600 messages of 12 KB, with async and sync send;
- for every phase (start-up, idle, produce, hold, consume), the CPU and memory used by the broker, by the benchmark client and by the rest of the machine, and the broker's CPU time per 1,000 messages and per MB.

Every test message is an XML `TextMessage` with 20 fields holding random values, plus a base64 buffer that pads the document to the exact target size. Payloads are generated from a fixed seed, so both brokers receive identical messages.

### Running the comparison

You need PowerShell 7, JDK 17 or later, the release build (`cargo build --release`) and the ActiveMQ distributions unpacked somewhere (they are not part of this repository). Use a quiet machine: the script stops if the CPU load is above 20% or less than 8 GB of RAM is free, and repeats any run disturbed by other processes.

```
pwsh scripts\compare-activemq.ps1 -ActiveMQ5 C:\tools\apache-activemq-5.18.7 -ActiveMQ6 C:\tools\apache-activemq-6.3.2
```

The full comparison takes a few hours. Run it first with `-DryRun`: it only checks the paths, the ports, free RAM and CPU load and prints what would run, without starting anything. Other options: `-Quick` (one short run of everything, to check the setup; not a valid comparison), `-OutDir <dir>` (default `docs\benchmarks`), `-Port` / `-AdminPort` (default 61616 / 8161, used for every broker), `-SkipActiveMQ5`, `-SkipActiveMQ6`, `-SkipDefault` (skip ActiveMQ with its default configuration), `-Runs <n>` and `-Force` (only warn about CPU load and free memory). The fair ActiveMQ configuration and the ActiveMQRust configuration files are in [`scripts/activemq-bench/`](scripts/activemq-bench/).

The script writes a Markdown report (machine details, every run, medians, ratios and a met / not met verdict for every criterion) and a CSV with one row per run into `-OutDir` (default `docs\benchmarks`); the results worth keeping are copied into [Tests and results](#tests-and-results).

### Soak test

`scripts\soak-compare.ps1` runs the `soak` scenario of the Java client against ActiveMQRust, ActiveMQ 5.x and 6.x, one after the other: producers send at a steady rate while consumers read, every message is checked for order per producer, losses and duplicates, and the script records start-up time, latency, and the CPU and memory of the broker and the client.

```
pwsh scripts\soak-compare.ps1 -ActiveMQ5 C:\tools\apache-activemq-5.18.7 -ActiveMQ6 C:\tools\apache-activemq-6.3.2 -Duration 240
pwsh scripts\soak-compare.ps1 -ActiveMQ5 ... -ActiveMQ6 ... -Duration 60 -Producers 20 -Consumers 20 -Size 10240 -Results docs\benchmarks\soak-20x20.csv
```

Defaults: 10 producers at 10 messages per second each, 10 consumers on one shared queue (`-Queues` for more), 1 KB messages, 300 s, results in `docs\benchmarks\soak-results.csv`. Other options: `-Rate 0` (producers send as fast as they can for the duration; the result adds throughput), `-ClientCompression` (`useCompression=true` in the client), `-MqrustConfig <file>` (configuration for ActiveMQRust, e.g. with `compress_threshold_kb`), `-AmqHeap 8g` and `-AmqMemoryLimitGB 7` (ActiveMQ heap and memory limit; defaults 4g and 3), `-ClientProfile amq5|amq6` (the client build used for every broker; default `amq5`) and `-Only <name>` (run one broker). After each ActiveMQ run the script checks its log for memory-limit, flow-control or spooling lines, which would mean it did not keep everything in RAM. The same scenario can be run directly: `java -jar tests\java-it\target\amq5\mqrust-acceptance.jar bench --scenario soak --producers 10 --consumers 10 --rate 10 --duration-seconds 300`.

## Contributing

> [!IMPORTANT]
> **Please don't contribute code to this project: contribute requests.**
> ActiveMQRust is developed by an AI coding agent working from specifications, and pull requests with code are not merged. Instead, [open an issue](https://github.com/matteobaccan/ActiveMQRust/issues/new/choose) and explain what you need: the problem, the behaviour you expect, the ActiveMQ feature or client usage you rely on, or how to reproduce a bug. Your request becomes a specification, and the agent writes the code and the tests.

## License

Released under the [MIT](LICENSE) license.
