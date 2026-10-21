# Changelog

All notable changes to ActiveMQRust are listed here. The section matching a
release tag is used as the body of the GitHub Release; GitHub appends the
list of merged pull requests and the compare link below it.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- **Server-side queue filter in admin console and API**: a filter bar above the queues
  table narrows the list by name (`q`, case-insensitive contains, up to 200 characters)
  and state checkboxes (`pending=1` for queues with pending messages, `noconsumers=1`
  for queues without consumers), combined with AND.
  - Plain GET form with visible labels and `role="search"`, operating without JavaScript
    and with CSP unchanged.
  - Composes with sorting (`sort`/`order`) and auto-refresh (`refresh=5`): sort links
    preserve active filter parameters, and the filter form preserves current sort and refresh.
  - Shows "Showing N of M queues" and a "Clear filter" link while a filter is active;
    displays "No queues match the filter" when no queues match.
  - JSON API endpoint `/api/queues` supports the same filter parameters with identical semantics.
- **Server-side table sorting in admin console and API**: every list table in the admin
  console is sortable by clicking column headers, with no JavaScript and CSP unchanged.
  Columns support typed comparisons: numeric, chronological for connection timestamps,
  IP address and port for client addresses, and case-insensitive natural order for names
  and identifiers (`Q2` before `Q10`, `ID:h-1:1:1:2` before `ID:h-1:1:1:10`).
  Ties are broken stably by the table's first column ascending.
  - Queues (`sort`/`order`), Topics (`sort`/`order`), Connections (`sort`/`order`).
  - Queue detail: Consumers (`csort`/`corder`) and Producers (`psort`/`porder`).
  - Message detail: Properties (`prsort`/`prorder`) and MapMessage entries (`msort`/`morder`).
  - Sorting parameters are preserved across auto-refresh (`?refresh=`), queue contents
    pagination, and XML Raw/Formatted view toggles.
  - JSON API endpoints `/api/topics?sort=&order=` and `/api/connections?sort=&order=`
    accept the same column keys as the console pages.
  - Queue message contents (FIFO), message headers, and StreamMessage bodies remain
    unsorted with plain headers.

- **OpenWire over TLS**: a new `[tls]` section adds a second listener, by convention on port
  61617, that ActiveMQ Java clients reach with `ssl://host:61617` and
  `failover:(ssl://host:61617)` without any change beyond their truststore.
  - The certificate comes from PEM files (`cert` and `key`) or from a PKCS#12 keystore
    (`keystore`, `keystore_password`), with paths relative to the configuration file. A JKS
    keystore is refused with the `keytool -importkeystore` command that converts it.
  - TLS 1.2 and 1.3 only (`min_version`), with the safe cipher suites of rustls and no key to
    enable others. `client_auth` (`none`, `optional`, `required`) with `client_ca` asks for a
    client certificate, which never replaces the username and password.
  - The handshake runs outside the accept loop and is abandoned after 10 seconds; failures are
    logged at debug level with at most one warning a minute, so a port scan cannot flood the
    log.
  - Every TLS file is checked at startup and by `check-config`: a mismatched key, a JKS
    keystore or an unreadable file exits with code 2 naming the key, while a certificate
    expiring within 30 days is only a warning.
  - New key `broker.plain` (default `true`): `false` leaves only the TLS listener. Both off is
    a configuration error.
  - The Connections page and `/api/connections` now carry `transport` (`tcp` or `ssl`),
    `tlsVersion`, `tlsCipher` and `clientCertificate`.
- **Instant Raw/Formatted switch** on the message page: the page now carries both views of an
  XML body and the switch is two radio buttons styled as the segmented control, so changing
  view sends no request, reloads nothing and keeps the scroll position. Still no JavaScript
  and the same Content-Security-Policy; keyboard arrows move between the options and only the
  visible view is exposed to assistive technologies. `?view=xml` still opens the formatted
  view, and with auto-refresh on the switch stays a pair of links so the view survives the
  reload.
- **Memory history on the overview**: two charts of the last hour, drawn on the server as
  inline SVG with no JavaScript - message memory (minimum-maximum band, last value and a
  dashed line at `max_memory_mb` when it is set) and process memory (Working Set and Private
  Bytes; resident size and physical footprint on macOS).
  - Each chart gives the peak of the hour with its time, 15-minute ticks, a legend, a tooltip
    per bucket, an `aria-label` summary and a collapsible **Data** table; the page stays
    readable at 360 px and the Content-Security-Policy is unchanged.
  - The broker records 360 buckets of 10 seconds while the console is enabled: message memory
    is read every second (minimum, maximum and last per bucket), process memory once per
    bucket, by a task outside the path that stores and delivers messages. The history uses
    fixed memory and starts empty at every start.
  - New card **Messages in memory** (pending plus inflight, a topic message once per
    subscription that holds it), also in `/api/overview` as `messagesInMemory`, and a new
    endpoint `GET /api/memory/history`.
- **Closing connections and banning addresses from the admin console**: every row of the
  Connections page offers **Close** (the client is told "Connection closed by the administrator"
  and the broker cleans up as for a dropped connection) and **Ban IP**, and a new **Bans** page
  lists the addresses kept out of the OpenWire listeners.
  - A ban is an address or a CIDR range, for 1 hour, 24 hours, 7 days or permanently, with an
    optional reason; adding one offers to close the connections already open from it. Banned
    addresses are refused right after `accept`, before any byte is read, and the refused
    attempts are counted per ban.
  - **Automatic bans**: 3 failed OpenWire logins from one address within 15 minutes ban it for
    1 minute, doubling after each further failure up to 24 hours. A successful login or a day
    without failures resets the escalation. Kept in memory only: a restart clears them.
  - Manual bans are stored in `bans.toml` next to the configuration file, rewritten atomically;
    a broken file stops the start with exit code 2 naming the line and the entry, and
    `check-config` reports it.
  - API: `POST /api/connections/{id}/close`, `GET`/`POST /api/bans` and
    `DELETE /api/bans/{address}`.
  - New key `admin.read_only` (default `false`): `true` removes every write action from the
    pages and answers `405` to their paths. Every write needs a `POST` from the console's own
    origin and is logged with the administrator and their address.
- **Daily log files**: logging writes to a folder (`log.dir`, default `logs` next to the
  executable) into an active `mqrust.log` that is rotated at local midnight, named after the
  day it holds (`mqrust-2026-10-21.log`), compressed to `.log.gz` by a background thread and
  deleted once older than `log.retention_days` (default 30, `0` keeps everything).
  - Rotation happens without traffic too, and a file left from an earlier day is rotated at
    startup; an archive is never overwritten (`-1`, `-2` counter), and a compression
    interrupted by a restart is finished at the next start.
  - Every file opens with a header line (version, process ID, configuration in use) whatever
    the level is.
  - Console mode writes to stdout and to the file; `log.file = false` keeps stdout only.
  - New keys `log.dir`, `log.file`, `log.retention_days` and `log.compress`, and a
    `--log-level <level>` option that overrides `log.level` for one run.
  - A log file that cannot be written never stops the broker: one warning on stderr, stdout
    keeps working and the file is tried again later. `check-config` reports an unusable
    `log.dir`.
  - **BREAKING**: as a Windows service the log moves from `mqrust.log` next to the executable
    to `logs\mqrust.log`. The old file is left untouched and can be deleted by hand.

### Changed

- **Dependencies updated**, including the major bumps the open Renovate pull requests
  were waiting for: `toml` 0.8 → 1.0, `toml_edit` 0.22 → 0.25, `quick-xml` 0.41 → 0.42,
  `sha2` 0.10 → 0.11, `windows-service` 0.7 → 0.8, `windows-sys` 0.59 → 0.61,
  `criterion` 0.5 → 0.8 and `embed-resource` 2 → 3. No behaviour changes:
  - The XML view of message bodies takes the `&str` events quick-xml 0.42 produces
    instead of decoding bytes; references, CDATA and processing instructions are still
    shown as written.
  - The benchmarks use `std::hint::black_box`, which replaces criterion's own.
  - `build.rs` reports a Windows version resource that fails to compile and keeps
    tolerating a toolchain without a resource compiler.

## [0.3.1] - 2026-10-06

### Changed

- **Insecure passwords can be confirmed.** `set-admin`, `user add` and `user passwd` no
  longer refuse a password with fewer than 8 characters, equal to the username, or
  `admin`/`password`: the prompt explains why it is insecure and asks
  `Use it anyway? [y/N]` (default No); answering `y` keeps it. Empty passwords are still
  refused, and `--password-stdin` still refuses insecure passwords (exit code 2), since
  nobody can confirm them.

## [0.3.0] - 2026-10-06

The first release with downloadable binaries: Windows x86_64 and, new, macOS on Apple
Silicon, built and tested by GitHub Actions.

### Added

- **macOS ARM64 (Apple Silicon) build**, `activemq-rust-macos-arm64-<version>.tar.gz`, next
  to the Windows one, `activemq-rust-windows-x86_64-<version>.zip`. Each archive holds the
  broker, LICENSE, README.md and the commented configuration template
  `mqrust.example.toml`. On macOS the admin console shows the resident size and the
  physical footprint of the process; the `service` commands answer that the Windows
  service is available on Windows only (exit code 2).
- **Continuous integration and release pipeline.** Every push and pull request is checked
  on Windows and macOS (formatting, Clippy with warnings as errors, the test suite in
  release mode); a version tag builds the archives and publishes the GitHub Release with a
  downloads grid and this changelog. When the version in `Cargo.toml` changes on `main`,
  the tag is created automatically.
- **Soak scenario** in the Java benchmark client (steady producers with order, loss and
  duplicate checks; full speed with `--rate 0`; client compression option) and
  `scripts/soak-compare.ps1` to run it against ActiveMQRust, ActiveMQ 5.18.7 and 6.3.2.
- **Load-test results in the README**: performance at a glance and every result (steady
  load, full speed at 15, 50 and 300 KB, client compression, ActiveMQ with an 8 GB heap),
  measured with the same Java client for every broker.
- **`scripts\test.cmd`**: formats the sources and runs the tests in release mode.
- **Issue and pull request templates**: requests instead of code contributions.
- **Renovate** keeps the dependencies up to date.

### Changed

- **Broker compression is off by default.** Bodies are stored as the clients send them;
  set `broker.compress_threshold_kb` to compress large bodies in the broker again.
- **Admin console**: the broker addresses are shown under the title, and the theme
  follows the system (the manual light/dark switch is gone).
- **New logo**: "Rust" in oxidised metal with a moving light reflection.
- **Dependencies**: argon2 0.6 (session tokens now come from getrandom), base64 0.23; the
  Java test client's `amq5` profile uses activemq-client 5.19.11 (CVE-2026-33227 and
  CVE-2026-39304).
- **Sources formatted with rustfmt** (`max_width = 120`), with LF line endings.

## [0.2.0] - 2026-10-06

### Added

- **Command line setup**: `init-config`, `set-admin`, `user add | passwd | remove | list`
  and `--password-stdin`, with password and username rules. The configuration file is
  edited keeping comments, key order and line endings, validated and replaced atomically.
  Short help with the getting-started steps, long help with examples and exit codes.
- **Admin console completed**: form login with in-memory sessions, login throttling,
  Origin/Referer checks, an XML view of text bodies, a responsive light/dark stylesheet
  with no JavaScript, a sortable queues table, expiration and compression details in pages
  and in the JSON API.
- **Comparison benchmark** (`scripts/compare-activemq.ps1`): dry run, CPU of broker,
  client and machine and broker memory per phase.
- **Soak comparison** with ActiveMQ 5.18.7 and 6.3.2 published in the README; acceptance
  scenarios also confirmed against the real ActiveMQ brokers.

### Changed

- **Message selectors follow ActiveMQ's selector engine**: Java-typed values and
  conversions, the grammar's lexer rules, all header identifiers, LIKE matchers;
  checked against a conformance table of 568 selectors.
- **Hot path and compression**: zlib level 2, compression golden vectors, FIFO and
  runtime progress during large compressions, benchmarks split by area.
- **Start-up messages** name the configuration source, the console user and the number
  of messaging users, and the commands that replace default credentials.

### Fixed

- **Lost queue messages with concurrent producers**: a message enqueued by another
  connection with a lower sequence number after a consumer's cursor had moved past it was
  never dispatched.

## [0.1.0] - 2026-10-05

The initial broker.

### Added

- **In-memory OpenWire broker** compatible with the ActiveMQ Java client 5.18.x and 6.x
  (protocol versions 9-12): queues, topics, temporary destinations, local transactions,
  selectors, expiration with a sweeper, message compression and ActiveMQ-compatible IDs.
- **Admin console** and JSON API on port 8161.
- **Windows service**: `mqrust.exe service install | uninstall | start | stop | status`.
- **Configuration** in `mqrust.toml`, with errors that name the full key path; socket
  buffers sized for large dispatches and the runtime sized like the JVM
  (`broker.processors`).
- **Tests**: Rust unit and integration tests, golden vectors from the Java OpenWire
  marshaller, Java client acceptance tests and the ActiveMQ comparison scripts.
