## Context

The queues table is sorted by `sort_params`/`sort_queues` in `src/admin/pages.rs` over the eight `QUEUE_COLUMNS`; the header links are built by hand in `queues()`. The other tables (`topics()`, `connections()`, the Consumers and Producers tables of `queue_detail()`, Properties and the map body of `message_detail()`) are printed in the order of the broker snapshot. `Ctx::link` keeps the query parameters listed in `KEPT`.

## Goals / Non-Goals

**Goals:**
- One sorting behaviour for every list table, implemented once.
- Sorting one table never resets another table on the same page, the contents page, the view or the refresh.

**Non-Goals:**
- Sorting the queue contents (FIFO across pages), the headers table or the StreamMessage body.
- Multi-column sorting.
- Sorting in the browser.

## Decisions

### D1. A generic column table and one sort helper
Each table is described by a static list of columns: key, label, kind (`Text`, `Natural` identifier, `Number`, `Time`, `Address`) and an accessor from the row to a sort key. One helper parses the parameters, sorts with a stable sort and renders the header row (links, arrow, `aria-sort`, `num` class). The queues table moves to the same helper, with unchanged behaviour except the natural text order.
*Alternative*: copy the queues code into each page. Rejected: five copies of the toggle, arrow and fallback rules would drift.

### D2. One pair of parameters per table
Pages with one sortable table use `sort`/`order` (Queues, Topics, Connections). The queue detail uses `csort`/`corder` (Consumers) and `psort`/`porder` (Producers); the message detail uses `prsort`/`prorder` (Properties) and `msort`/`morder` (MapMessage body). All of them are added to `KEPT`, so every link of the page keeps them, including the contents page links and the Raw/Formatted switch.
*Alternative*: one shared `sort` parameter with a table prefix (`sort=consumers.prefetch`). Rejected: two tables on the same page could not be sorted at the same time.

### D3. Typed comparison
- Number: numeric (`9 < 10 < 100`).
- Time (Connected): chronological, on the stored timestamp, not on the displayed text.
- Address (Client): IP address numerically (IPv4 before IPv6), then port numerically; an address that cannot be parsed sorts as text after the others.
- Text and identifiers (Name, User, Selector, Type, Value, Connection/Consumer/Producer ID): case-insensitive natural order, where runs of digits compare as numbers, so `ID:host-1:2` comes before `ID:host-1:10` and `Q2` before `Q10`. An empty value (no selector, no user) sorts first in ascending order.
- Ties: by the table's first column ascending (natural order), then by the snapshot order, so the result is stable across refreshes.
*Alternative*: plain case-insensitive text everywhere. Rejected: OpenWire IDs end with numeric counters, and `:10` before `:2` makes the Connections and Consumers tables hard to read.

### D4. Defaults equal today's order
Topics: Name ascending. Connections: Connected ascending (oldest first, as today's connection order). Consumers: Consumer ID ascending. Producers: Producer ID ascending. Properties: Name ascending. Map entries: Key ascending. An unknown column falls back to the default of that table; an unknown `order` falls back to ascending.
*Alternative*: newest connection first. Rejected: it changes what operators see today; one click gives it.

### D5. API
`/api/topics` and `/api/connections` accept `sort`/`order` with the same keys as the page columns (`name`, `consumers`, `producers`, `published`, `discarded`; `connectionId`, `user`, `client`, `openwire`, `connected`, `sessions`, `consumers`, `producers`). The nested arrays of `/api/queues/{name}` are not sorted by parameters, as before.
*Alternative*: no API sorting. Rejected: `/api/queues` already sorts; the API should be uniform.

## Risks / Trade-offs

- [Natural order changes the Name order of the queues table for names with numbers] → intended; the modified requirement states it; existing tests are updated.
- [More parameters on the queue detail and message pages] → only present after the user clicks a header; the default URLs do not change.
