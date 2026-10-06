## Context

`Memory` in `src/broker/entry.rs` holds the broker-wide accounted message memory (`used`, a relaxed `AtomicU64`) updated by `MemTicket`. `Broker::message_count()` sums, per destination under its lock, pending messages plus, per subscription, inflight and topic pending messages; it is used today only at shutdown. `admin::process_memory()` reads Working Set / Private Bytes on Windows and resident size / physical footprint on macOS. The console has no JavaScript (CSP without `script-src`; `style-src 'self'`, so no inline `style` attributes).

## Goals / Non-Goals

**Goals:**
- See the last hour of message and process memory and spot a peak with its value and time.
- No change at all on the message path; fixed memory for the history.
- Show the number of messages held, on request.

**Non-Goals:**
- History longer than one hour, or kept across restarts.
- A history of the message count (it would need a counter on the message path; decided against).
- Capturing peaks shorter than one second.
- Per-destination history, alerts, interactive charts.

## Decisions

### D1. Sampling outside the message path
One task, started with the admin console, reads `Memory::used` every second and process memory every 10 seconds. Nothing is added to `MemTicket` or to delivery.
*Alternative*: a peak tracker (`fetch_max`) in `MemTicket` to catch sub-second peaks. Rejected by product decision: no extra work on the message path. A burst shorter than one second may be missed; bursts that fill memory for a second or more, which are the ones that hit the limit, are seen.

### D2. Buckets
A bucket of 10 seconds holds min, max and last of the ten message-memory samples, and the process memory read at its end. A ring of 360 buckets covers one hour. The bucket in progress is included in the output.
*Alternative*: one point per second (3600). Rejected: heavier SVG with no visible gain at chart width; min/max per bucket keep the shape.

### D3. Server-side SVG
The overview renders two inline `<svg viewBox>` charts that scale with the page width:
1. **Message memory** (MB): min–max band and last-value line; a dashed horizontal line for `max_memory_mb` when set, labelled "limit".
2. **Process memory** (MB): Working Set and Private Bytes lines.

Each chart has a 60-minute time axis with local-time ticks every 15 minutes, a value axis from 0 with rounded ticks, a legend, and above it "Peak <value> at <HH:MM:SS>". Colours come from CSS classes with tokens for both themes; series also differ by line style. Each bucket has an invisible rectangle with a `<title>` (time and values) for a native tooltip. Periods without data are left empty.
*Alternative*: a JavaScript chart library. Rejected: no JavaScript in the console.

### D4. Accessibility
Each `<svg>` has `role="img"` and an `aria-label` summary ("Message memory, last hour: now 120 MB, peak 410 MB at 10:42:10, limit 1024 MB"). A `<details>` "Data" below holds the buckets as a table.

### D5. Messages in memory card
The card shows `Broker::message_count()` computed when the overview or `/api/overview` is requested: pending messages plus inflight messages, where a topic message counts once per subscription that holds it. It briefly takes each destination lock once per request, which the overview already does for its other figures.
*Alternative*: a lock-free counter updated on store and release. Rejected by product decision (no counters on the message path).

### D6. API
`GET /api/memory/history` returns `{"intervalSeconds": 10, "limitBytes": <n|null>, "buckets": [{"start": "<RFC 3339>", "messageMemory": {"min", "max", "last"}, "workingSet": n, "privateBytes": n}, …]}`, oldest first, including the bucket in progress. `/api/overview` adds `messagesInMemory`.

### D7. Auto-refresh
The overview's existing auto-refresh (5 s) redraws the charts and the card.

## Risks / Trade-offs

- [Sub-second peaks can be missed] → accepted (D1); stated in the README.
- [Process memory every 10 s] → it changes slowly (the allocator keeps freed pages); message memory is sampled every second.
- [Topic copies make the card larger than the number of distinct messages] → the card's label explains "pending + inflight, per subscription for topics".
