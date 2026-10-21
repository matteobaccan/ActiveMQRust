## 1. Closing connections

- [x] 1.1 `ConnHandle::close_by_admin()`: `ConnectionError` "Connection closed by the administrator", then close; lookup of a connection by connection ID
- [x] 1.2 Check that the normal `cleanup` runs (redelivery, rollback, temporary destinations) and test it in `tests/connection_semantics.rs`

## 2. Ban list

- [x] 2.1 `src/bans.rs`: address and CIDR parsing (IPv4, IPv6, IPv4-mapped), exact set plus range list, expiry, refused counters
- [x] 2.2 `bans.toml` load and validation (file, line, entry in errors), atomic save, location next to the configuration file
- [x] 2.3 Housekeeping: remove expired bans from memory and file within a minute
- [x] 2.4 `check-config` validates `bans.toml`
- [x] 2.5 Failure tracker: 3 failures in 15 minutes → 1-minute automatic ban, doubling after each expiry up to 24 hours, reset on success, unban or 24 hours quiet; in memory only, pruned by housekeeping
- [x] 2.6 Hook the tracker into the OpenWire login (wrong credentials only), close open connections on an automatic ban, warning log line
- [x] 2.7 Unit tests: matching, ranges, mapped addresses, expiry, file round trip, invalid entries, automatic ban escalation with a fake clock, restart clears automatic bans

## 3. Accept path

- [x] 3.1 Check the remote IP right after `accept` on every OpenWire listener (plain and, when present, TLS), close without I/O
- [x] 3.2 Debug log per refusal, one warning per minute per address with the count

## 4. Console

- [x] 4.1 Row actions "Close" and "Ban IP" on `/connections`, hidden when read-only
- [x] 4.2 Confirmation pages: close; ban (open connections count, duration, reason, close checkbox, loopback/self warning); unban
- [x] 4.3 `POST` handlers with session, same-origin check, info logs, `303` with notices
- [x] 4.4 `/bans` page, sortable, "Add ban" form, navigation link
- [x] 4.5 Styles for the row actions and the destructive buttons in both themes

## 5. API

- [x] 5.1 `POST /api/connections/{id}/close`, `GET`/`POST /api/bans`, `DELETE /api/bans/{address}`

## 6. Tests and documentation

- [x] 6.1 `tests/admin_http.rs`: every scenario of the spec, including `403`, `405` in read-only mode, GET does not change anything
- [x] 6.2 Java integration test: a 5.19.11 and a 6.3.2 client closed by the administrator report the exception; a failover client stays out while banned
- [x] 6.3 README: closing connections, bans, `bans.toml`, NAT caveat, use the OS firewall for floods
- [x] 6.4 CHANGELOG entry under Unreleased
- [x] 6.5 `cargo fmt` and the test suite
