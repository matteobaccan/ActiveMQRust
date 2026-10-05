# ActiveMQRust vs ActiveMQ comparison (2026-10-05)

- Machine: Intel(R) Xeon(R) W-2123 CPU @ 3.60GHz, 31.7 GB RAM, Microsoft Windows 11 Pro for Workstations 10.0.26200
- JDK: openjdk version "21.0.12.1" 2026-08-18 LTS
- ActiveMQRust: ActiveMQRust 0.1.0 (release build)
- Runs: 1 warm-up (discarded) + 1 measured; values are medians. Warm-up inside each run: 20000 messages.
- Messages: XML TextMessage with 20 random fields plus a base64 buffer, NON_PERSISTENT, AUTO_ACKNOWLEDGE, prefetch 1000.
- 1 KB and 10 KB messages are below the 32 KB broker compression threshold, so ActiveMQRust compression is not active in (b), (c) and (d).

## ActiveMQ 5.18.7 (tuned, in RAM) vs ActiveMQRust - client amq5

| Measurement | ActiveMQ | ActiveMQRust | Verdict (ActiveMQRust better) |
|---|---|---|---|
| (a) idle Working Set | n/a | n/a | n/a |
| (a) idle Private Bytes | n/a | n/a | n/a |
| (b) 100k x 10 KB held: Working Set | n/a | n/a | n/a |
| (b) 100k x 10 KB held: Private Bytes | n/a | n/a | n/a |
| (c) produce time 100k x 10 KB | n/a | n/a | n/a |
| (c) consume time 100k x 10 KB | n/a | n/a | n/a |
| (d) 1 KB async: elapsed | n/a | n/a | n/a |
| (d) 1 KB sync: elapsed | n/a | n/a | n/a |
| (e) 10k x 50 KB held: Working Set (compression off) | 744,7 MB | 527,9 MB | met |
| (e) produce time 10k x 50 KB (compression off) | 3.286 ms | 2.279 ms | met |
| (e) consume time 10k x 50 KB (compression off) | 1.728 ms | 3.659 ms | not met |
| (e) 10k x 50 KB held: Working Set (ActiveMQRust defaults, compression active) | 744,7 MB | 606,4 MB | met |

Broker compression ratio in (e) (message memory with compression / without): 100,0%

- Target memory <= 1/5 of ActiveMQ at idle (Working Set): **n/a**
- Target idle Working Set < 20 MB: **n/a**

## Configuration used

ActiveMQ tuned (`scripts/activemq-bench/activemq-tuned.xml`, JVM `-Xmx4g`):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!--
  ActiveMQRust by Matteo Baccan
  SPDX-License-Identifier: MIT

  ActiveMQ configuration for a fair in-RAM comparison with ActiveMQRust:
  no persistence, VM queue cursor (no spooling to temp storage), no producer flow control,
  3 GB memory limits, no JMX, advisories or scheduler, only OpenWire on 127.0.0.1:61616,
  simple authentication with the benchmark user.
-->
<beans xmlns="http://www.springframework.org/schema/beans"
       xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
       xsi:schemaLocation="http://www.springframework.org/schema/beans http://www.springframework.org/schema/beans/spring-beans.xsd
                           http://activemq.apache.org/schema/core http://activemq.apache.org/schema/core/activemq-core.xsd">

  <broker xmlns="http://activemq.apache.org/schema/core"
          brokerName="bench"
          persistent="false"
          useJmx="false"
          advisorySupport="false"
          schedulerSupport="false"
          dataDirectory="${activemq.data}">

    <destinationPolicy>
      <policyMap>
        <policyEntries>
          <policyEntry queue=">" producerFlowControl="false">
            <pendingQueuePolicy>
              <vmQueueCursor/>
            </pendingQueuePolicy>
          </policyEntry>
          <policyEntry topic=">" producerFlowControl="false"/>
        </policyEntries>
      </policyMap>
    </destinationPolicy>

    <systemUsage>
      <systemUsage>
        <memoryUsage><memoryUsage limit="3 gb"/></memoryUsage>
        <storeUsage><storeUsage limit="3 gb"/></storeUsage>
        <tempUsage><tempUsage limit="3 gb"/></tempUsage>
      </systemUsage>
    </systemUsage>

    <plugins>
      <simpleAuthenticationPlugin>
        <users>
          <authenticationUser username="admin" password="admin" groups="admins,users"/>
        </users>
      </simpleAuthenticationPlugin>
    </plugins>

    <transportConnectors>
      <transportConnector name="openwire" uri="tcp://127.0.0.1:61616?maximumConnections=1000&amp;wireFormat.maxFrameSize=104857600"/>
    </transportConnectors>
  </broker>
</beans>

```
ActiveMQRust (`scripts/activemq-bench/mqrust-bench.toml`):
```toml
# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# ActiveMQRust configuration for the comparison benchmark: only the benchmark user is set;
# every other key keeps its default (broker compression above 32 KB is active).

[broker]
bind = "127.0.0.1"

[admin]
username = "admin"
password = "admin"

[[users]]
username = "admin"
password = "admin"

```
Every individual run is in the CSV file next to this report.

