## 1. Configuration

- [x] 1.1 Add `[admin]` keys `session_idle_minutes`, `session_max_hours`, `login_max_failures`, `login_lockout_seconds` with range validation and errors that name the key
- [x] 1.2 Add the keys, commented with defaults, to the `init-config` template; unit tests for defaults and invalid values

## 2. Sessions and login

- [x] 2.1 Create `src/admin/session.rs`: token generation (32 random bytes, URL-safe base64), store keyed by SHA-256, idle and absolute expiry, cap of 1,000 sessions, sweep every minute
- [x] 2.2 Add the per-IP failure table (15-minute window, lockout, cap of 10,000 IPs, reset on success, disabled when `login_max_failures = 0`)
- [x] 2.3 Verify the password against the configured secret also for unknown usernames (result discarded), so they take the same time as wrong passwords
- [x] 2.4 Implement `GET /login` (form, default-credentials warning) and `POST /login` (generic error, kept username, `429` during lockout, new session at every login, safe `next` redirect)
- [x] 2.5 Implement `POST /logout` (delete session, clear cookie, redirect)
- [x] 2.6 Add the `Origin`/`Referer` check on both `POST` endpoints (`403` on mismatch)
- [x] 2.7 Rewrite the guard: public paths, cookie for HTML (`303` to login), cookie or Basic for `/api/*` (`401` JSON, no `WWW-Authenticate`), failed Basic attempts counted in the throttling
- [x] 2.8 Update the failed-login logging (form and API, once per lockout, never password or token)
- [x] 2.9 Allow `POST` only on `/login` and `/logout`; `405` elsewhere

## 3. XML view

- [x] 3.1 Add `quick-xml` and create `src/admin/xml.rs`: detection, well-formedness check with error position, formatter with indentation rules, HTML escaping and colouring classes
- [x] 3.2 Enforce limits: 1 MB input, 256 KB output, depth 256, no DTD or entity processing
- [x] 3.3 Add the Raw/Formatted links and the `view=xml` handling on the message page, kept by auto-refresh
- [x] 3.4 Add `GET /api/queues/{name}/messages/{id}` with `view=xml` (`formattedBody`, `formatError`)
- [x] 3.5 Unit tests: indentation, text-only elements, comments/CDATA/PI preserved, references not expanded, malformed input with position, XXE and billion-laughs inputs, size and depth limits

## 4. Interface

- [x] 4.1 Rewrite `style.css` with colour, spacing and radius tokens; light palette on `:root`, dark palette for `prefers-color-scheme: dark` and `data-theme="dark"`; keep it under 20 KB
- [x] 4.2 New page shell: `color-scheme` and viewport metas, `lang`, top bar with product, navigation (`<details>` menu below 768 px), theme selector, user and logout; `<main>` content area with width cap
- [x] 4.3 Overview as responsive cards; tables wrapped in scroll containers with sticky headers, zebra rows, right-aligned numbers; badges with words for states
- [x] 4.4 Implement `GET /theme/{auto|light|dark}` (cookie set or cleared, safe redirect) and render `data-theme` from the cookie on every page, login included
- [x] 4.5 Login page in the same style, labelled fields, password-manager friendly
- [x] 4.6 Response headers: extended CSP, `Referrer-Policy`, `Cache-Control: no-store` behind login
- [x] 4.7 Check contrast of every palette pair (4.5:1 text, 3:1 non-text) and record the values in a comment in the stylesheet
- [x] 4.8 Queues table: keep server-side sorting on all columns; numeric and case-insensitive order, ties by name, `aria-sort`, sort kept by refresh and theme links, unknown column falls back to Name; same `sort`/`order` on `/api/queues`; tests
- [x] 4.9 Footer on every page and on `/login`: `ActiveMQRust <version>`, repository link from `CARGO_PKG_REPOSITORY`, `by Matteo Baccan`; responsive and themed; test
- [x] 4.10 Single version source: check footer, overview, `/api/overview`, `ProviderVersion` in `WireFormatInfo`, `--version`, start-up log and the Windows file version all come from `CARGO_PKG_VERSION`; Java test reading `ProviderVersion`; test that no version literal exists outside `Cargo.toml`

## 5. Tests and documentation

- [x] 5.1 Update `tests/admin_http.rs`: redirect to login, login success and failure, lockout and expiry, logout, session fixation, open redirect, foreign origin, API with Basic and with cookie, `405` rules, headers
- [x] 5.2 Update the Java acceptance tests that open console pages to log in through the form; API checks keep Basic
- [x] 5.3 Manual check in a browser at 360 px, 768 px and 1920 px, in light and dark system mode and with each manual theme; record screenshots in the change folder (`screenshots/`, taken with headless Chrome; the 360 px views are rendered in a 360 px frame because desktop Chrome cannot open a window that narrow)
- [x] 5.4 Update the README admin section (login, logout, sessions, theme, XML view, API with Basic, the new keys, the advice to put TLS in front when exposing the console)
- [x] 5.5 Measure the executable size before and after (release `mqrust.exe`: 2,841,088 bytes before, 2,964,480 bytes after, +120 KB); `openspec validate improve-admin-console` passes
