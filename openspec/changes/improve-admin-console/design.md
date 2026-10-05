## Context

The console from `add-admin-console` is server-side HTML on `axum`, protected by HTTP Basic with a cache of verified `Authorization` headers, styled by one embedded stylesheet, with a strict CSP (`default-src 'none'; style-src 'self'`) and no JavaScript. It is read-only. Operators asked for a real login with logout, a readable view of XML bodies, and a modern, responsive interface that follows the system light/dark theme.

## Goals / Non-Goals

**Goals:**
- Form login with server-side sessions, logout, lockout after repeated failures, safe redirects.
- Keep scripts working: the JSON API still accepts HTTP Basic.
- Formatted, coloured view of XML bodies, safe against XXE and entity bombs.
- Responsive layout from phone to wide monitor; light/dark theme from the operating system, with a manual override.
- Keep the console free of JavaScript and external resources, and keep the CSP strict.

**Non-Goals:**
- HTTPS on the admin listener (the console stays local by default; TLS is a separate change). Because of this the session cookie is not marked `Secure`.
- Multiple admin users, roles, or OpenWire users logging in to the console.
- Persistent sessions across restarts, "remember me".
- Formatting JSON or other body formats; editing, sending or deleting messages.

## Decisions

1. **Sessions in memory, keyed by SHA-256 of the token.** A `parking_lot::Mutex<HashMap<[u8; 32], Session>>` in `src/admin/session.rs`, with `Session { user, created, last_seen }`. Tokens are 32 bytes from the OS generator (`getrandom`, already pulled in by `argon2`), sent as URL-safe base64. Hashing means a memory dump does not yield usable cookies. Expired sessions are removed lazily on lookup and by a sweep every minute; the map is capped at 1,000 sessions (oldest dropped). *Alternative:* signed stateless cookies (HMAC) — rejected because logout and lockout need server-side state anyway.

2. **Cookie attributes: `HttpOnly; SameSite=Strict; Path=/`, no `Secure`.** The listener is plain HTTP; `Secure` would make the cookie unusable. `SameSite=Strict` plus an `Origin`/`Referer` check on the two `POST` endpoints covers CSRF without tokens in forms. When TLS arrives, `Secure` is added.

3. **Throttling per IP in a bounded map.** `HashMap<IpAddr, Failures { count, first, locked_until }>`, max 10,000 entries with oldest-first eviction, window 15 minutes. Locked requests are refused before Argon2 runs, so a flood cannot burn CPU on hashing. Unknown usernames are verified against a fixed dummy Argon2 hash so timing does not reveal valid names.

4. **Guard split by path.** One middleware: `/login`, `/theme/*`, `/style.css` are public; `/api/*` accepts the cookie or Basic (the existing verified-header cache stays); everything else needs the cookie, else `303` to `/login?next=…`. The `next` value is accepted only if it starts with `/` and not `//` or `/\`.

5. **XML formatting with `quick-xml`.** A pull parser that never reads DTDs or resolves entities and does not allocate per entity, so XXE and billion-laughs are impossible by construction. The formatter writes events back with indentation, escaping everything for HTML and wrapping tokens in `<span class="x-tag|x-attr|x-val|x-com|x-cdata|x-pi">`. Text-only elements stay on one line by buffering one start tag until the next event. Limits: 1 MB input, 256 KB output, depth 256. Well-formedness is checked in the same pass (`check_end_names`), and the error position is reported for the "Not well-formed XML" notice. *Alternative:* `roxmltree` — rejected because it builds a full DOM (more memory) and is less suited to streaming output with limits.

6. **View switch with links, not scripts.** `?view=xml` selects the formatted view; links keep `refresh` and `view`. This keeps the CSP without `script-src`.

7. **Theme without JavaScript.** Colour tokens as CSS custom properties: light values on `:root`, dark values under `@media (prefers-color-scheme: dark)` for `:root:not([data-theme="light"])`, and again under `:root[data-theme="dark"]`. The server writes `data-theme` on `<html>` only when the `mqrust_theme` cookie forces a theme. The selector is three small links to `GET /theme/{mode}?next=…`, which set or clear the cookie and redirect. A `GET` is acceptable here because it changes only a display preference, never broker state.

8. **Responsive layout with CSS only.** CSS grid for the card area (`repeat(auto-fit, minmax(12rem, 1fr))`), a content width cap of about 80rem, tables wrapped in `<div class="table-wrap">` with `overflow-x: auto`, navigation in a `<details>` element below 768 px, `overflow-wrap: anywhere` for IDs. The stylesheet stays one embedded file under 20 KB.

9. **Configuration.** New `[admin]` keys with validated ranges in `config.rs`, defaults as in the spec, added to `TEMPLATE`.

## Risks / Trade-offs

- [Plain-HTTP cookie can be sniffed on a network] → The console binds to `127.0.0.1` by default; the docs warn that exposing it needs TLS in front (reverse proxy) until a TLS change exists.
- [Throttling per IP can lock out users behind the same NAT] → Lockout is short (60 s by default), configurable, and can be disabled with `login_max_failures = 0`.
- [Sessions lost at restart] → Accepted: the broker is in-memory by design; the operator just logs in again.
- [Breaking change for people who use the browser with Basic URLs or scripts calling HTML pages] → HTML pages move to the login form; the JSON API keeps Basic, so monitoring scripts keep working. Documented in the README.
- [New dependency `quick-xml`] → Small, pure Rust, used only by the console; adds a little to the executable size, measured in the tasks.
- [Formatting large XML costs CPU on the admin request] → Capped at 1 MB input and done outside any destination lock, on a copy of the body already taken for rendering.
