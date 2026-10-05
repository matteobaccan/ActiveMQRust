## Why

The admin console authenticates with HTTP Basic: the browser shows its own credential pop-up, there is no way to log out short of closing the browser, credentials are resent with every request, and repeated wrong passwords are only logged, never slowed down. Operators also open messages whose body is XML (the common case for the applications that move from ActiveMQ), and today they see it on one line or with the producer's indentation, which makes it hard to read. Finally, the pages are a plain table layout that is hard to use on a laptop split screen or a phone, and they are always light, even when the operator's system is in dark mode.

## What Changes

- **BREAKING (console only)**: HTML pages no longer answer `401` with a Basic challenge. An unauthenticated request is redirected to a login page with a proper form, and a successful login opens a server-side session held in a cookie.
- Sessions: random token in an `HttpOnly`, `SameSite=Strict` cookie, held in memory; idle timeout and absolute lifetime; logout; the sessions end when the broker restarts.
- Protection of the login: per-client-IP throttling after repeated failures, the same response for a wrong username and a wrong password, failed logins logged as today (never the password).
- The page header shows the logged-in user and a logout button; the login page warns when the built-in default credentials are in use.
- The JSON API keeps accepting HTTP Basic (for scripts and monitoring) and also accepts the session cookie; without credentials it answers `401` without a Basic challenge, so no browser pop-up appears.
- The console stays read-only for broker state: `POST` is accepted only for login and logout.
- Message page: when the body is XML, a "Formatted" view re-indents it server-side, with syntax colouring done in CSS, next to the unchanged "Raw" view. Formatting never changes the stored message and never resolves DTDs or external entities.
- Modern, responsive look: a navigation bar that collapses on narrow screens, cards for the overview figures, tables that scroll horizontally inside their own box instead of widening the page, readable from 360 px wide phones up to wide monitors.
- Light and dark theme following the operating system setting (`prefers-color-scheme`), with no JavaScript, and a manual choice (automatic, light, dark) remembered in a cookie.
- Queues table: every column sortable (already true today), made explicit and stricter: numeric order, stable ties, sort kept on refresh, same sorting on `/api/queues`.
- Footer on every page: project name and version, GitHub repository link, "by Matteo Baccan".
- New `[admin]` configuration keys for the session timeouts and the login throttling.

## Capabilities

### New Capabilities

- `admin-login`: login page, session cookie, session lifetime, logout, login throttling, API authentication, CSRF protection of the two `POST` endpoints.
- `admin-xml-view`: XML detection and formatted view of message bodies in the console.
- `admin-ui`: responsive layout, light/dark theme that follows the operating system with a manual override, sortable queues table, footer.

### Modified Capabilities

- `admin-console` (from `add-admin-console`): "HTTP Basic authentication" is replaced by `admin-login` for HTML pages and narrowed to the JSON API; "Read-only console" allows `POST /login` and `POST /logout`; "Server-side HTML with embedded assets" adds the user and logout control to every page header.

## Impact

- `src/admin/mod.rs` (authentication guard, session store, login and logout handlers), `src/admin/pages.rs` (login page, page header, message page view switch), `src/admin/style.css` (rewritten: design tokens for both themes, responsive layout, login form, XML colouring); new `src/admin/session.rs` and `src/admin/xml.rs`.
- `src/config.rs`: new `[admin]` keys `session_idle_minutes`, `session_max_hours`, `login_max_failures`, `login_lockout_seconds`; template and validation updated.
- New dependency: an XML pull parser that does not process DTDs (`quick-xml`), used only by the console, never on the message path.
- `tests/admin_http.rs` and the Java acceptance tests that call the console with Basic credentials: HTML checks move to the login flow; API checks keep Basic.
- No change to OpenWire, to messaging or to memory use per message; sessions take a few hundred bytes each.
