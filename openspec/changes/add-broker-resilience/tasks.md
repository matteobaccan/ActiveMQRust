## 1. Crash isolation and fuzzing

- [ ] 1.1 Fuzz targets for the OpenWire decoder and the selector parser; turn every crash into a regression test
- [ ] 1.2 Remove or contain panics on the message path; a failure closes only its connection

## 2. Memory

- [ ] 2.1 Design and implement a safe default for `max_memory_mb` (share of physical RAM); README and template
- [ ] 2.2 Stress test: backlog driven to the limit; refused and discarded messages counted and reported

## 3. Stress

- [ ] 3.1 Thousands of connections opening and closing; very slow consumers with large prefetch

## 4. Automatic restart

- [ ] 4.1 Windows service recovery actions set by `service install`, shown by `service status`; restart logged
- [ ] 4.2 macOS launchd property list with `KeepAlive` (README template or `service install` on macOS)

## 5. Verification

- [ ] 5.1 `openspec validate add-broker-resilience` passes; README updated
