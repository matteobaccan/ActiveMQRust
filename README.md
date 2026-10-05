<p align="center"><img src="docs/assets/logo-wordmark.svg" alt="ActiveMQRust" width="420"></p>

# ActiveMQRust

An **in-memory** message broker written in **Rust** that speaks Apache ActiveMQ's **OpenWire** protocol.
Java applications that use the ActiveMQ driver (`activemq-client`) connect without code changes: just point them at `tcp://host:61616`.

The goal is a broker that is **compatible** with ActiveMQ, **uses less RAM** and is **faster** than ActiveMQ, shipped as a single Windows executable with no dependencies.

> **Project status:** version **0.2.0**. The broker is implemented and passes the Rust and Java test suites listed in [Tests and results](#tests-and-results). The specifications are [OpenSpec](https://github.com/Fission-AI/OpenSpec) specs in [`openspec/specs/`](openspec/specs/) (completed changes are archived in [`openspec/changes/archive/`](openspec/changes/archive/)). A soak comparison with ActiveMQ 5.18.7 and 6.3.2 is published; the full benchmark comparison is still to be run.

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
- **Web admin console** with a login form and sessions: queues (every column sortable), consumers, producers, connections and queue contents, a formatted view of XML bodies, a responsive layout with light and dark themes that follow the system, plus a JSON API.
- **Guided setup from the command line**: `mqrust.exe -h` shows the four steps to get started; `set-admin` and `user add` / `passwd` / `remove` / `list` manage the console user and the messaging users without editing the file by hand.
- The broker identifies itself as **`ActiveMQRust <version>`**.

Not in this version: persistence, durable subscriptions, XA transactions, wildcards, network of brokers, protocols other than OpenWire.

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

All suites below were run on 2026-10-05/06 on version 0.2.0 (Windows 11, Intel Xeon W-2123, JDK 21), and all of them pass.

### Functional tests

| Suite | What it checks | Result |
|---|---|---|
| Rust unit tests (`cargo test --release`, library) | codec, configuration, selectors, compression, sessions, XML formatter, CPU sizing, setup commands | 118 passed |
| `tests/broker_semantics.rs` | FIFO, round-robin, redelivery position, prefetch, acknowledgement types, DLQ, expiration, topics, browsers, transactions, memory limit, concurrent producers | 60 passed |
| `tests/connection_semantics.rs` | the broker driven over a real socket by a small OpenWire client | 14 passed |
| `tests/selector_semantics.rs` | 568 selectors checked against the results of ActiveMQ's own selector engine | 9 passed |
| `tests/admin_http.rs` | login, sessions, lockout, logout, API with Basic, headers, escaping, paging | 18 passed |
| `tests/cli_setup.rs` | help, `set-admin`, `user` commands, safe file editing, start-up messages, end to end | 17 passed |
| `tests/codec_golden.rs` | frames written by the real ActiveMQ client, OpenWire versions 9–12 | 4 passed |
| `tests/compression.rs`, `tests/hot_path.rs` | compressed golden vectors of all 5 message types, allocations per message, batching | 9 passed |
| **Rust total** | | **249 passed, 0 failed** |
| Java unit tests (`mvnw test`) | payload generation and result parsing of the bench client | 16 passed |
| Java `accept`, ActiveMQ client 5.18.x and 6.x | FIFO round trip, correlation-ID selectors, authentication | 3 + 3 passed |
| Java `integration`, 5.18.x and 6.x | 10,000-message FIFO, all message types, request/reply on temporary queues and topics, client ack, browser, topics, redelivery after a dropped connection, transactions, DLQ, client and broker compression, expiration, selectors, selector parity | 15 + 15 passed |
| Java `console`, 5.18.x and 6.x | form login, `ProviderVersion` equal to the console version, compressed message page, browsing does not consume | 4 + 4 passed |
| Java `compression`, 5.18.x and 6.x | byte-identical bodies for the 5 message types at the threshold boundaries | 40 + 40 passed |
| Java `expiry-options`, 5.18.x and 6.x | TTL ceiling, default TTL, broker clock | 5 + 5 passed |
| Java `accept` against real **ActiveMQ 5.18.7 and 6.3.2** (tuned configuration) | the same three scenarios give the same results on ActiveMQ | 3 + 3 passed |

While the soak scenario was being written, the multi-producer runs found a bug: with several producers on one queue a message could stay undelivered. It was fixed (`Fix lost queue messages with concurrent producers`) and is covered by a test.

### Load tests

Soak test, 2026-10-05, one shared queue, every message checked for order per producer, losses and duplicates ([full report](docs/benchmarks/soak-2026-10-05.md)):

| | ActiveMQRust | ActiveMQ 5.18.7 | ActiveMQ 6.3.2 |
|---|---|---|---|
| **10 producers × 10 msg/s × 240 s, 10 consumers, 1 KB** | ok, 24,000 / 24,000 | ok, 24,000 / 24,000 | ok, 24,000 / 24,000 |
| Missing / duplicates / out of order | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| Startup | **72 ms** | 1,838 ms | 1,877 ms |
| Latency p50 / p99 | **0.69 / 1.03 ms** | 0.74 / 1.25 ms | 0.75 / 1.28 ms |
| Broker CPU (one core) / per 1,000 messages | **3.0 % / 301 ms** | 5.6 % / 561 ms | 5.7 % / 571 ms |
| Broker Working Set / Private Bytes (average) | **10.6 / 14.8 MB** | 182 / 642 MB | 192 / 645 MB |
| **20 producers × 10 msg/s × 60 s, 20 consumers, 10 KB** | ok, 12,000 / 12,000 | ok, 12,000 / 12,000 | ok, 12,000 / 12,000 |
| Missing / duplicates / out of order | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| Startup | **62 ms** | 1,825 ms | 1,825 ms |
| Latency p50 / p99 | **0.75 / 1.14 ms** | 0.81 / 1.26 ms | 0.83 / 1.26 ms |
| Broker CPU (one core) / per 1,000 messages | **6.2 % / 314 ms** | 16.2 % / 832 ms | 14.7 % / 757 ms |
| Broker Working Set / Private Bytes (average) | **12.7 / 17.7 MB** | 222 / 651 MB | 228 / 649 MB |

At a steady load all three brokers delivered every message exactly once and in order; ActiveMQRust used about 17 times less Working Set, about 40 times less Private Bytes and roughly half to a third of the CPU per message, and started about 25 times faster. These are single runs at a moderate fixed rate: they show resource use and correctness, not maximum throughput.

Still to run: the full comparison (`scripts\compare-activemq.ps1`, measurements (a)–(f) with 3 runs each) and the criterion micro-benchmarks (`cargo bench`). [`docs/benchmarks/activemq-comparison-2026-10-05.md`](docs/benchmarks/activemq-comparison-2026-10-05.md) is an early partial run of version 0.1.0 (only measurement (e), one run, before the compression and concurrency fixes) and is kept for reference only.

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

The script writes `activemq-comparison-<date>.md` (machine details, every run, medians, ratios and a met / not met verdict for every criterion) and `activemq-comparison-<date>.csv` (one row per run). Reports are published in [`docs/benchmarks/`](docs/benchmarks/).

### Soak test

`scripts\soak-compare.ps1` runs the `soak` scenario of the Java client against ActiveMQRust, ActiveMQ 5.x and 6.x, one after the other: producers send at a steady rate while consumers read, every message is checked for order per producer, losses and duplicates, and the script records start-up time, latency, and the CPU and memory of the broker and the client.

```
pwsh scripts\soak-compare.ps1 -ActiveMQ5 C:\tools\apache-activemq-5.18.7 -ActiveMQ6 C:\tools\apache-activemq-6.3.2 -Duration 240
pwsh scripts\soak-compare.ps1 -ActiveMQ5 ... -ActiveMQ6 ... -Duration 60 -Producers 20 -Consumers 20 -Size 10240 -Results docs\benchmarks\soak-20x20.csv
```

Defaults: 10 producers at 10 messages per second each, 10 consumers on one shared queue (`-Queues` for more), 1 KB messages, 300 s, results in `docs\benchmarks\soak-results.csv`. The same scenario can be run directly: `java -jar tests\java-it\target\amq5\mqrust-acceptance.jar bench --scenario soak --producers 10 --consumers 10 --rate 10 --duration-seconds 300`.

## License

Released under the [MIT](LICENSE) license.
