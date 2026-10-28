## Why

The Connections page only shows who is connected. When a client misbehaves (a runaway producer filling memory, a consumer stuck with thousands of inflight messages, an unknown host trying passwords), the operator can only restart the broker, which drops every client and, since messages live in RAM, every message. Operators need to close one connection, and to stop a host from coming back: a client with a `failover:` URL reconnects within a second of being closed.

## What Changes

- **Close a connection**: a "Close" action per row of the Connections page. The broker sends the client a `ConnectionError` with the text "Connection closed by the administrator" and closes the socket; the cleanup is the same as for a dropped connection (unacknowledged messages back to their queues as redelivered, open transactions rolled back, temporary destinations of the connection removed).
- **Ban an IP address**: a "Ban IP" action per row, and an "Add ban" form for an IP address or a CIDR range. A banned address is refused by every OpenWire listener (plain and TLS) right after `accept`, before any byte is read. Banning offers to close the open connections from that address at the same time (on by default).
- Ban duration: 1 hour, 24 hours, 7 days or permanent, with an optional reason. Expired bans are removed automatically.
- **Automatic ban** after failed logins: 3 wrong passwords from one IP within 15 minutes ban it for 1 minute; each further failure after the ban doubles the duration, up to 24 hours; a successful login or 24 hours without failures resets it. Kept in memory only: a restart clears every automatic ban.
- New page `/bans`: address or range, reason, who created it and when, expiry, refused attempts, "Unban".
- Bans survive restarts: they are stored in `bans.toml` next to the configuration file, written atomically; the file can be edited by hand and is checked at startup and by `check-config`.
- Confirmation page for every action, `POST` only, same-origin check, info log with admin user and IP, JSON API.
- The admin console itself is not affected by bans, so an administrator cannot lock themselves out of it.

## Capabilities

### New Capabilities

- `admin-connection-control`: closing connections, IP and CIDR bans, automatic bans after failed logins, ban durations and expiry, `/bans` page, `bans.toml`, refusal at accept, logging, API, security of the actions.

### Modified Capabilities

None directly. These are write operations of the console: they follow the rules of "Console write operations" and the `admin.read_only` switch introduced by `add-admin-message-removal`, which this change depends on.

## Impact

- Code: `src/broker/conn.rs` (close by administrator), `src/connection.rs` (send `ConnectionError` and run the normal cleanup), new `src/bans.rs` (ban list, matching, expiry, file load and atomic save), `src/server.rs` (check at accept on every listener), `src/admin/pages.rs` (row actions, confirmation pages, `/bans`), `src/admin/api.rs`, `src/admin/mod.rs` (routes), `src/config.rs` / `check-config` (`bans.toml` validation), tests.
- No new crates (`toml_edit` and the atomic write helper of `setup.rs` are reused).
- Accept path: one lookup in an in-memory set per new connection; no cost per message.
- ActiveMQ comparison: ActiveMQ can stop a connection through JMX but has no IP ban in its console; the ban list is an addition.
