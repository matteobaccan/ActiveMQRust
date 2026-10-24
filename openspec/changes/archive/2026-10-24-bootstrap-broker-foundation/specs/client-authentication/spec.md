## ADDED Requirements

### Requirement: Username and password login
The broker SHALL authenticate every OpenWire connection with `ConnectionInfo.userName` and `ConnectionInfo.password` against the configured `[[users]]`, or against the default user `admin`/`admin` when there is no configuration file. A connection SHALL NOT be able to use the broker before it authenticates.

#### Scenario: Valid credentials
- **WHEN** a Java client connects with a configured username and the correct password
- **THEN** `ConnectionInfo` receives a `Response` and the connection can be used

#### Scenario: Wrong password
- **WHEN** a Java client connects with a configured username and a wrong password
- **THEN** the broker replies with `ExceptionResponse` carrying `java.lang.SecurityException` ("User name [<user>] or password is invalid."), closes the connection, and the client raises `JMSSecurityException`

#### Scenario: Unknown user
- **WHEN** a Java client connects with a username that is not configured
- **THEN** the broker behaves exactly as for a wrong password

### Requirement: Credential formats
Each credential SHALL be either `password` (plain text) or `password_hash` (Argon2id, as produced by `mqrust.exe hash-password`). Plain-text passwords SHALL be compared in constant time, and their use SHALL produce a warning at startup.

#### Scenario: Argon2 hash
- **WHEN** a user is configured with `password_hash` and the client sends the matching password
- **THEN** authentication succeeds

#### Scenario: Plain-text warning
- **WHEN** at least one credential uses plain-text `password`
- **THEN** the broker logs a warning at startup recommending `password_hash`

### Requirement: Anonymous access policy
Connections with an empty username SHALL be rejected like failed logins unless `allow_anonymous = true`. The default SHALL be false.

#### Scenario: Anonymous rejected by default
- **WHEN** a client connects without a username and `allow_anonymous` is not set
- **THEN** the connection is rejected with `java.lang.SecurityException`

#### Scenario: Anonymous allowed
- **WHEN** `allow_anonymous = true` and a client connects without a username
- **THEN** the connection is accepted

### Requirement: Separate admin credentials
The admin console credentials (`[admin]`) SHALL be independent of the OpenWire users. Admin credentials SHALL NOT grant OpenWire access unless the same username and password are also configured in `[[users]]`.

#### Scenario: Admin cannot use OpenWire by default
- **WHEN** a file configures `[admin] username = "root"` and no `[[users]]` entry for `root`
- **THEN** an OpenWire connection as `root` is rejected

### Requirement: Uniform destination access
Any authenticated user SHALL be allowed to use any destination. There are no per-destination permissions.

#### Scenario: Any queue
- **WHEN** an authenticated user creates a producer on any queue name
- **THEN** the broker accepts it
