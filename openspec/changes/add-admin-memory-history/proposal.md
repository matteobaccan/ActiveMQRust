## Why

The overview shows message memory and process memory only at the moment the page is loaded. When a client reports slowness or the broker hit its memory limit a few minutes ago, the operator cannot tell whether there was a peak, how high it went, or when. Messages live in RAM, so memory is the first thing to look at; the console should keep the last hour, and show how many messages the broker holds.

## What Changes

- The broker keeps, in memory, the last hour of message memory and process memory (Working Set and Private Bytes; resident size and physical footprint on macOS).
- Message memory is read every second and process memory every 10 seconds, by a task outside the message path; nothing is added to the code that stores or delivers messages.
- History in 10-second buckets (360 per hour), with minimum, maximum and last message memory per bucket; fixed size, a few KB.
- On the overview page: a chart of the last hour for message memory (with the configured limit) and one for process memory, drawn on the server as inline SVG (no JavaScript), each with the peak of the hour and its time.
- A new card "Messages in memory": pending plus inflight messages, computed with the existing broker count when the page or the API is requested, not sampled.
- `/api/memory/history` returns the buckets as JSON; `/api/overview` adds `messagesInMemory`.
- History starts empty at every start; it is kept only while the admin console is enabled.

## Capabilities

### New Capabilities

- `admin-memory-history`: sampling, buckets, overview charts, messages-in-memory card, history API.

### Modified Capabilities

None. The overview requirement keeps its cards; the new card and the charts are added by the new capability.

## Impact

- Code: new `src/admin/history.rs` (sampler task, ring of buckets), `src/admin/pages.rs` (card, SVG charts), `src/admin/api.rs` (`/api/memory/history`, `messagesInMemory`), `src/admin/style.css` (chart colours in both themes), tests.
- Message path: unchanged. The sampler reads the existing relaxed atomic `Memory::used`; the card uses the existing `Broker::message_count()`, which visits every destination, only on request.
- No new crates.
