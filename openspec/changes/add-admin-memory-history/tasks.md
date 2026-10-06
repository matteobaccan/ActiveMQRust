## 1. History

- [ ] 1.1 `src/admin/history.rs`: ring of 360 buckets (min, max, last message memory; working set, private bytes), bucket in progress, local start time
- [ ] 1.2 Sampler task started with the admin console: `Memory::used` every second, process memory and bucket close every 10 seconds; stops on shutdown
- [ ] 1.3 Unit tests with a fake clock: bucket rollover, one-hour window, recent start, min/max within a bucket

## 2. Overview page

- [ ] 2.1 "Messages in memory" card from `Broker::message_count()`, with the label "pending + inflight, per subscription for topics"
- [ ] 2.2 SVG chart renderer: scales, 15-minute ticks, min–max band, lines, dashed limit line, empty periods, legend, "Peak … at …", per-bucket `<title>`, `role="img"` and `aria-label`
- [ ] 2.3 The two charts and the collapsible "Data" table
- [ ] 2.4 Chart colours and line styles as CSS classes for light and dark themes; check 360 px width

## 3. API

- [ ] 3.1 `GET /api/memory/history`; `messagesInMemory` in `/api/overview`

## 4. Tests and documentation

- [ ] 4.1 `tests/admin_http.rs`: card value (queue and topic copies), SVG present without `<script>`, limit line on/off, peak text, history JSON
- [ ] 4.2 README: admin console section (charts, card, API, sub-second peaks not captured)
- [ ] 4.3 CHANGELOG entry under Unreleased
- [ ] 4.4 `cargo fmt` and the test suite
