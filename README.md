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
