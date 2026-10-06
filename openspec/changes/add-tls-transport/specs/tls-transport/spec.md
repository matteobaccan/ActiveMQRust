## ADDED Requirements

### Requirement: TLS listener
The `[tls]` section SHALL accept `enabled` (boolean, default `false`), `bind` (IP address, default the value of `broker.bind`) and `port` (1–65535, default `61617`). When `enabled` is `true` the broker SHALL accept OpenWire over TLS on `bind:port`, next to the plain listener, with the same protocol behaviour, authentication and semantics as plain connections. The handshake SHALL run outside the accept loop and SHALL be abandoned after 10 seconds. The startup log SHALL contain `TLS listening on <bind>:<port>` with the minimum version and the client certificate mode. The TLS port SHALL NOT equal the plain port when both listeners are open on the same address.

#### Scenario: Java client over ssl://
- **WHEN** TLS is enabled with a certificate trusted by the client's truststore and an ActiveMQ Java client 5.19.11 or 6.3.2 connects to `ssl://localhost:61617` and sends and receives messages
- **THEN** the messages are delivered as over `tcp://localhost:61616`, in FIFO order with the same IDs and headers

#### Scenario: Failover URL
- **WHEN** a Java client connects to `failover:(ssl://localhost:61617)` and the broker is restarted
- **THEN** the client reconnects over TLS

#### Scenario: Slow handshake
- **WHEN** a client opens a TCP connection to the TLS port and sends nothing
- **THEN** the broker closes it after 10 seconds and keeps accepting other connections

#### Scenario: Disabled by default
- **WHEN** the broker starts without a `[tls]` section
- **THEN** nothing listens on port 61617

### Requirement: Server certificate
The server certificate SHALL be given either as `cert` (PEM file with the certificate chain, leaf first) and `key` (PEM file with an unencrypted PKCS#8, PKCS#1 or SEC1 private key), or as `keystore` (PKCS#12 file, `.p12` or `.pfx`) and `keystore_password`; exactly one of the two forms SHALL be configured when TLS is enabled. Relative paths SHALL resolve against the folder of the configuration file. A PKCS#12 file SHALL contain exactly one private key entry. JKS keystores SHALL be refused with a message that suggests converting them with `keytool -importkeystore -deststoretype PKCS12`. The private key and the keystore password SHALL never appear in any log line or API response.

#### Scenario: PEM files
- **WHEN** `cert = "broker.crt"` and `key = "broker.key"` are configured next to `mqrust.toml`
- **THEN** the broker starts and presents that certificate chain

#### Scenario: PKCS#12 keystore
- **WHEN** a keystore created with `keytool -genkeypair -storetype PKCS12` is configured with its password
- **THEN** the broker starts and presents the certificate of its key entry

#### Scenario: JKS refused
- **WHEN** `keystore` points to a JKS file
- **THEN** startup fails with exit code 2, naming `tls.keystore` and suggesting the `keytool -importkeystore` conversion

#### Scenario: Both forms
- **WHEN** both `cert` and `keystore` are configured
- **THEN** startup fails with exit code 2 and a message naming both keys

### Requirement: Protocol versions and cipher suites
The broker SHALL accept TLS 1.2 and TLS 1.3, and `tls.min_version` (`"1.2"` default, or `"1.3"`) SHALL set the lowest accepted version. SSLv3, TLS 1.0 and TLS 1.1 SHALL always be refused. Only AEAD cipher suites with forward secrecy SHALL be offered, and no configuration key SHALL enable other suites.

#### Scenario: Old protocol refused
- **WHEN** a client offers only TLS 1.1
- **THEN** the handshake fails and no OpenWire command is processed

#### Scenario: TLS 1.3 only
- **WHEN** `min_version = "1.3"` and a client offers only TLS 1.2
- **THEN** the handshake fails

### Requirement: Client certificates
`tls.client_auth` SHALL accept `"none"` (default), `"optional"` and `"required"`, and `tls.client_ca` (PEM bundle of trusted CA certificates) SHALL be required with `"optional"` and `"required"`. With `"optional"` a client MAY present a certificate, which SHALL chain to `client_ca` or the handshake fails; with `"required"` a client without a valid certificate SHALL fail the handshake. Username and password authentication SHALL still apply after the handshake. The subject of a verified client certificate SHALL be shown for the connection in the admin console.

#### Scenario: Required and missing
- **WHEN** `client_auth = "required"` and a client connects without a certificate
- **THEN** the handshake fails and the failure is logged at debug level with the client IP

#### Scenario: Required and valid
- **WHEN** `client_auth = "required"` and a client presents a certificate signed by `client_ca` with valid credentials
- **THEN** the connection is accepted and the Connections page shows the certificate subject

#### Scenario: Certificate does not replace the password
- **WHEN** a client presents a valid certificate but a wrong password and anonymous access is off
- **THEN** the connection is refused as over plain TCP

### Requirement: Plain listener switch
`broker.plain` SHALL be a boolean, default `true`. With `false` the plain OpenWire listener SHALL NOT be opened and only the TLS listener SHALL accept clients. `broker.plain = false` with TLS disabled SHALL be a configuration error that names both keys and exits with code 2.

#### Scenario: TLS only
- **WHEN** `broker.plain = false` and TLS is enabled
- **THEN** nothing listens on `broker.port` and clients connect over `ssl://`

#### Scenario: No listener at all
- **WHEN** `broker.plain = false` and `tls.enabled = false`
- **THEN** startup fails with exit code 2 naming `broker.plain` and `tls.enabled`

### Requirement: Certificate checks
At startup and in `mqrust.exe check-config` the broker SHALL check that every configured TLS file is readable and well-formed, that the private key matches the leaf certificate and that `client_ca` holds at least one certificate; any failure SHALL be a configuration error naming the key and the file, exit code 2. A leaf certificate already expired, or expiring within 30 days, SHALL produce a warning with its expiry date, and SHALL NOT stop the broker.

#### Scenario: Key does not match
- **WHEN** `key` belongs to another certificate
- **THEN** `check-config` and startup fail with exit code 2 naming `tls.key`

#### Scenario: Expiring certificate
- **WHEN** the certificate expires in 10 days
- **THEN** the broker starts and logs a warning with the expiry date, and `check-config` reports the same warning with exit code 0

### Requirement: Handshake logging
A failed or timed-out handshake SHALL be logged at debug level with the client IP and the reason, and at most one warning per minute SHALL summarise the number of failed handshakes since the previous warning. No OpenWire data SHALL be processed before the handshake completes.

#### Scenario: Port scan
- **WHEN** 1000 connections to the TLS port fail the handshake within one minute
- **THEN** the log contains at most one warning for that minute with the count, and debug lines only when the level is debug or trace

### Requirement: TLS data in the admin console
The Connections page and `/api/connections` SHALL show for each connection its transport (`tcp` or `ssl`) and, for TLS connections, the negotiated TLS version and cipher suite, and the verified client certificate subject when present.

#### Scenario: Mixed connections
- **WHEN** one client is connected over `tcp://` and one over `ssl://` with TLS 1.3
- **THEN** the Connections page shows `tcp` for the first, and `ssl`, `TLS 1.3` and the cipher suite for the second

### Requirement: Deliberate differences from ActiveMQ
The URL scheme, default port and client configuration SHALL be the same as ActiveMQ's `ssl://` transport. These differences SHALL be intentional: JKS keystores are not read (PKCS#12 and PEM are); cipher suites and protocols below TLS 1.2 cannot be enabled; certificates are reloaded only by a restart; the admin console stays on HTTP.

#### Scenario: JKS conversion documented
- **WHEN** an operator reads the README TLS section
- **THEN** it shows how to convert a JKS keystore to PKCS#12 and how to import the broker certificate into a Java truststore
