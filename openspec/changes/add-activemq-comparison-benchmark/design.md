## Context

`optimize-broker-performance` provides a `bench` mode in the Java program of `tests/java-it` (throughput, latency and scale scenarios with the real `activemq-client`) and publishes ActiveMQRust's own numbers. It does not compare against ActiveMQ. The project claims to use less RAM and be faster than ActiveMQ for in-RAM processing without storage, and §14.3 sets the target "throughput ≥ ActiveMQ, memory ≤ 1/5". This change is a new user requirement, not in the original design document: prove or disprove those claims with a fair, repeatable comparison on the same Windows machine, with the same client, the same settings and the same messages.

Fairness is the main design concern. ActiveMQ by default persists messages to KahaDB, applies producer flow control and spools non-persistent messages to temporary storage when memory fills up. Comparing that with a RAM-only broker would measure storage, not the brokers. The primary comparison therefore configures ActiveMQ to also keep everything in RAM with no storage, and reports the default configuration only as a secondary reference. A second fairness concern is broker-side compression: ActiveMQRust compresses bodies above 32 KB by default and ActiveMQ does not, so any scenario above that size must separate the effect of compression from the effect of the broker design.

## Goals / Non-Goals

**Goals:**
- One script that runs the whole comparison unattended and writes a Markdown report and a CSV file.
- The same Java client binary, settings and message sets against both brokers, so differences come from the brokers.
- Realistic messages: XML text documents, as commonly exchanged between Java applications.
- An ActiveMQ setup that is as favourable to ActiveMQ as the scenario allows, documented exactly.
- Honest reporting: every criterion and target shown as met or not met with the measured values, and every run labelled with whether broker compression was active.

**Non-Goals:**
- Persistent messaging, durable subscriptions, failover or network-of-brokers comparisons.
- Comparison with ActiveMQ Artemis or other brokers.
- Running in CI: the comparison needs ActiveMQ distributions and a quiet machine for a long time.
- Tuning ActiveMQRust specially for the comparison: apart from the like-for-like run of scenario (e), it runs with its default settings.

## Decisions

### D1. Same client jar, paired by driver version
The comparison uses `mqrust-acceptance.jar` in `bench` mode for both brokers. ActiveMQ 5.18.x is compared with ActiveMQRust using the `amq5` build of the jar (activemq-client 5.18.x); ActiveMQ 6.x with ActiveMQRust using the `amq6` build (activemq-client 6.x). Both brokers listen on `127.0.0.1:<port>`, one at a time (the script's `-Port`, default 61616; the admin or web console port is `-AdminPort`, default 8161), and the client connects to `tcp://127.0.0.1:<port>` with the same URL options (`jms.prefetchPolicy.queuePrefetch=1000`, `jms.useCompression=false`, async or sync send as the workload requires) and the same credentials, `admin` / `admin`, used for both brokers in the benchmark only. Messages are `NON_PERSISTENT` and sessions `AUTO_ACKNOWLEDGE`.
- *Alternatives:* a separate benchmark tool such as ActiveMQ's `activemq-perf` plugin (Maven-only, different statistics, not usable unchanged against both brokers); a Rust load generator (would not prove anything about the real Java driver).

### D2. XML TextMessage payloads with an exact size
Every benchmark message is a `TextMessage` whose text is:

```xml
<message><id>42</id><field01>k3J9aQ…</field01>…<field20>true</field20><payload encoding="base64">q83Z…</payload></message>
```

- `<id>` is the message's sequence number in the run (also set as the int property `seq`).
- `field01`–`field20` have fixed names and fixed kinds by position: `field01`–`field04` alphanumeric strings of 8–24 characters; `field05`–`field08` integers (`long` range); `field09`–`field12` decimals with 2–6 fractional digits; `field13`–`field16` ISO-8601 UTC timestamps with milliseconds (`2026-10-05T14:30:00.123Z`); `field17`–`field20` booleans. All values are ASCII and need no XML escaping.
- `<payload encoding="base64">` holds standard base64 (with padding) of random bytes. Its length is the largest multiple of 4 that fits; 0 to 3 space characters before `</message>` (insignificant XML whitespace) make up the difference, so the UTF-8 length of the document equals the target exactly: 1,024, 10,240, 12,288 (measurement (f)) or 51,200 bytes. Because every character is ASCII, the UTF-8 length equals the character count and the Java modified-UTF-8 length used on the wire.
- All values and random bytes come from one `java.util.SplittableRandom` with a fixed seed per scenario (a different fixed seed for warm-up messages). Both brokers therefore receive identical message sets.
- The whole set is generated, and each document's UTF-8 length is checked, before the timed section starts; a wrong length aborts the run. The client JVM runs with `-Xmx3g` so 100,000 × 10 KB pre-generated documents fit (Java stores ASCII strings with one byte per character).
- *Alternatives:* random `BytesMessage` plus plain text (the previous plan; less representative of real traffic); generating messages inside the send loop (generation cost would be counted as produce time); a single document reused for every message (identical bodies are unrealistic and could favour any future deduplication).

### D3. `hold` workload with separately timed phases
A `hold` run creates a new queue, produces N messages with no consumer attached, waits a hold window of 10 s, then consumes them. Produce time runs from the first `send` to the moment the broker has confirmed all N messages. With async send the client returns before the broker has the message, so the last message is sent through a second producer on the same session with a send timeout (`ActiveMQMessageProducer.setSendTimeout(120000)`, long enough for a broker that is still writing 100,000 earlier messages), which makes that one send synchronous; because the session uses one TCP connection, its `Response` proves the broker has processed every earlier message. All messages stay `NON_PERSISTENT`. Consume time runs from the creation of the consumer to the receipt of message N.
- *Alternatives:* send every message synchronously (fair, but measures round trips rather than broker throughput and is already covered by the 1 KB sync run); send the last message `PERSISTENT` to force a synchronous send (changes the delivery mode of one message and, in the default ActiveMQ configuration, writes it to KahaDB); stop the clock at the last async `send` call (measures the client's socket buffer, not the broker).

### D4. Consumer verification: cheap checks timed, full checks on a sample after timing
Inside the timed consume loop the client checks, for every message, that `getText().length()` equals the target size and that `seq` is exactly one more than the previous one (FIFO, nothing lost). Both are O(1) on a Java `String` and an int property, so their cost is negligible. The client keeps references to a 1% sample (every 100th message, plus the first and the last). After the consume timer stops, it parses each sampled document with StAX, checks the element structure (`id`, `field01`–`field20`, `payload`), checks that the payload is valid base64, and compares the text with the document generated from the seed for that `seq`. Any failure marks the run failed.
- *Alternatives:* parse every message inside the timed section (XML parsing would dominate consume time and hide broker differences); keep every message and verify all after timing (doubles client memory to about 2 GB for 100,000 × 10 KB); no content check (a broker that corrupts bodies could look fast).

### D5. Memory sampled from outside the process
`compare-activemq.ps1` samples the broker process every 250 ms with `Get-Process -Id <pid>`, recording `WorkingSet64` (Working Set) and `PrivateMemorySize64` (Private Bytes) with a timestamp. For ActiveMQ the process is the `java.exe` started by the script. The client prints timestamped `PHASE` lines (`warmup-start`, `warmup-end`, `produce-start`, `produce-end`, `hold-end`, `consume-start`, `consume-end`), and the script assigns samples to phases. Peak is the highest sample over the run. Steady values are medians over a 5 s window: idle steady from the last 5 s of the 10 s after the broker accepts connections, hold steady from the last 5 s of the hold window. No garbage collection is forced in ActiveMQ.
- *Alternatives:* in-process measurement (JMX for Java, the admin API for ActiveMQRust: two different methods, not comparable); `typeperf` performance counters (comparable, but more parsing and locale-dependent counter names); forcing a GC with `jcmd GC.run` before sampling (does not represent normal operation and barely changes committed memory).

### D6. Fair ActiveMQ "tuned" configuration
The primary ActiveMQ runs use `scripts\activemq-bench\activemq-tuned.xml`:

```xml
<broker xmlns="http://activemq.apache.org/schema/core" brokerName="bench"
        persistent="false" useJmx="false" advisorySupport="false" schedulerSupport="false"
        dataDirectory="${activemq.data}">
  <destinationPolicy><policyMap><policyEntries>
    <policyEntry queue=">" producerFlowControl="false">
      <pendingQueuePolicy><vmQueueCursor/></pendingQueuePolicy>
    </policyEntry>
    <policyEntry topic=">" producerFlowControl="false"/>
  </policyEntries></policyMap></destinationPolicy>
  <systemUsage><systemUsage>
    <memoryUsage><memoryUsage limit="3 gb"/></memoryUsage>
    <storeUsage><storeUsage limit="3 gb"/></storeUsage>
    <tempUsage><tempUsage limit="3 gb"/></tempUsage>
  </systemUsage></systemUsage>
  <plugins><simpleAuthenticationPlugin><users>
    <authenticationUser username="admin" password="admin" groups="admins,users"/>
  </users></simpleAuthenticationPlugin></plugins>
  <transportConnectors>
    <transportConnector name="openwire"
        uri="tcp://127.0.0.1:61616?maximumConnections=1000&amp;wireFormat.maxFrameSize=104857600"/>
  </transportConnectors>
</broker>
```

The file is a template for the port: the script writes a copy per run with `127.0.0.1:61616` replaced by `127.0.0.1:<port>`. `persistent="false"` removes KahaDB; the `vmQueueCursor` keeps every message in memory and never spools to temporary storage; the broker-wide 3 GB memory limit (there is no per-queue limit, so a queue may use all of it) holds 100,000 × 10 KB (about 1 GB of payload) or 10,000 × 50 KB (about 0.5 GB) with room for overhead, so producer flow control is never triggered (and is disabled as well, for queues and topics). The store and temp limits are set to 3 GB only so that ActiveMQ does not warn about the free disk space at start-up; with no persistence and the VM cursor they are not used. The connector options (`maximumConnections`, `wireFormat.maxFrameSize`) are those of the distribution's own `activemq.xml`. The benchmark user is `admin` / `admin`, the same as for ActiveMQRust. JMX, advisories, the scheduler, the web console and the other transport connectors are off, because ActiveMQRust has none of them on the OpenWire path; this reduces ActiveMQ's memory and CPU, which favours ActiveMQ. The JVM flags are `-Xmx4g` with no `-Xms`, so the heap grows only as needed (also favourable to ActiveMQ's memory figures), the default G1 collector, and the same JDK as the client. The script launches `java.exe` directly with the arguments `activemq.bat` would use (`-Dactivemq.home`, `-Dactivemq.base`, `-Dactivemq.conf`, `-Dactivemq.data` pointing to a fresh temporary directory, `-Djava.io.tmpdir`, `-jar bin\activemq.jar start xbean:file:<config>`), so it knows the PID of the process to sample.
- *Alternatives:* ActiveMQ defaults only (measures KahaDB and spooling, not in-RAM processing); `-Xms4g -Xmx4g` (common in production, but makes ActiveMQ's memory look worse than necessary); keeping JMX and advisories on (realistic, but unfavourable to ActiveMQ and not like-for-like).

### D7. ActiveMQ default configuration as a secondary reference
Each ActiveMQ version is also run with its `conf\activemq.xml` and the default JVM memory options of `activemq.bat` (read from the file; `-Xms1G -Xmx1G` in 5.18.7 and 6.3.2). The script copies the distribution's `conf` directory for each run and changes only the listening ports (OpenWire to `-Port`, the web console to `-AdminPort`, the other transports to the four ports after `-Port`), so the reference run can use the same ports as the others; the distribution itself is never modified. These results appear in a separate table labelled "default configuration, reference only" and are not used to decide the success criteria. A default run that blocks on flow control or spools to disk is reported as such.
- *Alternative:* omit default runs (hides what users experience out of the box).

### D8. ActiveMQRust configurations and broker compression
ActiveMQRust runs with its defaults plus the benchmark user, from `scripts\activemq-bench\mqrust-bench.toml` (`[[users]] username = "admin"`, `password = "admin"`, the same admin console credentials, and `bind = "127.0.0.1"`; everything else default: `compress_threshold_kb = 32`, `max_memory_mb = 0`, log level `info`). The script passes `--port` and `--admin-port` on the command line. The 1 KB and 10 KB documents are below the 32 KB threshold (the 10 KB `content` is 10,244 bytes with its length prefix), so broker compression never runs in scenarios (b), (c) and (d). The 50 KB documents (`content` 51,204 bytes) are above it, so scenario (e) runs ActiveMQRust twice: "compression on" with the default file, and "compression off" with `scripts\activemq-bench\mqrust-bench-nocompress.toml`, which adds `compress_threshold_kb = 0`. Base64 of random bytes carries 6 bits of information per 8-bit character, so deflate level 1 is expected to save roughly a quarter, above the 10% minimum saving, but the actual ratio is measured, not assumed. The like-for-like comparison with ActiveMQ uses the "compression off" run; the "compression on" run shows the product as shipped. Every row of the report and the CSV carries a `broker_compression` value (`active` or `off`; always `off` for ActiveMQ). For ActiveMQRust the value is not inferred from the message size or the configuration: the script reads the broker's compressed-message count from the admin JSON API once, shortly before the end of the hold window (a broker-wide counter in `/api/overview` when the broker has one, otherwise the `compressed` flag of the first page, up to 50 messages, of the held queue), and labels the run `active` only if that count is above 0. Throughput runs hold no messages; without a broker-wide counter their label is `unknown`.
- *Alternatives:* only the default run (would mix a feature effect with the broker comparison); only the compression-off run (would hide a real advantage of the shipped product); turning off the admin console or logging (would not represent the shipped product).

### D9. Measuring the achieved compression ratio
The achieved ratio is the ActiveMQRust message memory in use (from `/api/overview` of the admin console, read by the script at the end of the hold window) in the "compression on" run divided by the same value in the "compression off" run, as medians over the measured runs. As a cross-check the client also compresses its 1% sample with `java.util.zip.Deflater` level 1 after timing and reports that ratio (`deflate_ratio` in the `RESULT` line). If the admin console is not present, or the "compression on" run was not labelled `active` from the broker's own count, the script reports the ratio as "not available".
- *Alternatives:* compare Working Set only (includes allocator and runtime noise); sum compressed sizes from the messages API (50 per call, 200 calls per run, and it disturbs the measurement).

### D10. Run procedure: fresh broker per run, warm-up inside and outside
For each broker setup and workload, the script performs one warm-up run, which is discarded, and 3 measured runs, and reports the median. The broker is started fresh for every run (also for the warm-up run) and stopped afterwards, with a new data directory each time. Inside each run, after the idle sample, the client first exchanges 20,000 messages of the same size on a separate warm-up queue (excluded from timing; 1,000 distinct documents from the warm-up seed are generated and sent in cycles, which keeps generation time and client memory small), so the JVM of ActiveMQ and the client have compiled their hot paths before the measured phase; this favours ActiveMQ, whose JIT would otherwise be measured cold.
- *Alternatives:* keep one broker for all runs (earlier runs leave heap and caches behind and skew memory); no in-run warm-up (penalizes the JVM broker unfairly).

### D11. Report and CSV produced by the script
The script writes `docs\benchmarks\activemq-comparison-<yyyy-MM-dd>.csv` with one row per run (including warm-up runs, flagged) and `activemq-comparison-<yyyy-MM-dd>.md` with machine details, exact configurations and JVM flags, the message format, tables with every measured value plus mean, median, min and max and the ActiveMQRust / ActiveMQ ratio, start-up times, the compression labels and ratio, the criteria verdicts and notes on invalid runs. The report is generated, then committed by the user; it is never edited by hand except for a free-text "observations" section.
- *Alternative:* hand-written reports (error-prone, and invites selective reporting).

### D12. How criteria and targets are judged
The project success criteria compare medians against the tuned ActiveMQ setup of each version: memory in (a) and (b) lower for both Working Set and Private Bytes, and produce and consume time in (c) shorter. For (e) the same comparisons are reported with the "compression off" run as the like-for-like verdict and the "compression on" run as a separate, labelled verdict. The §14.3 targets are evaluated separately: "memory ≤ 1/5" on the steady Working Set in (a), (b) and (e), and "throughput ≥ ActiveMQ" on msgs/s in (c), (d) and (e). In (b) and (e) the payload itself must be in RAM in both brokers, which limits the possible ratio; the report shows the per-message overhead above payload (`(steady hold − idle − payload bytes) / N`) to explain the result, but the verdict uses the totals. A verdict is never adjusted by hand.
- *Alternative:* judge (b) and (e) on overhead only (more flattering, but not what the target says).

## Risks / Trade-offs

- [ActiveMQ silently spools or blocks, making its numbers worse for a configuration reason] → After each tuned run the script checks that the temporary storage directory is empty and the broker log has no memory-limit messages; otherwise the run is marked invalid and repeated (at most twice), then reported.
- [Broker compression is mistaken for a design advantage] → Every row carries `broker_compression`; the like-for-like verdict for (e) uses the compression-off run; the achieved ratio is printed next to the results.
- [Client-side generation or verification distorts timings] → Generation and full verification happen outside the timed sections; only O(1) length and sequence checks are timed, identically for both brokers.
- [Machine noise from other processes, power saving or thermal throttling] → The script records the power plan; before anything is built or started it measures the machine's CPU load over 5 s and stops if it is above 20% (`-Force` turns this into a warning); during every run it measures the CPU used by processes other than the broker, the client and the script itself (from `GetSystemTimes` minus the processor time of those three) and marks the run invalid and repeats it, at most twice, when that is above 20%; it uses the median of 3 runs and reports every run.
- [Not enough RAM for the held messages plus the pre-generated set in the client] → The script checks free physical memory before anything starts and before each run (at least 8 GB) and stops with a clear message (`-Force`: warning only).
- [An interrupted comparison leaves a half-written report] → The CSV and the report are written to a temporary directory and moved to the output directory (`-OutDir`, default `docs\benchmarks`) only when everything has finished, so an interrupted run never touches an existing report.
- [Working Set can be trimmed by Windows under memory pressure] → Private Bytes are reported next to Working Set; both must favour ActiveMQRust for criteria (a) and (b).
- [Results tied to one machine] → The report states the machine details and that results apply to that machine; anyone can reproduce them with the script.
- [The comparison shows a target is missed] → The report says so; the change is complete when the comparison is run and reported honestly, and misses become follow-up tasks.

## Migration Plan

Not applicable: this change adds a benchmark client workload, a script and documentation. It does not change the broker. Removing it means deleting the script and the generated reports.

## Open Questions

- Exact ActiveMQ versions to pin (latest 5.18.x and latest 6.x patch release at the time of the run); the report records the versions used.
- Whether a LAN run (client on another machine) should be added later; this change measures loopback only, because memory sampling and machine details are simpler on one machine.
