## 1. Generic sorting

- [x] 1.1 Column definition (key, label, kind, accessor) and one helper that parses `<prefix>sort`/`<prefix>order`, sorts stably and renders the header row with links, arrow, `aria-sort` and `num` class
- [x] 1.2 Typed comparators: number, time, socket address (IP then port, unparsable last), case-insensitive natural text with empty values first; ties by the first column
- [x] 1.3 Move the queues table to the helper; natural order for Name; update existing tests
- [x] 1.4 Add `csort`, `corder`, `psort`, `porder`, `prsort`, `prorder`, `msort`, `morder` to `KEPT`
- [x] 1.5 Unit tests for every comparator and for the default and fallback rules

## 2. Tables

- [x] 2.1 Topics table sortable, default Name ascending
- [x] 2.2 Connections table sortable, default Connected ascending
- [x] 2.3 Queue detail: Consumers and Producers tables, independent parameters
- [x] 2.4 Message detail: Properties and MapMessage body tables
- [x] 2.5 Contents, headers and StreamMessage tables keep plain headers

## 3. API

- [x] 3.1 `/api/topics` and `/api/connections` accept `sort`/`order` with the page column keys

## 4. Tests and documentation

- [x] 4.1 `tests/admin_http.rs`: every scenario of the spec
- [x] 4.2 README: admin console section lists the sortable tables and parameters
- [x] 4.3 CHANGELOG entry under Unreleased
- [x] 4.4 `cargo fmt` and the test suite
