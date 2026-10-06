## 1. Generic sorting

- [ ] 1.1 Column definition (key, label, kind, accessor) and one helper that parses `<prefix>sort`/`<prefix>order`, sorts stably and renders the header row with links, arrow, `aria-sort` and `num` class
- [ ] 1.2 Typed comparators: number, time, socket address (IP then port, unparsable last), case-insensitive natural text with empty values first; ties by the first column
- [ ] 1.3 Move the queues table to the helper; natural order for Name; update existing tests
- [ ] 1.4 Add `csort`, `corder`, `psort`, `porder`, `prsort`, `prorder`, `msort`, `morder` to `KEPT`
- [ ] 1.5 Unit tests for every comparator and for the default and fallback rules

## 2. Tables

- [ ] 2.1 Topics table sortable, default Name ascending
- [ ] 2.2 Connections table sortable, default Connected ascending
- [ ] 2.3 Queue detail: Consumers and Producers tables, independent parameters
- [ ] 2.4 Message detail: Properties and MapMessage body tables
- [ ] 2.5 Contents, headers and StreamMessage tables keep plain headers

## 3. API

- [ ] 3.1 `/api/topics` and `/api/connections` accept `sort`/`order` with the page column keys

## 4. Tests and documentation

- [ ] 4.1 `tests/admin_http.rs`: every scenario of the spec
- [ ] 4.2 README: admin console section lists the sortable tables and parameters
- [ ] 4.3 CHANGELOG entry under Unreleased
- [ ] 4.4 `cargo fmt` and the test suite
