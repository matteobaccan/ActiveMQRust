## ADDED Requirements

### Requirement: Login page
`GET /login` SHALL show an HTML form with a username field, a password field (`type="password"`, `autocomplete="current-password"`) and a submit button, styled like the rest of the console, with no JavaScript. The page SHALL be reachable without a session and SHALL contain no broker data other than the product name and version. When the built-in default credentials (`admin`/`admin`, no configuration file) are in use, the page SHALL show a warning that the default password must be changed.

#### Scenario: Login form
- **WHEN** a client without a session requests `/login`
- **THEN** the response is `200` with a form that posts `username` and `password` to `/login`, and the page names no queue, topic or connection

#### Scenario: Default credentials warning
- **WHEN** the broker runs without a configuration file and a client requests `/login`
- **THEN** the page shows a warning that the default admin credentials are in use

### Requirement: Redirect to login
An HTML page requested without a valid session SHALL be answered with `303 See Other` to `/login?next=<path>`, where `<path>` is the requested path and query, percent-encoded. The response SHALL NOT carry `WWW-Authenticate`. After a successful login the client SHALL be redirected to `next` when it is a local path starting with a single `/`, and to `/` otherwise.

#### Scenario: Unauthenticated page
- **WHEN** a client without a session requests `/queues?refresh=5`
- **THEN** the response is `303` to `/login?next=%2Fqueues%3Frefresh%3D5`, without `WWW-Authenticate` and without broker data

#### Scenario: Back to the requested page
- **WHEN** the client then logs in successfully from that login page
- **THEN** it is redirected to `/queues?refresh=5`

#### Scenario: Open redirect refused
- **WHEN** a client logs in successfully with `next=//evil.example/` or `next=https://evil.example/`
- **THEN** it is redirected to `/`

### Requirement: Session on successful login
`POST /login` with the `[admin]` credentials SHALL create a session and set a cookie `mqrust_session` holding a token of 256 bits from the operating system's secure random generator, encoded in URL-safe base64, with attributes `HttpOnly`, `SameSite=Strict`, `Path=/`, and no `Expires`/`Max-Age` (a browser session cookie). The broker SHALL store only a hash of the token. A new session SHALL be created at every login; an existing session cookie sent with the login SHALL be discarded (no session fixation). Credentials SHALL be checked with the same constant-time comparison and Argon2 verification used today.

#### Scenario: Successful login
- **WHEN** a client posts the correct username and password to `/login`
- **THEN** the response is `303` with `Set-Cookie: mqrust_session=<token>; HttpOnly; SameSite=Strict; Path=/`, and a following `GET /queues` with that cookie answers `200`

#### Scenario: Token changes at every login
- **WHEN** the same client logs in twice
- **THEN** the two tokens differ and the first one no longer opens any page

#### Scenario: Token not stored in clear
- **WHEN** a session is open
- **THEN** no log line at any level contains the token, and the session store is keyed by the token's SHA-256 hash

### Requirement: Failed login
A `POST /login` with a wrong username, a wrong password or a missing field SHALL answer `200` with the login page and the single message "Invalid username or password", the same for every cause, and SHALL keep the typed username but not the password. The response time SHALL NOT reveal whether the username exists: the password check SHALL run against the configured admin secret even when the username is wrong, and its result SHALL then be discarded.

#### Scenario: Wrong password
- **WHEN** a client posts the admin username with a wrong password
- **THEN** the login page is shown again with "Invalid username or password", the username field keeps its value and the password field is empty

#### Scenario: Unknown user
- **WHEN** a client posts an unknown username
- **THEN** the response is identical to the wrong-password case apart from the echoed username

### Requirement: Login throttling
After `admin.login_max_failures` (default 5) failed logins from the same client IP within 15 minutes, further `POST /login` from that IP SHALL be refused for `admin.login_lockout_seconds` (default 60) with `429 Too Many Requests`, the login page and a message giving the waiting time, without checking the credentials. A successful login SHALL reset the counter of that IP. The failure table SHALL hold at most 10,000 IPs, dropping the oldest entries, so it cannot grow without bound. `login_max_failures = 0` SHALL disable throttling.

#### Scenario: Lockout
- **WHEN** a client IP posts 5 wrong passwords and then the correct one within a minute
- **THEN** the sixth request answers `429` and no session is created

#### Scenario: Lockout expires
- **WHEN** the same IP posts the correct credentials 61 seconds after the lockout started
- **THEN** the login succeeds

#### Scenario: Other clients unaffected
- **WHEN** one IP is locked out
- **THEN** a login from another IP with the correct credentials succeeds

### Requirement: Session lifetime
A session SHALL expire after `admin.session_idle_minutes` (default 30) without requests and in any case `admin.session_max_hours` (default 8) after login. An expired session SHALL be treated as no session. Sessions SHALL be held only in memory and SHALL all end when the broker restarts. Pages with auto-refresh (`refresh=5`) SHALL keep the session alive. Changing the `[admin]` credentials requires a restart, which therefore ends every session.

#### Scenario: Idle timeout
- **WHEN** a session has made no request for 31 minutes with the default settings
- **THEN** the next page request is redirected to `/login`

#### Scenario: Absolute lifetime
- **WHEN** a page with `refresh=5` stays open for more than 8 hours
- **THEN** the session ends after 8 hours and the page is redirected to `/login`

#### Scenario: Restart
- **WHEN** the broker restarts
- **THEN** a cookie issued before the restart opens no page

### Requirement: Logout
Every page SHALL show the logged-in username and a logout button, which is a form posting to `/logout`. `POST /logout` SHALL delete the session on the broker, clear the cookie (`Max-Age=0`) and redirect to `/login`.

#### Scenario: Logout
- **WHEN** a logged-in client posts to `/logout` and then requests `/queues` with the old cookie
- **THEN** the second request is redirected to `/login`

### Requirement: Cross-site request protection
`POST /login` and `POST /logout` SHALL be refused with `403` when the request carries an `Origin` header (or, without `Origin`, a `Referer`) whose scheme, host and port differ from the `Host` the request was sent to. Together with `SameSite=Strict` this prevents another site from logging the user in or out.

#### Scenario: Foreign origin
- **WHEN** a request to `http://127.0.0.1:8161/logout` carries `Origin: https://evil.example`
- **THEN** the response is `403` and the session stays open

### Requirement: API authentication
Paths under `/api/` SHALL accept either a valid session cookie or HTTP Basic credentials for the `[admin]` user, checked as today, including the cache of verified credentials. Without valid credentials they SHALL answer `401` with a JSON body `{"error":"unauthorized"}` and SHALL NOT send `WWW-Authenticate`, so browsers never show a credential pop-up. Failed Basic attempts SHALL count toward the login throttling of the client IP, and while that IP is locked out its Basic requests SHALL be refused with `429` and `{"error":"too many failed logins"}` without checking the credentials; a valid session cookie SHALL keep working.

#### Scenario: Basic during a lockout
- **WHEN** a client IP has sent 5 wrong Basic credentials to `/api/overview` and then sends the correct ones
- **THEN** the response is `429` until the lockout ends, and `200` afterwards

#### Scenario: Script with Basic
- **WHEN** a script requests `/api/overview` with correct Basic credentials and no cookie
- **THEN** the response is `200` with the overview JSON

#### Scenario: Browser with session
- **WHEN** a logged-in browser requests `/api/queues` with only its session cookie
- **THEN** the response is `200`

#### Scenario: No credentials
- **WHEN** a client requests `/api/queues` without cookie or `Authorization`
- **THEN** the response is `401` with `{"error":"unauthorized"}` and no `WWW-Authenticate` header

### Requirement: Login configuration keys
The `[admin]` section SHALL accept `session_idle_minutes` (1–1440, default 30), `session_max_hours` (1–168, default 8), `login_max_failures` (0–100, default 5) and `login_lockout_seconds` (1–86400, default 60). Out-of-range values SHALL be rejected at start-up with an error that names the key. `init-config` SHALL write them, commented, with their defaults.

#### Scenario: Invalid value
- **WHEN** the configuration sets `[admin] session_idle_minutes = 0`
- **THEN** the broker does not start and the error names `admin.session_idle_minutes`
