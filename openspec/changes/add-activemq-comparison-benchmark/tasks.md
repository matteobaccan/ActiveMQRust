## 1. XML message generator

- [x] 1.1 Implement the XML document generator in `tests/java-it` (`id`, `field01`–`field20` with fixed kinds, base64 payload of random bytes, 0–3 trailing spaces) from a fixed-seed `SplittableRandom`
- [x] 1.2 Implement exact sizing for 1,024, 10,240 and 51,200 bytes and the pre-send size check that aborts the run
- [x] 1.3 Unit tests: exact UTF-8 length for all three sizes over 10,000 documents each, well-formedness with StAX, field kinds, valid base64, same seed gives identical sets

## 2. Bench mode extensions

- [x] 2.1 Pre-generate the full message set before timing; run the client with `-Xmx3g`
- [x] 2.2 Implement the `hold` workload: produce with no consumer, synchronous last send through a second producer with send timeout, hold window, consume phase, `PHASE` and `RESULT` lines with ms, msgs/s and MB/s per phase
- [x] 2.3 Switch the 1 KB `throughput` scenario to XML payloads and report produce time, consume time, msgs/s and MB/s for async and sync send
- [x] 2.4 Implement the in-loop O(1) checks (text length, consecutive `seq`) and the after-timing sample verification (every 100th plus first and last: StAX structure, base64, equality with the regenerated document)
- [x] 2.5 Add the `Deflater` level 1 ratio on the sample to the `RESULT` line
- [x] 2.6 Add the 20,000-message warm-up queue phase with its own seed
- [x] 2.7 Validate the `hold` and `throughput` workloads against a real ActiveMQ 6.x by hand

## 3. Broker configurations

- [x] 3.1 Write `scripts\activemq-bench\activemq-tuned.xml` and the JVM argument list for 5.18.x and 6.x
- [x] 3.2 Write `scripts\activemq-bench\mqrust-bench.toml` (benchmark user only) and `mqrust-bench-nocompress.toml` (plus `compress_threshold_kb = 0`)
- [x] 3.3 Check that tuned ActiveMQ holds 100,000 × 10 KB and 10,000 × 50 KB with no spooling and no flow-control log lines (verified by the RAM-only log check of `soak-compare.ps1` in the full-speed runs, which held up to 7.7 GB of queued messages without such lines)

## 4. Comparison script

- [x] 4.1 Create `scripts\compare-activemq.ps1` with parameters (paths of `mqrust.exe`, ActiveMQ 5.18.x and 6.x, JDK; runs; skip switches) and input validation
- [x] 4.2 Implement pre-run checks (port 61616 free, at least 8 GB free RAM, CPU usage warning above 20%)
- [x] 4.3 Implement broker start and stop for ActiveMQRust and for ActiveMQ (`java.exe` launched directly, tuned and default configurations, fresh data directory per run)
- [x] 4.4 Implement 250 ms memory sampling (Working Set, Private Bytes) and phase assignment from the `PHASE` lines; compute peaks and 5 s steady medians
- [x] 4.5 Implement the run loop: per broker setup and measurement, warm-up run plus N measured runs, overall run timeout, invalid-run detection (spooling, memory-limit log lines) with one retry
- [x] 4.6 Run scenario (e) twice for ActiveMQRust (compression active and off); read message memory from `/api/overview` at the end of the hold window; skip the active run when compression or the admin console is unavailable
- [x] 4.7 Collect machine details (CPU, cores, RAM, Windows edition, version and build, power plan, JDK, broker versions)

## 5. Report generation

- [x] 5.1 Write the CSV with one row per run and all required columns, including `broker_compression`
- [x] 5.2 Write the Markdown report: machine details, message format, configurations and JVM command lines, median tables with ratios, compression labels and ratios, per-message overhead for (b) and (e), reference table for default configurations, invalid-run notes
- [x] 5.3 Compute the success criteria and §14.3 target verdicts in the script, with values next to each verdict
- [x] 5.4 Add the README section that explains how to run the comparison and links the latest report

## 6. Verification

- [ ] 6.1 A full run of `scripts\compare-activemq.ps1` on the development machine completes unattended and writes the Markdown report and the CSV in its output folder; the results are copied into the README
- [ ] 6.2 The report contains every measurement (a)–(e) for both ActiveMQ versions, the compression labels and ratio, and a met / not met verdict for every criterion and target, stated as measured
- [ ] 6.3 Every target not met has a follow-up task
- [x] 6.4 `openspec validate add-activemq-comparison-benchmark` passes

## 7. Resource usage under load

- [x] 7.1 Record per phase the broker and client CPU time, average and peak Working Set and Private Bytes, and the machine CPU average; derive CPU % of one core and of the machine, CPU ms per 1,000 messages and per MB, and memory per held message
- [x] 7.2 Add the resource figures to the report tables (mean/median/min/max) and every individual value to the CSV

## 8. Soak scenario

- [x] 8.1 Add the `soak` scenario to Bench.java: steady per-producer rate for a duration, shared or per-pair queues, checks for order, duplicates, losses and length, latency percentiles
- [x] 8.2 Add `scripts/soak-compare.ps1` to run the soak scenario against every broker (startup, latency, broker and client CPU, broker memory), run it and publish the report in `docs/benchmarks/`

## 9. Same memory ceiling as ActiveMQ (0.4.0)

- [ ] 9.1 Re-run the ActiveMQRust full-speed runs (15 KB, 50 KB, 300 KB, 20 + 20 clients, 60 s) with `max_memory_mb = 4096` (and 8192), the same ceiling as the ActiveMQ heap, and report delivered, rejected and discarded messages next to the ActiveMQ results in the README
- [ ] 9.2 Use Apache ActiveMQ 5.19.11 (latest 5.x, same version as the test client) instead of 5.18.7 as the 5.x reference; re-run every ActiveMQ 5 load test (soak 1 KB and 10 KB, full speed 15/50/300 KB, client compression, 8 GB heap) and update the README
- [ ] 9.3 Publish in the README the "4 GB fixed for every broker" run (15 KB, 20 + 20 clients, full speed, 60 s; ActiveMQ `-Xms4g -Xmx4g -XX:+AlwaysPreTouch`, ActiveMQRust `MIMALLOC_RESERVE_OS_MEMORY=4GiB` and `max_memory_mb = 4096`). First run on 2026-10-06: ActiveMQRust ok 1,383,663/1,383,663, 337 MB/s, 36 ms CPU per 1,000 messages, peak 1.0 GB; ActiveMQ 5.18.7 failed out of memory 502,311/769,645; ActiveMQ 6.3.2 failed out of memory 80,808/348,092 — repeat with 5.19.11
