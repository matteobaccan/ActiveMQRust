## MODIFIED Requirements

### Requirement: Overview page
`/` SHALL show: `ActiveMQRust <version>` with the crate version; the OpenWire listen address and the admin console URL as two labelled lines directly below the page title, outside the figure cards; and, as cards, uptime; the number of active connections; the number of queues and of topics; the message memory in use and the configured limit (or "no limit"); and the process memory, as Working Set (RSS) and Private Bytes, read from the operating system at request time.

#### Scenario: Product identity and counts
- **WHEN** an authenticated client requests `/` while two OpenWire connections are open and three queues exist
- **THEN** the page shows `ActiveMQRust <crate version>`, 2 active connections, 3 queues, and both listen addresses

#### Scenario: Memory values
- **WHEN** an authenticated client requests `/`
- **THEN** the page shows the process Working Set in MB within 5% of the value reported by `Get-Process -Id <pid>` at the same time, and the message memory limit as "no limit" when `max_memory_mb = 0`

#### Scenario: Addresses under the title
- **WHEN** an authenticated client requests `/` on a broker listening on `0.0.0.0:61616` with the console on `127.0.0.1:8161`
- **THEN** right below the `<h1>` the page shows one line `OpenWire tcp://0.0.0.0:61616` and one line `Admin console http://127.0.0.1:8161`, and no figure card contains either address
