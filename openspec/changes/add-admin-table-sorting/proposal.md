## Why

Only the queues table can be sorted. With many clients connected, the Connections page is a list in connection order that cannot be sorted by user, client address, connection time or consumer count; the same goes for topics and for the consumers and producers of a queue. Every list table in the console needs the same sorting, so it behaves the same way everywhere.

## What Changes

- Every list table becomes sortable by every column, with the rules of the queues table (server-side, no JavaScript, header links, arrow and `aria-sort`, stable ties, kept by auto-refresh):
  - Topics: Name, Consumers, Producers, Published, Discarded.
  - Connections: Connection ID, User, Client, OpenWire, Connected, Sessions, Consumers, Producers.
  - Queue detail, Consumers: Consumer ID, Connection ID, Client, Prefetch, Inflight, Selector.
  - Queue detail, Producers: Producer ID, Connection ID, Client.
  - Message detail, Properties: Name, Type, Value; MapMessage body: Key, Type, Value.
- Pages with several tables use one pair of parameters per table, so sorting one table keeps the order of the others.
- Typed comparison per column: numbers numerically, dates chronologically, client addresses by IP then port, text case-insensitively in natural order (`Q2` before `Q10`). This natural order also applies to the Name column of the queues table.
- `/api/topics` and `/api/connections` accept `sort` and `order`, like `/api/queues`.
- Deliberately not sortable: the queue contents table (the order is the FIFO position, across pages), the message headers table and the StreamMessage body (their order is part of the content).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `admin-ui`: "Sortable queues table" uses the natural text order for Name; new requirements "Sortable tables on every page" and "Tables that keep their natural order".

## Impact

- Code: `src/admin/pages.rs` (a generic column definition and sort helper shared by every table, header rendering, parameters per table in `KEPT`), `src/admin/api.rs` (`/api/topics`, `/api/connections`), tests in `tests/admin_http.rs`.
- No new crates, no JavaScript.
- Default orders stay as today: Topics by Name, Connections by Connected (oldest first), Consumers and Producers by ID, Properties and Map entries by Name/Key.
