## Context

`/queues` renders server-side HTML with no JavaScript (the CSP has no `script-src`). Sorting uses the `sort`/`order` query parameters through `QUEUES_TABLE` (`src/admin/sort.rs`, shared by the page and `/api/queues`), and `Ctx::with_kept` keeps the parameters listed in `KEPT` (sort parameters of every table, `page`, `view`, `seq`, `refresh`) on every link.

## Goals / Non-Goals

**Goals:**
- Narrow the queues list by name and by two common states, without JavaScript.
- Filter, sort and auto-refresh compose: each one keeps the others.

**Non-Goals:**
- Regular expressions or wildcards in the name filter.
- Filters on the topics and connections pages (they can follow the same pattern later).
- Remembering filters across sessions.

## Decisions

### D1. Plain GET form, parameters in the URL
A `<form method="get" action="/queues" role="search">` with a labelled text input `q`, two checkboxes (`pending=1`, `noconsumers=1`) and hidden inputs carrying the current `sort`, `order` and `refresh`. The URL can be shared and bookmarked, and auto-refresh, which reloads the URL, keeps the filter for free.
*Alternative*: filtering in the browser with JavaScript. Rejected: the console has no JavaScript by design (CSP and simplicity), and the API needs the same filter anyway.

### D2. Name match: case-insensitive "contains"
`q` is trimmed; an empty value means no name filter. A queue matches when its name contains `q`, compared case-insensitively (Unicode lowercase). A `q` longer than 200 characters is cut to 200.
*Alternative*: prefix match or `*` wildcards. Rejected: "contains" finds both `ORDERS.DLQ` and `app.orders.in` with `orders`, and matches the ActiveMQ console.

### D3. Filters combine with AND, filtering before sorting
Rows are filtered on the snapshot, then sorted by `QUEUES_TABLE.sort`. "Only with pending messages" keeps `pending > 0`; "Only without consumers" keeps queues with zero consumers (queue browsers count as consumers, as in the Consumers column).
*Alternative*: OR between the checkboxes. Rejected: the useful question is "messages waiting and nobody reading", which is AND.

### D4. One helper for page and API
`QueueFilter::from_params` and `QueueFilter::matches(&DestSnapshot)` are used by both `/queues` and `/api/queues`, so the two cannot diverge. `q`, `pending` and `noconsumers` are added to `KEPT`, so sort links keep the filter.
*Alternative*: separate parsing in each handler. Rejected: duplicated rules drift apart.

### D5. Feedback
While any filter is active the page shows "Showing N of M queues" (M = all queues) and a "Clear filter" link that keeps `sort`, `order` and `refresh`. With no match the table shows one row "No queues match the filter". The value of `q` is shown back HTML-escaped.
*Alternative*: no count. Rejected: without it an operator cannot tell a filtered list from a broker with few queues.

## Risks / Trade-offs

- [The input is reflected in the page] → `q` is always escaped with `esc`, in the input value and in the links; a test uses `<script>` in `q`.
- [Very many queues] → filtering is one linear pass over the snapshot already built for the page; no extra cost compared with rendering.
