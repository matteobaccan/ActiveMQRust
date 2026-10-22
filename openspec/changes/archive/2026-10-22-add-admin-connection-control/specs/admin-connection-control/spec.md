## ADDED Requirements

### Requirement: Close a connection
Each row of the Connections page SHALL offer a "Close" link to a confirmation page showing the connection ID, user, client address, and the numbers of consumers, producers and inflight messages, with a "Close connection" button that sends a `POST`. The `POST` SHALL send the client a `ConnectionError` whose exception message is "Connection closed by the administrator", close the socket, and clean up as for a dropped connection: unacknowledged messages SHALL return to their queues and be redelivered with `JMSRedelivered=true`, open transactions SHALL be rolled back, and temporary destinations created by the connection SHALL be removed. The browser SHALL be redirected (`303`) to the Connections page with the notice "Connection <id> closed", or "Connection <id> is no longer open" when it had already closed.

#### Scenario: Close a consumer connection
- **WHEN** a Java client holds 5 unacknowledged messages from `Q1` and an administrator closes its connection
- **THEN** the client's `ExceptionListener` receives an exception with "Connection closed by the administrator", and the 5 messages are pending again in `Q1` with `JMSRedelivered=true` for the next consumer

#### Scenario: Transaction rolled back
- **WHEN** a transacted producer has sent 3 messages without committing and its connection is closed by an administrator
- **THEN** none of the 3 messages is delivered

#### Scenario: Already gone
- **WHEN** the connection closes by itself after the confirmation page was opened, and the administrator then confirms
- **THEN** the Connections page shows "Connection <id> is no longer open" and nothing else changes

### Requirement: Ban an address
Each row of the Connections page SHALL offer a "Ban IP" link, and the `/bans` page an "Add ban" form accepting an IPv4 or IPv6 address or a CIDR range. The confirmation page SHALL show the address or range, the number of connections currently open from it, a duration choice (1 hour, 24 hours, 7 days, permanent; default 24 hours), an optional reason of at most 200 characters, and a checkbox "Also close these connections", checked by default. When the address is `127.0.0.1`, `::1` or the address the administrator is connected from, the page SHALL show a warning, and the ban SHALL still be allowed. Confirming SHALL add the ban and, when the checkbox is set, close every open connection from it as in "Close a connection". An invalid address or range SHALL be refused with a message and nothing added.

#### Scenario: Ban from a connection row
- **WHEN** an administrator bans the address of a connection from `203.0.113.7` for 24 hours with "Also close these connections" set
- **THEN** every connection from `203.0.113.7` is closed and `/bans` lists `203.0.113.7` with an expiry 24 hours later

#### Scenario: Ban a range
- **WHEN** an administrator adds a permanent ban for `10.1.0.0/16`
- **THEN** a client connecting from `10.1.2.3` is refused and one from `10.2.0.1` is accepted

#### Scenario: Ban without closing
- **WHEN** an administrator bans an address and clears "Also close these connections"
- **THEN** the open connections from that address keep working and new connections from it are refused

#### Scenario: Invalid address
- **WHEN** the "Add ban" form is submitted with `300.1.1.1`
- **THEN** the page explains that the address is invalid and no ban is added

### Requirement: Refusal of banned addresses
Every OpenWire listener, plain and TLS, SHALL check the remote IP address right after accepting a connection, before reading any byte or starting a TLS handshake, and SHALL close a connection from a banned address without sending anything. IPv4-mapped IPv6 addresses SHALL be matched as IPv4. Each refusal SHALL increase the ban's refused-attempt counter, SHALL be logged at debug level, and at most one warning per minute SHALL summarise the refusals per address. An expired ban SHALL NOT refuse any connection. Bans SHALL NOT apply to the admin console.

#### Scenario: Failover client kept out
- **WHEN** a client with `failover:(tcp://broker:61616)` is closed and banned
- **THEN** its reconnection attempts are refused, `/bans` shows the refused-attempt count growing, and the log has at most one warning per minute about it

#### Scenario: Admin console still reachable
- **WHEN** the administrator bans their own address
- **THEN** the admin console keeps working from that address

#### Scenario: Expiry
- **WHEN** a 1-hour ban was added 61 minutes ago
- **THEN** a connection from that address is accepted and the ban is no longer listed

### Requirement: Automatic ban after failed logins
The broker SHALL count, per IP address, the OpenWire logins refused because of a wrong username or password; protocol errors, TLS handshake failures and refusals of banned addresses SHALL NOT count. When an address reaches 3 failed logins within 15 minutes, the broker SHALL ban it automatically for 1 minute and close its open connections. Each further failed login from that address after the ban has expired SHALL ban it again for twice the previous duration (2, 4, 8 minutes and so on), up to at most 24 hours. The escalation of an address SHALL reset after a successful login from it or after 24 hours without failed logins. Automatic bans and failure counts SHALL be kept in memory only: they SHALL NOT be written to `bans.toml` and SHALL all be cleared when the broker restarts. Automatic bans SHALL be refused at accept as in "Refusal of banned addresses", SHALL be listed on `/bans` and in `/api/bans` marked `automatic` with their expiry, and SHALL be removable with "Unban", which also resets the escalation of that address. A manual ban of the same address SHALL take precedence. Each automatic ban SHALL be logged as a warning: `automatic ban of <ip> for <duration> after <n> failed logins`.

#### Scenario: Third failure
- **WHEN** a client from `203.0.113.7` fails the login 3 times within 15 minutes
- **THEN** connections from `203.0.113.7` are refused for 1 minute and `/bans` lists it as automatic

#### Scenario: Doubling
- **WHEN** after the 1-minute ban has expired the same address fails the login once more, and then again after the next ban expires
- **THEN** it is banned for 2 minutes, then for 4 minutes

#### Scenario: Cap
- **WHEN** the doubling would exceed 24 hours
- **THEN** the ban lasts 24 hours

#### Scenario: Reset after success
- **WHEN** a banned address's ban expires and its next login succeeds
- **THEN** a later failed login starts counting from zero again, and 3 failures within 15 minutes give a 1-minute ban

#### Scenario: Restart clears automatic bans
- **WHEN** an address is automatically banned for 8 minutes and the broker is restarted
- **THEN** the address can connect at once and its failure count is zero, while manual bans from `bans.toml` still apply

#### Scenario: Failures spread out
- **WHEN** an address fails the login 3 times, once every 10 minutes
- **THEN** it is not banned, because no 15-minute window holds 3 failures

### Requirement: Bans page
`/bans` SHALL list every active ban with address or range, reason, created by, created at, expires (or "permanent") and refused attempts since the broker started, sortable like the other tables, with an "Unban" action per row leading to a confirmation page. Removing a ban SHALL take effect immediately for new connections. The navigation SHALL link to `/bans`.

#### Scenario: Unban
- **WHEN** an administrator removes the ban of `203.0.113.7`
- **THEN** the next connection from `203.0.113.7` is accepted

### Requirement: Ban persistence
Bans SHALL be stored in `bans.toml` in the folder of the configuration file, or next to the executable when the built-in defaults are used, one `[[ban]]` entry each with `address`, `reason`, `created`, `created_by` and, for temporary bans, `expires` (RFC 3339). Every change SHALL rewrite the file atomically (temporary file in the same folder, then rename). If the write fails, the change SHALL still apply in memory, and a warning SHALL be logged and shown on the page. At startup and in `mqrust.exe check-config` the file SHALL be validated; an error SHALL name the file, the line and the entry, and exit with code 2. A missing file SHALL mean no bans. Expired entries SHALL be removed from the file within one minute of their expiry while the broker runs, and at startup. Refused-attempt counters SHALL NOT be stored.

#### Scenario: Restart keeps bans
- **WHEN** a permanent ban is added and the broker is restarted
- **THEN** connections from that address are still refused

#### Scenario: Hand-edited file with an error
- **WHEN** `bans.toml` contains `address = "10.1.0.0/40"`
- **THEN** startup and `check-config` fail with exit code 2, naming `bans.toml`, the line and the invalid range

### Requirement: Security and logging of connection actions
Closing, banning and unbanning SHALL be performed only by `POST` (pages) or by the API methods below, from an authenticated administrator, with the same-origin check of the other console write operations, and SHALL be refused with `405` when `admin.read_only` is `true`, in which case their links SHALL NOT be shown. Viewing a confirmation page SHALL NOT change anything. Each action SHALL be logged at info level: `admin <user> from <ip> closed connection <id> (<client address>)`, `admin <user> from <ip> banned <address> until <time | permanent>: <reason>`, `admin <user> from <ip> removed ban <address>`.

#### Scenario: Cross-site ban refused
- **WHEN** a logged-in browser sends the ban `POST` with `Origin: https://evil.example`
- **THEN** the response is `403` and no ban is added

#### Scenario: Read-only console
- **WHEN** `admin.read_only = true`
- **THEN** the Connections and Bans pages show no Close, Ban IP, Add ban or Unban actions, and their `POST` paths answer `405`

### Requirement: Connection control API
The API SHALL offer `POST /api/connections/{id}/close` (`200` with `{"closed": "<id>"}`, `404` when the connection is not open), `GET /api/bans` (the list as on `/bans`), `POST /api/bans` with `address`, `duration` (`1h`, `24h`, `7d` or `permanent`), optional `reason` and `closeConnections` (default `true`), answering `200` with the ban and the number of closed connections or `400` with an error object for invalid input, and `DELETE /api/bans/{address}` with the percent-encoded address or range (`200`, or `404` when not banned).

#### Scenario: Ban by script
- **WHEN** a script with HTTP Basic credentials posts `{"address": "198.51.100.0/24", "duration": "7d", "reason": "load test"}` to `/api/bans`
- **THEN** the response is `200`, the ban is listed with an expiry 7 days later, and `bans.toml` contains it

### Requirement: Deliberate difference from ActiveMQ
ActiveMQ can stop a connection through JMX and has no IP ban in its web console. ActiveMQRust SHALL offer both in the console and the API; the `ConnectionError` sent on close SHALL be understood by unchanged ActiveMQ Java clients.

#### Scenario: Unchanged client
- **WHEN** an ActiveMQ 5.19.11 or 6.3.2 Java client is closed by the administrator
- **THEN** it reports the exception through its `ExceptionListener` and does not hang
