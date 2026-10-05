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

`scripts\compare-activemq.ps1` runs the same Java client against ActiveMQRust and Apache ActiveMQ 5.18.x and 6.x on the same machine, with ActiveMQ configured without persistence so that both process messages in RAM. It measures:

- memory at idle, and the start-up time of each broker;
- memory while holding **100,000 messages of 10 KB** each, and while holding **10,000 messages of 50 KB** each;
- time to **produce** and time to **consume** those messages;
- throughput with 1 KB messages and with 3,600 messages of 12 KB, with async and sync send.

Every test message is an XML `TextMessage` with 20 fields holding random values, plus a base64 buffer that pads the document to the exact target size. Payloads are generated from a fixed seed, so both brokers receive identical messages.

### Running the comparison

You need PowerShell 7, JDK 17 or later, the release build (`cargo build --release`) and the ActiveMQ distributions unpacked somewhere (they are not part of this repository). Use a quiet machine: the script stops if the CPU load is above 20% or less than 8 GB of RAM is free, and repeats any run disturbed by other processes.

```
pwsh scripts\compare-activemq.ps1 -ActiveMQ5 C:\tools\apache-activemq-5.18.7 -ActiveMQ6 C:\tools\apache-activemq-6.3.2
```

The full comparison takes a few hours. Useful options: `-Quick` (one short run of everything, to check the setup; not a valid comparison), `-OutDir <dir>` (default `docs\benchmarks`), `-Port` / `-AdminPort` (default 61616 / 8161, used for every broker), `-SkipActiveMQ5`, `-SkipActiveMQ6`, `-SkipDefault` (skip ActiveMQ with its default configuration), `-Runs <n>` and `-Force` (only warn about CPU load and free memory). The fair ActiveMQ configuration and the ActiveMQRust configuration files are in [`scripts/activemq-bench/`](scripts/activemq-bench/).

The script writes `activemq-comparison-<date>.md` (machine details, every run, medians, ratios and a met / not met verdict for every criterion) and `activemq-comparison-<date>.csv` (one row per run). Reports are published in [`docs/benchmarks/`](docs/benchmarks/).

## License

Released under the [MIT](LICENSE) license.
