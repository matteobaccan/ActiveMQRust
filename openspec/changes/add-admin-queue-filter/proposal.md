## Why

With dozens or hundreds of queues (per-application queues, temporary reply queues, DLQs) the `/queues` page becomes a long list: finding one queue, or the queues that need attention, means scrolling. Sorting by every column already exists ("Sortable queues table" in `admin-ui`); what is missing is a way to narrow the list.

## What Changes

- A filter bar above the queues table: a name filter (case-insensitive "contains") and two checkboxes, "Only with pending messages" and "Only without consumers".
- Server-side, no JavaScript, through the query parameters `q`, `pending=1` and `noconsumers=1`, combined with AND. Sort links and auto-refresh keep the filter, and the filter form keeps the sort.
- While a filter is active: a count "Showing N of M queues" and a "Clear filter" link; a dedicated row when nothing matches.
- `/api/queues` accepts the same parameters.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `admin-ui`: new requirement "Filterable queues table", next to "Sortable queues table".

## Impact

- Code: `src/admin/pages.rs` (filter form, row filtering, `KEPT` parameters, count line), `src/admin/api.rs` (`/api/queues` parameters), `src/admin/style.css` (filter bar, also at 360 px), tests in `tests/admin_http.rs`.
- No new crates, no JavaScript. The overview counts are not affected by the filter.
- ActiveMQ comparison: the ActiveMQ web console has a "Queue Name Filter" (contains); the two state checkboxes are an addition.
