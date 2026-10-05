## 1. Default

- [x] 1.1 Set the `compress_threshold_kb` default to 0 in `src/config.rs` and describe the trade-off in the `init-config` template; regenerate `mqrust.example.toml`
- [x] 1.2 Set `compress_threshold_kb = 32` explicitly in `scripts/activemq-bench/mqrust-bench.toml` so the comparison keeps measuring broker compression

## 2. Tests

- [x] 2.1 Rust tests that exercise broker compression set the threshold explicitly; default-configuration tests assert that nothing is compressed
- [x] 2.2 Java `Integration.brokerCompression` and `CompressionChecks` take the threshold as an option (default 0 = verify that nothing is compressed by the broker; >0 = boundary checks at that threshold)
- [x] 2.3 Full Rust suite and Java suites (amq5 and amq6) pass, including the compression checks against a broker started with `compress_threshold_kb = 32`

## 3. Documentation

- [x] 3.1 README: features list, configuration and load-test results explain the default and the measured trade-off
- [x] 3.2 `openspec validate disable-broker-compression-by-default` passes
