## Why

The reason to adopt ActiveMQRust is that it is **compatible** with ActiveMQ, **uses less RAM** and is **faster** for in-RAM message processing without storage. The project goals and the §14.3 target (throughput ≥ ActiveMQ, memory ≤ 1/5) are claims until they are measured against a real ActiveMQ on the same machine, with the same client, the same settings and the same messages. This change adds a repeatable, fair, scripted comparison and publishes the results, including any target that is missed.

## What Changes

- The `bench` mode of the Java program in `tests/java-it` (from `optimize-broker-performance`) gains a `hold` workload: produce N messages with no consumer, hold them in the broker, then consume them, timing the produce and consume phases separately and reporting msgs/s and MB/s. The same jar and the same settings are used against both brokers.
- Every benchmark message is a JMS `TextMessage` holding an XML document with an `<id>`, 20 fields `field01` … `field20` with random values of mixed kinds, and a base64 buffer of random bytes that fills the document to the exact target size (1,024, 10,240 or 51,200 bytes). Messages are generated from a fixed seed before timing starts, so both brokers receive identical message sets.
- Measurements: (a) idle memory after startup; (b) memory while holding 100,000 messages of 10 KB; (c) produce and consume time for those messages; (d) 1 KB throughput with async and sync send; (e) memory, produce time and consume time while holding 10,000 messages of 50 KB.
- Scenario (e) is above the 32 KB broker compression threshold, so ActiveMQRust runs twice: with default settings (broker compression active) and with `compress_threshold_kb = 0` (like-for-like). The report labels which runs had broker compression active and states the compression ratio achieved.
- Memory sampling of the broker process on Windows (Working Set and Private Bytes, peak and steady values; `java.exe` for ActiveMQ).
- A fair ActiveMQ setup for 5.18.x and 6.x: persistence disabled, all messages kept in memory with no producer flow control and no spooling to temporary storage, documented XML and JVM flags; plus a secondary run with ActiveMQ's default configuration.
- `scripts\compare-activemq.ps1`, which starts each broker, samples memory, runs the bench, stops the broker, repeats (warm-up plus 3 measured runs, broker restarted for each) and writes a Markdown report and a CSV file in `docs/benchmarks/`.
- Success criteria evaluated in the report: ActiveMQRust uses less memory in (a) and (b) and is faster in (c); the §14.3 targets are reported as met or not met.

## Capabilities

### New Capabilities

- `activemq-comparison-benchmark`: comparison workloads, XML message generation and verification, fair broker setups, memory sampling, run procedure, the comparison script, the report and CSV, and the success criteria.

### Modified Capabilities

None.

## Impact

- `tests/java-it/`: `hold` workload, XML message generator and verifier, and `PHASE` / `RESULT` output in the `bench` mode of `mqrust-acceptance.jar`.
- New `scripts\compare-activemq.ps1` and `scripts\activemq-bench\` (ActiveMQ XML configurations, JVM flags, the two ActiveMQRust configuration files used for the runs).
- New output in `docs/benchmarks/`: `activemq-comparison-<yyyy-MM-dd>.md` and `.csv`; a README section linking the latest report.
- External tools on the benchmark machine only: JDK 17+ (the development machine has JDK 21), ActiveMQ 5.18.x and 6.x distributions (downloaded by the user, not committed), PowerShell 7, at least 16 GB of RAM. No change to `mqrust.exe` or its dependencies.
- Depends on `add-queue-messaging` (queues) and `optimize-broker-performance` (the `bench` mode and the tuned hot path). The compression-active run of scenario (e) needs `add-message-compression`; the measured compression ratio is read from the `add-admin-console` JSON API.
