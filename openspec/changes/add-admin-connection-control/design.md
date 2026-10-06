## Context

Each OpenWire connection has a `ConnHandle` (`src/broker/conn.rs`) with its remote address and an outbound channel; `close_after(cmds)` sends commands and then closes, as done for a rejected login with a `ConnectionError`. `Connection::cleanup` already handles a dropped connection: inflight messages back to pending as redelivered, transactions rolled back, temporary destinations removed, advisory state released. `src/server.rs` accepts sockets and spawns `connection::serve`; the TLS change adds a second listener. `setup.rs` has `write_atomic` (temporary file in the same folder, then rename). Console writes (`add-admin-message-removal`) use confirmation pages, `POST`, the same-origin check and `admin.read_only`.

## Goals / Non-Goals

**Goals:**
- Close one connection with the same cleanup as a network drop.
- Keep a host out, across restarts, until the ban expires or is removed.
- Refuse banned hosts as early and as cheaply as possible.

**Non-Goals:**
- Banning users or client IDs (a user is disabled with `mqrust.exe user remove` or by changing its password).
- Banning access to the admin console.
- A firewall replacement: the TCP connection is still accepted by the OS before being closed.

## Decisions

### D1. Close = ConnectionError, then the normal cleanup
The admin action calls `ConnHandle::close_by_admin()`, which sends a `ConnectionError` (`exception` = `javax.jms.JMSException`, message "Connection closed by the administrator") and closes the socket after it is written. The connection task then runs `cleanup` exactly as on a drop. A Java client sees the exception through its `ExceptionListener`; with `failover:` it reconnects, which is why "Ban IP" exists.
*Alternative*: drop the socket without a message. Rejected: the client logs an unexplained I/O error, and operators cannot tell an administrator's action from a network failure.

### D2. Bans by IP address or CIDR range
A ban is an address or a range (`203.0.113.7`, `10.1.0.0/16`, `2001:db8::/32`). IPv4-mapped IPv6 addresses (`::ffff:203.0.113.7`) are matched as IPv4. Exact addresses are kept in a `HashSet`, ranges in a short list checked in order; lookups take a read lock only at `accept`.
*Alternative*: ban by client ID or username. Rejected: both are chosen by the client and change freely; the IP is what the broker sees before reading anything.

### D3. Refusal at accept
Right after `accept`, before TLS or OpenWire, the remote IP is checked; a banned connection is closed immediately without reading or writing anything, and the ban's refused counter is increased. Refusals are logged at `debug` per attempt and summarised by one `warn` per minute with the count per address.
*Alternative*: refuse after the OpenWire login with a security exception. Rejected: it spends a TLS handshake and protocol parsing on a host the operator wants out.

### D4. Durations and expiry
1 hour, 24 hours, 7 days or permanent, chosen on the confirmation page (default 24 hours). Expiry is checked at lookup (an expired ban never refuses) and expired bans are removed from the list and the file by the housekeeping task within a minute.
*Alternative*: permanent only. Rejected: temporary bans are the common case for a misbehaving client that will be fixed.

### D5. Closing connections when banning
The ban confirmation page shows how many connections are open from the address or range and a checkbox "Also close these connections", checked by default. Without it, the ban only affects new connections.
*Alternative*: always close. Rejected: an operator may want to stop reconnections while a client finishes its work.

### D6. Persistence in `bans.toml`
Bans are stored in `bans.toml` in the folder of the configuration file (next to the executable when the built-in defaults are used), one `[[ban]]` table per entry: `address`, `reason`, `created` (RFC 3339), `created_by`, `expires` (RFC 3339, absent = permanent). Every change rewrites the file with the atomic write helper; a write failure keeps the in-memory change, is logged as a warning and shown on the page. At startup and in `check-config` the file is parsed: a syntax error or an invalid address is a configuration error naming the file, line and entry, exit code 2. Hand edits apply at the next restart. The refused-attempt counters are not stored.
*Alternative 1*: store bans in `mqrust.toml`. Rejected: the console would rewrite the operator's main configuration on every ban.
*Alternative 2*: in memory only. Rejected: a restart would let banned hosts back in.

### D6b. Automatic bans after failed logins
A per-IP failure tracker (in memory) records refused OpenWire logins (wrong username or password only). Three failures within a sliding 15-minute window create an automatic ban of 1 minute; each failure after an automatic ban has expired doubles the next duration (2, 4, 8 … minutes, capped at 24 hours). A successful login, an "Unban", or 24 hours without failures resets the address. Automatic bans live in the same in-memory list as manual ones with an `automatic` flag, so the accept check is the same; they are never written to `bans.toml`, and a restart clears them and the tracker. The tracker is pruned by the housekeeping task, so a scan from many addresses cannot grow it without bound (entries older than 24 hours are dropped).
*Alternative 1*: count protocol and TLS failures too. Rejected: a misconfigured but honest client would ban itself; only credential guessing is the target.
*Alternative 2*: persist automatic bans. Rejected by product decision: a restart clears everything.
*Alternative 3*: no cap on the doubling. Rejected: after 20 doublings a ban would last about two years.

### D7. Pages, security and logging
Row actions "Close" and "Ban IP" on `/connections` lead to confirmation pages (`GET`) whose button sends a `POST`; `/bans` lists bans and has the "Add ban" form and "Unban" actions, also confirmed. Every `POST` requires a session and the same-origin check, as the other console writes; `admin.read_only = true` hides the actions and answers `405`. Info log lines: `admin <user> from <ip> closed connection <id> (<client>)`, `admin <user> from <ip> banned <address> until <time|permanent>: <reason>`, `admin <user> from <ip> removed ban <address>`. Banning `127.0.0.1`, `::1` or the address the administrator is using shows a warning on the confirmation page but is allowed (it affects OpenWire only).
*Alternative*: forbid banning loopback. Rejected: a local runaway client is a legitimate target; the console stays reachable.

### D8. API
`POST /api/connections/{id}/close`; `GET /api/bans`; `POST /api/bans` with `{"address", "duration": "1h"|"24h"|"7d"|"permanent", "reason", "closeConnections"}`; `DELETE /api/bans/{address}` (percent-encoded, `/` of a range included). Same authentication and origin rules as the other write endpoints.

## Risks / Trade-offs

- [Clients behind NAT or a proxy share one IP] → banning that IP stops all of them; the confirmation page shows how many connections the ban affects.
- [The OS still completes the TCP handshake] → documented: for floods, use the OS firewall; the broker closes banned sockets without reading.
- [`bans.toml` edited while the broker runs] → the next console change rewrites it from the in-memory list; the README says to edit it only while the broker is stopped.
