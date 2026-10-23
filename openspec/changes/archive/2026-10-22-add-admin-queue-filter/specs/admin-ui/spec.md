## ADDED Requirements

### Requirement: Filterable queues table
The queues page SHALL show, above the table, a filter form (`role="search"`, every control with a visible label) with a name field and the checkboxes "Only with pending messages" and "Only without consumers". The form SHALL use GET with the parameters `q`, `pending=1` and `noconsumers=1`, and SHALL work without JavaScript. A queue SHALL match the name filter when its name contains the trimmed value of `q`, compared case-insensitively; an empty `q` SHALL NOT filter, and a `q` longer than 200 characters SHALL be cut to 200. "Only with pending messages" SHALL keep queues with at least one pending message; "Only without consumers" SHALL keep queues with zero consumers. Active filters SHALL combine with AND. The sort links and auto-refresh SHALL keep the filter, and the form SHALL keep the current `sort`, `order` and `refresh`. While a filter is active the page SHALL show "Showing N of M queues" and a "Clear filter" link that removes `q`, `pending` and `noconsumers` and keeps the other parameters; when nothing matches, the table SHALL show the row "No queues match the filter". The value of `q` SHALL be HTML-escaped wherever it is shown. `/api/queues` SHALL accept the same parameters with the same rules. The overview counts SHALL NOT be affected by the filter.

#### Scenario: Name contains, any case
- **WHEN** the queues `app.ORDERS.in`, `ORDERS.DLQ` and `billing` exist and the client opens `/queues?q=orders`
- **THEN** only `app.ORDERS.in` and `ORDERS.DLQ` are listed and the page shows "Showing 2 of 3 queues"

#### Scenario: Messages waiting and nobody reading
- **WHEN** `Q1` has 5 pending messages and no consumers, `Q2` has 5 pending and 1 consumer, `Q3` has 0 pending and no consumers, and the client opens `/queues?pending=1&noconsumers=1`
- **THEN** only `Q1` is listed

#### Scenario: Filter kept by sorting
- **WHEN** the client opens `/queues?q=app&sort=pending&order=desc` and follows the Consumers header link
- **THEN** the new URL keeps `q=app` and sorts by Consumers ascending

#### Scenario: Form keeps the sort
- **WHEN** the table is sorted by Pending descending with `refresh=5` and the client submits the filter form with `orders`
- **THEN** the resulting URL contains `q=orders`, `sort=pending`, `order=desc` and `refresh=5`

#### Scenario: Kept on refresh
- **WHEN** the client opens `/queues?q=orders&refresh=5`
- **THEN** every refresh shows only the matching queues

#### Scenario: No match
- **WHEN** the client opens `/queues?q=nothing-like-this`
- **THEN** the table shows one row "No queues match the filter" and the page shows a "Clear filter" link

#### Scenario: Escaped input
- **WHEN** the client opens `/queues?q=%3Cscript%3E`
- **THEN** the page contains `&lt;script&gt;` and no `<script>` element coming from the input

#### Scenario: API filter
- **WHEN** a script requests `/api/queues?q=dlq&pending=1&sort=pending&order=desc`
- **THEN** the JSON array contains only queues whose name contains `dlq` in any case and that have pending messages, ordered by pending, highest first
