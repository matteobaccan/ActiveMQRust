## 0. Re-evaluation at the start of the release

- [ ] 0.1 Re-read this change against the storage of 0.6.0 and what was built since; detail thresholds, read-ahead, durable subscriptions and the `memory-management` delta with the user before implementing

## 1. Configuration

- [ ] 1.1 `[paging]` keys, validation (requires `max_memory_mb`), template entries

## 2. Paging

- [ ] 2.1 Entry body resident or paged (journal position or page file position)
- [ ] 2.2 Background pager: thresholds, tail-first selection, read-ahead window protected
- [ ] 2.3 Page files: append, delete when empty, delete all at startup, disk limit
- [ ] 2.4 Read-ahead reader per destination; dispatch waits instead of reordering
- [ ] 2.5 Journal read-back for persistent bodies with storage on

## 3. Console and tests

- [ ] 3.1 Paged counts and page file size on the overview, queue pages and API; message page reads paged bodies
- [ ] 3.2 Tests: backlog twice the memory limit, order and bodies preserved, selectors and expiry on paged messages, restart cleanup
- [ ] 3.3 Ask the user before running load tests with paging

## 4. Documentation

- [ ] 4.1 README paging section and CHANGELOG entry
- [ ] 4.2 `cargo fmt` and the test suite
