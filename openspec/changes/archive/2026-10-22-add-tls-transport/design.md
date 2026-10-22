## Context

`src/server.rs` binds one `TcpListener` (`broker.bind`:`broker.port`) and spawns `connection::serve(TcpStream, …)` per accepted socket. `serve_with_codec` sets `TCP_NODELAY`, splits the socket with `TcpStream::into_split` (lock-free owned halves) and runs the OpenWire negotiation and the command loop. The project builds on Windows x86_64 and macOS ARM64 in CI, without OpenSSL. ActiveMQ configures TLS with `<sslContext>` (keystore/truststore, JKS or PKCS#12) and transport options such as `needClientAuth`, `wantClientAuth`, `transport.enabledProtocols`, `transport.enabledCipherSuites`.

## Goals / Non-Goals

**Goals:**
- `ssl://host:61617` from the ActiveMQ Java clients 5.19.x and 6.x works with no client change other than the truststore.
- Plain TCP performance unchanged; TLS cost limited to encryption.
- Safe defaults: modern protocol versions only, no weak suites, no way to enable them.
- Clear errors at startup and in `check-config` for every certificate problem.

**Non-Goals:**
- HTTPS for the admin console (separate change).
- Hot reload of certificates.
- Authentication by client certificate alone.
- `nio+ssl`, `stomp+ssl`, AMQP/MQTT/WebSocket transports.
- JKS keystores.

## Decisions

### D1. rustls with the ring provider
`rustls` + `tokio-rustls`, crypto provider `ring`. Pure Rust plus prebuilt assembly, builds on both CI targets with no extra tool, memory-safe parser, TLS 1.2/1.3 only.
*Alternative 1*: `native-tls` (SChannel on Windows, Secure Transport on macOS). Rejected: different behaviour and supported options per OS, PKCS#12 only on some, harder to test identically.
*Alternative 2*: OpenSSL. Rejected: a native dependency to ship and patch on Windows.
*Alternative 3*: `aws-lc-rs` provider. Rejected for now: needs CMake and NASM on Windows builds; can be switched later behind the same code.

### D2. Two listeners, one connection code path
The TLS listener is a second accept loop on `[tls] bind:port`. After `accept`, the handshake runs in the spawned task (never in the accept loop) with a 10-second timeout; then the stream goes to the same `serve_with_codec`, made generic over `AsyncRead + AsyncWrite + Unpin + Send`. TCP options (`TCP_NODELAY`, buffers) are set on the socket before the handshake. For TLS the stream is split with `tokio::io::split`; plain TCP keeps `into_split`, so its hot path is unchanged.
*Alternative*: a single port that sniffs plain or TLS on the first byte. Rejected: ActiveMQ uses separate ports and URLs; sniffing is surprising and weakens "TLS only" setups.

### D3. Certificate formats: PEM and PKCS#12
Either `cert` (PEM chain, leaf first) + `key` (PEM, PKCS#8, PKCS#1 or SEC1, unencrypted), or `keystore` (PKCS#12) + `keystore_password`, exactly one of the two forms. PKCS#12 with exactly one private key entry is used; more than one is an error naming the file. Relative paths resolve against the folder of the configuration file, so certificates live next to `mqrust.toml`.
*Alternative*: PEM only. Rejected: operators coming from ActiveMQ have PKCS#12 keystores from `keytool`; PKCS#12 is also what Windows exports (`.pfx`).

### D4. Versions and suites
`min_version = "1.2" | "1.3"`, default `"1.2"`; TLS 1.0/1.1 and SSLv3 are not supported by rustls at all. Cipher suites are rustls's defaults (AEAD only, forward secrecy); no configuration key. ALPN is not used (OpenWire has no ALPN id).
*Alternative*: `enabled_cipher_suites` like ActiveMQ. Rejected: every suite rustls offers is already safe; a list of names only adds ways to misconfigure.

### D5. Client certificates
`client_auth = "none"` (default), `"optional"` (ActiveMQ `wantClientAuth`) or `"required"` (`needClientAuth`), with `client_ca` (PEM bundle) required for the last two. A presented certificate must chain to `client_ca`; with `"required"` a missing or invalid certificate fails the handshake. The usual username/password authentication still runs after the handshake. The subject of a verified client certificate is shown in the admin Connections page.
*Alternative*: map the certificate subject to a user. Rejected for this change: it needs its own user configuration; noted for later.

### D6. Plain listener switch
`[broker] plain = true` by default. With `false`, only the TLS listener is opened; `false` with TLS disabled is a configuration error (no listener at all).
*Alternative*: `port = 0` to disable plain. Rejected: `0` already means "invalid" in validation and reads as "any port" to many users.

### D7. Startup checks and certificate lifetime
At startup and in `check-config`: files readable, PEM/PKCS#12 parsed, key matches the leaf certificate, `client_ca` parsed. Any failure is a configuration error naming the key and file, exit code 2. A leaf certificate already expired or expiring within 30 days is a warning (startup log and `check-config`), not an error, so an expiring certificate never stops a broker restart.
*Alternative*: refuse expired certificates. Rejected: clients decide what they trust; the broker reports clearly.

### D8. Logging and admin data
Startup logs `TLS listening on <bind>:<port> (TLS 1.2+, client certificates: <mode>)`. A failed or timed-out handshake is logged at `debug` with IP and reason, plus one `warn` per minute summarising the count, so a port scan does not flood the log. The Connections page and `/api/connections` gain `transport` (`tcp` or `ssl`), `tlsVersion` and `tlsCipher` (TLS only) and `clientCertificate` (subject, when verified).
*Alternative*: warn for every failed handshake. Rejected: trivially floods the log.

## Risks / Trade-offs

- [`tokio::io::split` adds a small lock per read/write for TLS] → only on TLS connections; measured in the TLS benchmark when the user approves a load run.
- [Private key and keystore password in files] → documented: restrict access to the configuration folder; the password is never logged.
- [Java truststore setup is the most common failure] → the README shows the `keytool` commands to import the broker certificate and the client URL `ssl://host:61617`; the handshake log names the reason (`unknown CA`, `bad certificate`).
- [Old Java clients that only speak TLS 1.0/1.1] → not supported; JDK 8u261+ and every JDK 11+ speak TLS 1.2/1.3.

## Migration Plan

Nothing changes until `[tls] enabled = true`. To move clients: enable TLS next to the plain listener, switch clients to `ssl://…:61617` (or `failover:(ssl://…)`), then set `broker.plain = false`. Rollback: `enabled = false`.

## Open Questions

None.
