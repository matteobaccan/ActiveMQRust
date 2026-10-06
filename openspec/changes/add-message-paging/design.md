## Context

Messages live in RAM as `Entry` (shared `Message` with headers, properties and body, plus a memory ticket). The memory limit refuses or drops messages once `max_memory_mb` is reached. From 0.6.0 an optional journal stores persistent messages with their exact bytes. Dispatch takes pending messages in FIFO order, one prefetch window per consumer; selectors evaluate headers and properties.

## Goals / Non-Goals

**Goals:**
- Backlogs larger than RAM without changing ordering, IDs, selectors or expiration.
- No cost while memory stays below the start threshold, and none at all with paging off.

**Non-Goals:**
- Paging topic messages of non-durable subscriptions (they are bounded by the slow-consumer limit).
- Paging headers and properties.
- Keeping non-persistent paged messages across restarts.

## Decisions

### D1. Page bodies only
Headers, properties and the FIFO index stay in RAM; only the body (the largest part) moves out. Selectors, expiration, the queue browser listing and the admin tables keep working without disk reads.
*Alternative*: page whole messages. Rejected: every selector evaluation or expiry check would read the disk.

### D2. Which bodies, and when
Above `start_pct` of the memory limit, a background task pages bodies from the tail of each destination's pending list (the messages furthest from dispatch), largest destinations first, until memory is below `stop_pct`. Messages within the read-ahead window (default 1000 per destination, or the sum of consumer prefetches if larger) are never paged.
*Alternative*: page at arrival once over the threshold. Rejected: it slows the producer path; the background task keeps the hot path unchanged.

### D3. Where bodies go
Persistent message bodies with storage on are already in the journal: the entry keeps the journal position and drops the body. Other bodies are appended to page files in `paging.dir` (append-only, deleted when no live body remains, all deleted at start).
*Alternative*: always write page files. Rejected: it writes persistent data twice.

### D4. Read-ahead
A per-destination reader keeps the bodies of the next read-ahead window resident, so dispatch reads from RAM in steady state; when a consumer is faster than the disk, dispatch waits for the reader rather than reordering.

### D5. Limits
Accounted memory counts resident bodies plus a fixed per-entry overhead for paged ones. `paging.max_disk_mb` caps page files; when reached, the existing memory-limit behaviour applies.

## Risks / Trade-offs

- [Disk reads slow down consumers of very long backlogs] → read-ahead; measured only with the user's approval.
- [Complexity on top of storage] → separate change, after storage is stable; to be detailed at the re-evaluation.

## Open Questions

- Thresholds, read-ahead sizes and topic durable subscriptions: to be decided at the re-evaluation of the release.
