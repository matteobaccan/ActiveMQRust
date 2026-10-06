## 1. Filter logic

- [x] 1.1 `QueueFilter` in `src/admin/pages.rs`: parse `q` (trim, 200-character cap), `pending`, `noconsumers`; `matches(&DestSnapshot)` with case-insensitive "contains" and AND
- [x] 1.2 Add `q`, `pending`, `noconsumers` to `KEPT` so sort links (`Table::sort_page`) keep the filter
- [x] 1.3 Unit tests: case-insensitive contains, empty and long `q`, each checkbox, AND combination

## 2. Queues page

- [x] 2.1 Filter form (GET, `role="search"`, labels, hidden `sort`/`order`/`refresh`) above the table, values shown escaped
- [x] 2.2 Filter before sorting; "Showing N of M queues", "Clear filter" link, "No queues match the filter" row
- [x] 2.3 Styles for the filter bar in both themes and at 360 px width

## 3. API

- [x] 3.1 `/api/queues` applies `QueueFilter` before sorting

## 4. Tests and documentation

- [x] 4.1 `tests/admin_http.rs`: every scenario of the spec, including escaping and the API
- [x] 4.2 README: the admin console section mentions the filter and its parameters
- [x] 4.3 CHANGELOG entry under Unreleased
- [x] 4.4 `cargo fmt` and the test suite
