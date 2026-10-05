## MODIFIED Requirements

### Requirement: HTTP Basic authentication
Every console path, HTML and JSON, SHALL require authentication with the `[admin]` credentials (`username` with `password` or `password_hash`), or `admin`/`admin` when there is no configuration file. HTML pages SHALL authenticate with the login form and session cookie defined by `admin-login`; a request without a valid session SHALL be redirected to `/login` and receive no broker data. Paths under `/api/` SHALL accept HTTP Basic credentials or the session cookie, and without valid credentials SHALL answer `401` with no broker data and without a `WWW-Authenticate` header. Admin credentials SHALL be independent of the OpenWire `[[users]]`.

#### Scenario: No credentials
- **WHEN** a client requests `/queues` without a session cookie
- **THEN** the response is `303` to `/login?next=%2Fqueues` and the body contains no queue names

#### Scenario: No credentials on the API
- **WHEN** a client requests `/api/overview` without a session cookie or an `Authorization` header
- **THEN** the response is `401` without `WWW-Authenticate`

#### Scenario: Wrong password
- **WHEN** a client posts the admin username and a wrong password to `/login`
- **THEN** no session is created and the login page shows "Invalid username or password"

#### Scenario: Valid credentials with Argon2 hash
- **WHEN** `[admin]` uses `password_hash` and a client logs in with the matching password
- **THEN** it receives a session cookie and `/` answers `200` with the overview page

#### Scenario: OpenWire user cannot log in to the console
- **WHEN** `[[users]]` contains `app1`/`secret`, `[admin]` uses another username, and a client logs in as `app1`/`secret`, or calls `/api/overview` with Basic `app1`/`secret`
- **THEN** the login fails and the API answers `401`

### Requirement: Failed admin login logging
Every failed login, through the login form or through HTTP Basic on the API, SHALL be logged as a warning with the remote IP and the username; a refused attempt during a lockout SHALL be logged once per lockout. The password and the session token SHALL NOT appear in any log line at any level. A request without credentials SHALL NOT be logged as a failed login.

#### Scenario: Wrong credentials are logged without password
- **WHEN** a client at `127.0.0.1` posts `admin` with password `wrong-pw-123` to `/login`
- **THEN** a warning containing `127.0.0.1` and `admin` is logged, and the string `wrong-pw-123` appears in no log line

#### Scenario: First browser request is not a failed login
- **WHEN** a client requests `/` without a session
- **THEN** the response redirects to `/login` and no failed-login warning is logged

### Requirement: Read-only console
The console SHALL NOT change broker state. Only `GET` and `HEAD` SHALL be accepted, except `POST /login` and `POST /logout`, which change only the console session; any other method or path SHALL receive `405 Method Not Allowed`. Viewing queue contents or a message, in any view, SHALL NOT consume, acknowledge, reorder or redeliver messages, and SHALL NOT change any counter.

#### Scenario: Write method rejected
- **WHEN** an authenticated client sends `POST /queues/TEST.A` or `DELETE /api/queues/TEST.A`
- **THEN** the response is `405` and the queue is unchanged

#### Scenario: Browsing does not consume
- **WHEN** a queue holds 10 messages and an authenticated client views its contents page and every message detail page, raw and formatted
- **THEN** a consumer created afterwards receives all 10 messages in FIFO order with `JMSRedelivered=false`, and the queue counters are unchanged by the views

### Requirement: Server-side HTML with embedded assets
Pages SHALL be generated server-side as HTML, with CSS embedded in the executable and no JavaScript. Every value taken from broker data (destination names, IDs, properties, bodies, usernames) SHALL be HTML-escaped. Responses SHALL carry `Content-Security-Policy: default-src 'none'; style-src 'self'; form-action 'self'; frame-ancestors 'none'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin` and, on pages behind login, `Cache-Control: no-store`. Every page behind login SHALL show the logged-in username, the theme selector and a logout button in its top bar. Every page SHALL support optional auto-refresh every 5 seconds, enabled with the query parameter `refresh=5` and kept on the page's links.

#### Scenario: Escaped content
- **WHEN** a queue named `Q<script>` holds a TextMessage whose text and a property value are `<script>alert(1)</script>`
- **THEN** the queues page, the queue detail page and the message page contain the escaped text `&lt;script&gt;` and no `<script>` element

#### Scenario: Auto-refresh
- **WHEN** a logged-in client requests `/queues?refresh=5`
- **THEN** the page contains `<meta http-equiv="refresh" content="5">` and the links on the page keep `refresh=5`

#### Scenario: No asset files on disk
- **WHEN** the broker runs from a folder that contains only `mqrust.exe`
- **THEN** every console page renders with its stylesheet

#### Scenario: User and logout in the top bar
- **WHEN** a client logged in as `admin` opens any page
- **THEN** the top bar shows `admin` and a logout button

#### Scenario: Pages not cached
- **WHEN** a logged-in client requests `/queues`
- **THEN** the response carries `Cache-Control: no-store`
