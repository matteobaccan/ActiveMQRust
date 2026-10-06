## Why

Every live message is held in RAM, so a backlog can never exceed the memory limit: when consumers are down for hours, producers are refused or asynchronous messages are dropped once `max_memory_mb` is reached. ActiveMQ handles long backlogs by keeping only part of them in memory and the rest on disk. Paging lets ActiveMQRust keep its RAM-first speed while absorbing backlogs larger than memory.

## What Changes

- New `[paging]` section, **off by default**: with it off, nothing changes.
- When message memory goes above `paging.start_pct` of `max_memory_mb` (default 70 %), the bodies of pending messages that are far from being dispatched are moved out of RAM; headers, properties and the FIFO index stay in RAM, so selectors, expiration, ordering and the admin console keep working.
- With storage on, a persistent message body is already in the journal: paging only drops it from RAM and reads it back from the journal, with no extra write. Non-persistent bodies (and persistent ones with storage off) go to page files in `paging.dir`, which are discarded at every start.
- Bodies are read back ahead of dispatch (a read-ahead window per destination), so consumers keep their order and do not wait for the disk in steady state.
- Paging stops when memory falls below `paging.stop_pct` (default 50 %); page files are deleted when their messages are gone.
- The memory limit applies to what stays in RAM; `paging.max_disk_mb` limits the page files, and beyond it the memory limit rules apply again.
- Admin console: paged messages and page file size per destination and on the overview; message pages read paged bodies from disk.

## Capabilities

### New Capabilities

- `message-paging`: `[paging]` configuration, what is paged and when, page files, read-ahead, interaction with storage, limits, admin data.

### Modified Capabilities

- `memory-management`: the accounted message memory counts only bodies held in RAM when paging is on (to be written in detail at the re-evaluation).

## Impact

- Code: `src/broker/entry.rs` (body resident or paged), new `src/paging/` (page files, read-ahead, cleanup), `src/broker/destination.rs` (paging decisions per destination), `src/storage/` (read a body back from the journal), `src/config.rs`, admin pages and API.
- Depends on `add-message-storage` (0.6.0) for the journal read-back.
- Planned for the last release of the roadmap (0.7.0); to be re-evaluated, and detailed, at the start of that release.
