## 1. Dependencies and configuration

- [ ] 1.1 Add `rustls` (ring provider), `tokio-rustls`, `rustls-pemfile`, `p12-keystore`; check that both CI targets build
- [ ] 1.2 `[tls]` keys (`enabled`, `bind`, `port`, `cert`, `key`, `keystore`, `keystore_password`, `min_version`, `client_auth`, `client_ca`) and `broker.plain`, with validation errors naming the keys
- [ ] 1.3 Template entries, commented, with defaults

## 2. Certificates

- [ ] 2.1 `src/tls.rs`: load PEM chain and key, or PKCS#12 with one key entry; detect and refuse JKS with the conversion hint
- [ ] 2.2 Key/certificate match, `client_ca` parsing, expiry warning within 30 days
- [ ] 2.3 Build the rustls `ServerConfig`: versions from `min_version`, client verifier from `client_auth`
- [ ] 2.4 `check-config` runs the same checks
- [ ] 2.5 Unit tests with generated test certificates (PEM, PKCS#12, mismatched key, expired, JKS sample)

## 3. Listener and connections

- [ ] 3.1 Make `serve_with_codec` generic over the stream; keep `into_split` for TCP, `tokio::io::split` for TLS
- [ ] 3.2 TLS accept loop with the handshake in the spawned task and a 10-second timeout
- [ ] 3.3 `broker.plain = false`: no plain listener
- [ ] 3.4 Handshake failure logging: debug per failure, one warning per minute with the count
- [ ] 3.5 Record transport, TLS version, cipher suite and client certificate subject on the connection

## 4. Admin console

- [ ] 4.1 Connections page and `/api/connections`: `transport`, `tlsVersion`, `tlsCipher`, `clientCertificate`

## 5. Tests

- [ ] 5.1 Rust integration tests: TLS connection end to end, TLS 1.3 only, required client certificate missing/valid, wrong password with valid certificate, slow handshake, TLS-only mode
- [ ] 5.2 Java integration tests (`tests/java-it`): `ssl://` and `failover:(ssl://)` profiles with clients 5.19.11 and 6.3.2
- [ ] 5.3 Check that plain TCP tests and results are unchanged; ask the user before any TLS load run

## 6. Documentation

- [ ] 6.1 README TLS section: configuration, PEM and PKCS#12, `keytool` commands (create keystore, JKS → PKCS#12, import into a Java truststore), client URLs, TLS-only migration, differences from ActiveMQ; test results after the run
- [ ] 6.2 CHANGELOG entry under Unreleased
- [ ] 6.3 `cargo fmt` and the full test suite
