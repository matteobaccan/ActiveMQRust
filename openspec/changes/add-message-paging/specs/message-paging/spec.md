## ADDED Requirements

### Requirement: Paging configuration
The `[paging]` section SHALL accept `enabled` (boolean, default `false`), `dir` (default `paging`, relative to the executable folder or absolute), `start_pct` (default 70) and `stop_pct` (default 50, lower than `start_pct`), percentages of `broker.max_memory_mb`, `read_ahead` (default 1000 messages per destination) and `max_disk_mb` (0 = no limit). Paging SHALL require `broker.max_memory_mb` greater than 0. Invalid values SHALL be configuration errors naming the key, exit code 2. With `enabled = false` the broker SHALL NOT create or read any page file.

#### Scenario: Paging without a memory limit
- **WHEN** `paging.enabled = true` and `broker.max_memory_mb = 0`
- **THEN** startup fails with exit code 2 naming both keys

### Requirement: Backlogs larger than memory
With paging on, when accounted message memory exceeds `start_pct` of the limit, the broker SHALL move out of RAM the bodies of pending queue messages and durable subscription messages furthest from dispatch, until memory falls below `stop_pct`, never paging the next `read_ahead` messages of a destination. Headers, properties and order SHALL stay in RAM. Consumers SHALL receive every message with its original body, ID, headers and properties, in FIFO order. Selectors, expiration, the queue browser and the admin console SHALL work on paged messages.

#### Scenario: Backlog twice the memory limit
- **WHEN** `max_memory_mb = 100`, paging is on, and 200 MB of messages are sent to a queue with no consumer
- **THEN** every send succeeds, accounted memory stays below the limit, and a consumer started afterwards receives all messages in order with identical bodies

### Requirement: Page storage and cleanup
With storage on, a persistent message body SHALL be paged by dropping it from RAM and reading it back from the journal, with no additional write. Other bodies SHALL be written to page files in `paging.dir`. Page files SHALL be deleted when they hold no live body, and every page file SHALL be deleted at startup. When page files reach `paging.max_disk_mb`, the memory limit rules SHALL apply to new messages.

#### Scenario: Restart discards page files
- **WHEN** the broker stops with non-persistent messages paged out and starts again
- **THEN** `paging.dir` holds no page file from the previous run

### Requirement: Paging data in the admin console
The overview and the queue pages, and the matching API fields, SHALL show the number of paged messages and the size of page files; the message page SHALL read a paged body from disk without changing the message.

#### Scenario: Paged message viewed
- **WHEN** an administrator opens the page of a paged message
- **THEN** the body is shown and the message stays paged and pending
