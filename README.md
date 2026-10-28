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
- **Web admin console** with a login form and sessions: queues (every column sortable, filterable by name and state), consumers, producers, connections and queue contents, a formatted view of XML bodies, a responsive layout with light and dark themes that follow the system, plus a JSON API. An administrator can close a connection and ban an address or range; three failed logins from one address ban it automatically.
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

Passwords are typed twice with hidden input and stored only as Argon2id hashes. A password with fewer than 8 characters, equal to the username, or `admin`/`password` is insecure: the prompt warns and asks `Use it anyway? [y/N]` (default No); answer `y` to keep it. Empty passwords are always refused, and `--password-stdin` refuses insecure ones. Usernames are 1–64 letters, digits, `.`, `_`, `-` or `@`.

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
mqrust.exe --log-level <level>                 log level for this run (overrides log.level)
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

### TLS

OpenWire travels in clear text on port 61616. Turning on `[tls]` adds a second listener, by convention on port 61617, that Java clients reach with `ssl://host:61617` exactly as with ActiveMQ; the plain listener stays unless `broker.plain = false` turns it off.

```toml
[broker]
plain = true                 # false leaves only the TLS listener

[tls]
enabled = true
port = 61617                 # bind defaults to broker.bind
keystore = "broker.p12"      # PKCS#12, or cert + key in PEM
keystore_password = "..."    # the password of that keystore
min_version = "1.2"          # 1.2 or 1.3
client_auth = "none"         # none, optional or required
# client_ca = "clients-ca.crt"
```

Paths are relative to the folder of `mqrust.toml`. Keep the keystore and its password out of version control: the configuration file holds the password in clear text, so restrict access to its folder. Give **either** `cert` (PEM chain, leaf first) and `key` (PEM private key, unencrypted PKCS#8, PKCS#1 or SEC1), **or** `keystore` (PKCS#12 with exactly one key entry) and `keystore_password`.

```
# A keystore for the broker, and a truststore for the clients
keytool -genkeypair -alias broker -keyalg RSA -keysize 2048 -validity 365 ^
        -dname "CN=mybroker" -ext "SAN=dns:mybroker,ip:10.0.0.5" ^
        -keystore broker.p12 -storetype PKCS12 -storepass <password> -keypass <password>
keytool -exportcert -alias broker -keystore broker.p12 -storepass <password> -rfc -file broker.crt
keytool -importcert -noprompt -alias broker -file broker.crt ^
        -keystore truststore.p12 -storetype PKCS12 -storepass <password>

# An existing JKS keystore is converted once (JKS itself is not read)
keytool -importkeystore -srckeystore broker.jks -destkeystore broker.p12 -deststoretype PKCS12
```

The client points at that truststore and connects with `ssl://`:

```
java -Djavax.net.ssl.trustStore=truststore.p12 ^
     -Djavax.net.ssl.trustStoreType=PKCS12 ^
     -Djavax.net.ssl.trustStorePassword=<password> -jar myapp.jar
```

with `ssl://mybroker:61617` or `failover:(ssl://mybroker:61617)`.

- **Client certificates**: `client_auth = "optional"` (ActiveMQ's `wantClientAuth`) or `"required"` (`needClientAuth`) with `client_ca`, a PEM bundle of the accepted CAs. A certificate never replaces the username and password, which are still checked; the subject of a verified certificate is shown on the Connections page.
- **Checks at startup**: every file is read and parsed by the broker and by `check-config`, and a certificate that does not match its key, a JKS keystore or an unreadable file is a configuration error with exit code 2. A certificate that has expired or expires within 30 days is only a warning.
- **Moving to TLS only**: keep both listeners while the clients change their URL, then set `broker.plain = false`. Leaving both off is a configuration error.
- A failed or timed-out handshake (10 seconds) is logged at debug level, with at most one warning per minute giving the count, so a port scan cannot flood the log.

Deliberate differences from ActiveMQ: JKS keystores are not read (PKCS#12 and PEM are), only TLS 1.2 and 1.3 are offered with rustls's safe cipher suites and no key to enable others, certificates are reloaded by a restart, and the admin console stays on HTTP (put a TLS reverse proxy in front of it).

### Logging

One line per event, with a local timestamp: `2026-10-21 09:15:04 INFO OpenWire listening on 0.0.0.0:61616`.

- **Folder**: `log.dir`, by default `logs` next to `mqrust.exe`; an absolute path is used as it is, and the folder is created if missing. Give each broker instance its own folder: two brokers writing the same folder are not detected.
- **Active file**: always `logs\mqrust.log`. Every file opens with a header line naming the version, the process ID and the configuration in use, whatever the level is.
- **Daily rotation**: at local midnight `mqrust.log` is renamed after the day it holds, `mqrust-2026-10-21.log`, and a new `mqrust.log` is started. This happens even when nothing is logged after midnight, and a file left from an earlier day is rotated at startup. An archive is never overwritten: a name already taken gets a counter, `mqrust-2026-10-21-1.log`.
- **Compression**: each archive is compressed to `mqrust-2026-10-21.log.gz` by a background thread (`log.compress = false` keeps plain `.log` files). The uncompressed file is deleted only after the `.gz` is complete, and a compression interrupted by a restart is finished at the next startup.
- **Retention**: `log.retention_days` (default 30) deletes archives older than that many days, by the date in their name; other files in the folder are never touched. `0` keeps every archive.
- **Outputs**: in console mode lines go to stdout and to the file (`log.file = false` for stdout only). As a Windows service the file is the only output, so `log.file` is ignored; a configuration that fails to load is reported in `logs\mqrust.log` next to the executable.
- **Level**: `log.level` (`error`, `warn`, `info`, `debug`, `trace`), or `--log-level <level>` for one run.
- A log file that cannot be written (full disk, missing permission, folder held by another program) never stops the broker: it keeps serving clients and logging to stdout, prints one warning on stderr and tries the file again later. `check-config` reports a `log.dir` it cannot create or write.

Unlike ActiveMQ, which rolls `activemq.log` by size and keeps a fixed number of backups, ActiveMQRust rotates by local day with no size limit and keeps archives by age. A single day is never split.

## Admin console

The read-only web console answers on `http://127.0.0.1:8161/` (keys `[admin] bind` / `port`, or `--admin-bind` / `--admin-port`). If the port cannot be bound, the broker logs an error and keeps running without the console.

- **Login**: pages open a login form; a successful login starts a session held in memory, in an `HttpOnly`, `SameSite=Strict` cookie. The top bar shows the user and a **Log out** button. Sessions end after `session_idle_minutes` without requests, `session_max_hours` after login, or when the broker restarts. The login page warns when the built-in `admin` / `admin` credentials are in use.
- **Throttling**: after `login_max_failures` failed logins from one IP within 15 minutes, that IP is refused (`429`) for `login_lockout_seconds`. Failed logins are logged with IP and username, never the password.
- **Pages**: overview (version, uptime, connections, memory, broker compression counters), queues, topics, connections, queue detail with consumers, producers and paginated contents (50 per page), message detail (headers, properties, body by JMS type). Add `?refresh=5` to refresh every 5 seconds.
- **Sortable tables**: all list tables are sortable by clicking column headers (server-side, no JavaScript, typed comparison: numeric, chronological, IP/port, case-insensitive natural order for text and IDs; ties broken by the first column ascending). Sorting parameters are kept across auto-refresh, pagination, and between tables on the same page:
  - Queues (`sort`/`order`): Name, Pending, Inflight, Consumers, Producers, Enqueued, Consumed, Expired (default: Name ascending).
  - Topics (`sort`/`order`): Name, Consumers, Producers, Published, Discarded (default: Name ascending).
  - Connections (`sort`/`order`): Connection ID, User, Client, OpenWire, Connected, Sessions, Consumers, Producers (default: Connected ascending).
  - Queue detail: Consumers table (`csort`/`corder`: Consumer ID, Connection ID, Client, Prefetch, Inflight, Selector; default: Consumer ID ascending), Producers table (`psort`/`porder`: Producer ID, Connection ID, Client; default: Producer ID ascending).
  - Message detail: Properties table (`prsort`/`prorder`: Name, Type, Value; default: Name ascending), MapMessage body table (`msort`/`morder`: Key, Type, Value; default: Key ascending).
  - Queue contents (FIFO), message headers, and StreamMessage body maintain their fixed order with plain headers.
- **Queue filter**: a filter bar above the queues table narrows the list by name (`q`, case-insensitive contains, up to 200 characters) and state checkboxes (`pending=1` for queues with pending messages, `noconsumers=1` for queues without consumers), combined with AND. Works server-side with no JavaScript: sort links and auto-refresh keep the active filter, and the filter form keeps the current sort and refresh. While a filter is active, the page shows "Showing N of M queues" and a "Clear filter" link; if no queues match, the table displays "No queues match the filter".
- **XML bodies**: a TextMessage holding well-formed XML gets a **Formatted** view (indented and coloured) next to the unchanged **Raw** view. The page carries both views and the switch is a pair of radio buttons styled as a segmented control, so changing view sends no request, reloads nothing and keeps the scroll position, still without JavaScript; `?view=xml` opens the page on the formatted view. With auto-refresh on the switch stays a pair of links, so the chosen view survives each reload. DTDs and entities are never resolved; bodies above 1 MB are not formatted.
- **Theme**: follows the system light/dark setting. No JavaScript and no external resources are loaded.
- **Memory charts**: under the cards, the overview draws the last hour of message memory (minimum-maximum band, last value and, when `max_memory_mb` is set, a dashed limit line) and of process memory, as inline SVG with no JavaScript. Each chart gives the peak of the hour with its time, a tooltip per 10-second bucket and a collapsible **Data** table. The broker reads message memory every second and process memory once per bucket, outside the path that stores and delivers messages, so a peak shorter than a second can be missed; the history uses a few kilobytes, covers one hour and starts empty at every restart. The **Messages in memory** card counts pending plus inflight messages, a topic message once per subscription that still holds it.
- **Closing a connection**: every row of the Connections page has a **Close** action. The client is told "Connection closed by the administrator" through its `ExceptionListener` and the socket is closed; the broker then cleans up as for a client that died (unacknowledged messages return to their queues as redelivered, open transactions are rolled back, temporary destinations are removed). A client with a `failover:` URL reconnects within a second, which is what **Ban IP** is for.
- **Bans**: the **Bans** page lists the addresses kept out of the OpenWire listeners and adds one from a connection row or from its own form: an IPv4 or IPv6 address, or a CIDR range (`10.1.0.0/16`), for 1 hour, 24 hours, 7 days or permanently, with an optional reason. Banning offers to close the connections already open from that address. A banned address is refused right after `accept`, before any byte is read, and the page counts the refused attempts. Bans never apply to the admin console: banning your own address only warns.
  - After **3 failed OpenWire logins from one address within 15 minutes** the broker bans it automatically for 1 minute, and for twice as long after each further failure (up to 24 hours); a successful login or a day without failures clears the escalation. Automatic bans live in memory only: a restart clears them. Only wrong credentials count, never protocol or TLS errors.
  - Manual bans are stored in `bans.toml` next to the configuration file (next to the executable with the built-in defaults) and are rewritten atomically at every change. Edit the file only while the broker is stopped: the next change made from the console rewrites it from what the broker holds. A broken file stops the start with exit code 2, naming the line and the entry; `mqrust.exe check-config` reports it too.
  - Every write operation needs a `POST` from the console's own origin and a confirmation page, and is logged with the administrator, their address and what was done. `admin.read_only = true` removes every write action from the pages and answers `405` to their paths.
  - A ban is not a firewall: the operating system still completes the TCP handshake, and clients behind the same NAT or proxy share one address, so the confirmation page shows how many connections the ban affects. Use the operating system firewall against floods.

```toml
[admin]
session_idle_minutes = 30    # 1-1440
session_max_hours = 8        # 1-168
login_max_failures = 5       # 0 disables the lockout
login_lockout_seconds = 60   # 1-86400
read_only = false            # true refuses every write operation
```

The console uses plain HTTP and binds to `127.0.0.1` by default. To reach it from other machines, put a TLS reverse proxy in front of it rather than binding it to a public address.

### JSON API

The same data is available as JSON for scripts and monitoring. The write endpoints (`POST`, `DELETE`) follow the same rules as the console pages: they need the `[admin]` credentials and answer `405` when `admin.read_only` is set. API paths accept HTTP Basic with the `[admin]` credentials (or a browser session cookie); without valid credentials they answer `401 {"error":"unauthorized"}`, with no browser pop-up.

```
curl -u admin:admin http://127.0.0.1:8161/api/queues?sort=pending&order=desc
```

| Path | Returns |
| --- | --- |
| `/api/overview` | `{product, version, uptimeSeconds, openwire, admin, connections, queues, topics, messageMemory, messagesInMemory, memoryLimit, memoryLimitReached, compressed, compressDiscarded, workingSet, privateBytes}` |
| `/api/memory/history` | `{intervalSeconds, limitBytes, buckets: [{start, messageMemory: {min, max, last}, workingSet, privateBytes}]}`, the last hour in 10-second buckets, oldest first, the one in progress last |
| `/api/queues?q=&pending=&noconsumers=&sort=&order=` | `[{name, temporary, pending, inflight, consumers, producers, enqueued, consumed, expired, discarded, memory, compressed}]` (`q`: name contains, `pending=1`: pending messages, `noconsumers=1`: no consumers; `sort`: name, pending, inflight, consumers, producers, enqueued, consumed, expired; `order`: asc, desc) |
| `/api/queues/{name}` | the queue fields plus `consumers: [{consumerId, connectionId, client, prefetch, inflight, selector, browser}]`, `producers: [{producerId, connectionId, client}]`, `withExpiration`, `nextExpiration`, `nextExpirationText` |
| `/api/queues/{name}/messages?offset=&limit=` | `{total, offset, limit, messages: [message]}` in FIFO order; `limit` defaults to 50 and is clamped to 1..50 |
| `/api/queues/{name}/messages/{id}?view=xml` | one message plus `inflight` and `body` (`{kind: text, text, truncated}`, `{kind: bytes, hex, size, truncated}`, `{kind: map, entries}`, `{kind: stream, values}`, `{kind: object, size}`, `{kind: none}`); with `view=xml`, `formattedBody` or `formatError` |
| `/api/topics?sort=&order=` | `[{name, temporary, consumers, producers, published, discarded, pending, memory}]` (`sort`: name, consumers, producers, published, discarded; `order`: asc, desc) |
| `/api/connections?sort=&order=` | `[{connectionId, clientId, user, client, openwireVersion, connectedAt, sessions, consumers, producers}]` (`sort`: connectionId, user, client, openwire, connected, sessions, consumers, producers; `order`: asc, desc) |
| `POST /api/connections/{id}/close` | `{closed}`; `404` when the connection is not open |
| `GET /api/bans?sort=&order=` | `[{address, reason, created, createdBy, expires, automatic, refusedAttempts}]` (`sort`: address, reason, by, created, expires, refused) |
| `POST /api/bans` | body `{address, duration: 1h\|24h\|7d\|permanent, reason, closeConnections}` (duration `24h` and `closeConnections` true by default); answers `{ban, closedConnections}`, or `400 {"error"}` for an invalid address or duration |
| `DELETE /api/bans/{address}` | `{removed}`; `404` when the address is not banned. Percent-encode the `/` of a range (`198.51.100.0%2F24`) |

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
| Rust unit tests (`cargo test --release`, library) | codec, configuration, selectors, compression, sessions, XML formatter, CPU sizing, setup commands, log rotation and retention, bans and automatic bans, memory history and charts, TLS certificate checks, nesting limits of frames and selectors | 186 passed |
| `tests/broker_semantics.rs` | FIFO, round-robin, redelivery position, prefetch, acknowledgement types, DLQ, expiration, topics, browsers, transactions, memory limit, concurrent producers | 60 passed |
| `tests/connection_semantics.rs` | the broker driven over a real socket by a small OpenWire client, administrative close and a frame nested without end included | 17 passed |
| `tests/selector_semantics.rs` | 568 selectors checked against the results of ActiveMQ's own selector engine | 9 passed |
| `tests/admin_http.rs` | login, sessions, lockout, logout, API with Basic, headers, escaping, paging, system theme, addresses, closing connections, bans, read-only console, memory history, transport of a connection | 49 passed |
| `tests/cli_setup.rs` | help, `set-admin`, `user` commands, safe file editing, start-up messages, log file and folder, end to end | 21 passed |
| `tests/codec_golden.rs` | frames written by the real ActiveMQ client, OpenWire versions 9–12 | 4 passed |
| `tests/compression.rs`, `tests/hot_path.rs` | compressed golden vectors of all 5 message types, allocations per message, batching | 9 passed |
| `tests/tls_transport.rs` | OpenWire over TLS end to end, TLS 1.3 only, client certificate required (missing and valid), password still checked, handshake timeout, TLS-only mode | 5 passed |
| `tests/fuzz_seeds.rs` | the seeds of the fuzz targets replayed, and a seeded sweep of random bytes, mutated client frames and generated selector text | 9 passed |
| **Rust total** | | **369 passed, 0 failed** |
| Java unit tests (`mvnw test`) | payload generation and result parsing of the bench client | 16 passed |
| Java `accept`, ActiveMQ client 5.19.11 and 6.3.2 | FIFO round trip, correlation-ID selectors, authentication | 3 + 3 passed |
| Java `integration`, 5.19.11 and 6.3.2 | 10,000-message FIFO, all message types, request/reply on temporary queues and topics, client ack, browser, topics, redelivery after a dropped connection, transactions, DLQ, client and broker compression, expiration, selectors, selector parity | 15 + 15 passed |
| Java `console`, 5.19.11 and 6.3.2 | form login, `ProviderVersion` equal to the console version, compressed message page, browsing does not consume | 4 + 4 passed |
| Java `compression`, 5.19.11 and 6.3.2 | byte-identical bodies for the 5 message types at the threshold boundaries | 40 + 40 passed |
| Java `expiry-options`, 5.19.11 and 6.3.2 | TTL ceiling, default TTL, broker clock | 5 + 5 passed |
| Java `tls`, 5.19.11 and 6.3.2 | ten-message round trip over `ssl://` and over `failover:(ssl://)` against a `keytool` PKCS#12 keystore, wrong password still refused | 3 + 3 passed |
| Java `integration` over `ssl://`, 5.19.11 | the whole integration suite again, over TLS instead of plain TCP | 15 passed |
| Java `console`, 5.19.11 and 6.3.2 | a connection closed by an administrator reports the reason to the client; a banned client cannot come back through its `failover:` URL | 2 + 2 passed |
| Java `accept` against real **ActiveMQ 5.18.7 and 6.3.2** (tuned configuration) | the same three scenarios give the same results on ActiveMQ | 3 + 3 passed |

On Windows, CI also checks the two promises no Rust test can reach: that `mqrust.exe`
imports only Windows system DLLs, so that it runs where nothing is installed, and that the
broker installs as a service, starts, serves both ports, writes its log, stops and uninstalls
(`scripts/check-deps.ps1` and `scripts/service-check.ps1`, which also run by hand).

The TLS transport was exercised on loopback with certificates made by `keytool`, which is how an operator sets it up: `tests/tls_transport.rs` runs on every platform of the matrix, Windows included, and the Java checks ran against a broker on the same machine. Three things it has not seen yet: a Java client over TLS on Windows, mutual TLS with a Java client (only the Rust test client has presented a certificate), and a load run over TLS.

The frame decoder, the message properties and the selector parser are also fuzzed, with
`cargo-fuzz` on a nightly toolchain (`fuzz/`, see AGENTS.md). The first campaigns ran 4.7
million inputs through the decoder, 4.3 million through the message properties and 3.5
million through the selector parser. They found a way to stop the broker: a selector made
of a few thousand prefix operators or open parentheses recursed once per level until the
stack overflowed, which aborts the process rather than the connection. Reading the decoder
had turned up the same shape a day earlier, in a frame whose data structures nested without
end. Both are fixed, bounded at 64 and 32 levels, and both are covered by a test. The
fuzzer itself runs on Linux and macOS; its seeds and what it finds are replayed by
`tests/fuzz_seeds.rs` on every platform of the matrix.

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
