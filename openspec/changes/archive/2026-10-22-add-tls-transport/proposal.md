## Why

OpenWire travels in clear text on port 61616: usernames, passwords and message bodies can be read by anyone on the network path. ActiveMQ offers the `ssl://` transport (by convention on port 61617) so Java clients connect with `ssl://host:61617` or `failover:(ssl://…)` and a truststore. Applications moving from ActiveMQ expect the same URL to work, and many environments require encrypted connections for any credential.

## What Changes

- A TLS listener for OpenWire, configured in a new `[tls]` section: off by default, `bind` (default as `broker.bind`), `port` (default `61617`). Java clients connect with `ssl://host:61617`, unchanged from ActiveMQ.
- Server certificate as PEM (certificate chain + private key) or PKCS#12 (`.p12`/`.pfx`, the format `keytool -storetype PKCS12` writes) with its password. JKS is not read: it is converted once with `keytool -importkeystore`.
- TLS 1.2 and 1.3 (`min_version`, default `1.2`); no SSLv3, TLS 1.0 or 1.1. Cipher suites: the safe defaults of the TLS library, no option to add weak ones.
- Client certificates (mutual TLS): `client_auth = "none" | "optional" | "required"` with a CA bundle, like ActiveMQ's `needClientAuth`/`wantClientAuth`. A client certificate does not replace the username/password check.
- The plain listener stays, and can be turned off with `broker.plain = false` to accept TLS only.
- Handshake timeout, logging of failed handshakes, transport and TLS version per connection in the admin Connections page and API.
- `check-config` loads the certificate and key, checks that they match, and warns about certificates expired or expiring within 30 days.
- The OpenWire protocol and every broker semantic are unchanged: TLS only wraps the byte stream.

## Capabilities

### New Capabilities

- `tls-transport`: `[tls]` configuration, certificate formats, protocol versions, client certificates, plain listener switch, handshake rules, logging, admin data, `check-config` checks and Java client compatibility.

### Modified Capabilities

None. Connections page columns are extended through the new capability; existing requirements are unchanged.

## Impact

- New crates: `rustls` and `tokio-rustls` (TLS, with the `ring` crypto provider, which builds on Windows and macOS without extra tools), `rustls-pemfile` (PEM), `p12-keystore` (PKCS#12). All pure Rust or with prebuilt assembly, no OpenSSL.
- Code: `src/server.rs` (second listener, TLS accept with timeout, plain switch), `src/connection.rs` (`serve` generic over the stream type instead of `TcpStream`), new `src/tls.rs` (loading and validating certificates, server configuration), `src/config.rs` (`[tls]`, `broker.plain`), `src/admin` (transport column and fields), template, README, Java integration tests (`tests/java-it`) with an `ssl://` profile.
- Performance: TLS adds CPU per byte; benchmarks of the plain listener must not change. A TLS benchmark is added to the README only after the user approves a load run.
- Out of scope: HTTPS for the admin console, certificate hot reload (a restart applies a new certificate), mapping client certificates to users (ActiveMQ's `JaasCertificateAuthenticationPlugin`), other ActiveMQ transports (`nio+ssl`, `stomp+ssl`, AMQP, MQTT, WebSocket).
