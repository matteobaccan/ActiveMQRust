## MODIFIED Requirements

### Requirement: Sortable queues table
Every column of the queues table SHALL be sortable by clicking its header: Name, Pending, Inflight, Consumers, Producers, Enqueued, Consumed and Expired, and any column added later. Sorting SHALL be done server-side through `sort=<column>&order=asc|desc`, with no JavaScript; the default is Name ascending. Numeric columns SHALL sort numerically and Name SHALL sort case-insensitively in natural order, where runs of digits compare as numbers (`Q2` before `Q10`); rows with equal values SHALL keep a stable order by name ascending. The sorted column SHALL show an arrow for its direction and carry `aria-sort`; clicking it again SHALL reverse the direction, clicking another column SHALL sort that column ascending. The sort SHALL be kept by auto-refresh, and an unknown `sort` value SHALL fall back to Name. `/api/queues` SHALL accept the same `sort` and `order` parameters.

#### Scenario: Every column sortable
- **WHEN** a logged-in client opens `/queues`
- **THEN** each of the eight column headers is a link that sorts by that column

#### Scenario: Numeric order
- **WHEN** three queues have 9, 10 and 100 pending messages and the client sorts by Pending ascending
- **THEN** the rows are in the order 9, 10, 100

#### Scenario: Natural name order
- **WHEN** the queues `Q10`, `q2` and `Q1` exist and the table is sorted by Name ascending
- **THEN** the rows are in the order `Q1`, `q2`, `Q10`

#### Scenario: Ties broken by name
- **WHEN** queues `B` and `A` both have 0 consumers and the table is sorted by Consumers
- **THEN** `A` is listed before `B`

#### Scenario: Toggle direction
- **WHEN** the table is sorted by Enqueued ascending and the client clicks the Enqueued header
- **THEN** the table is sorted by Enqueued descending and the header shows the descending arrow with `aria-sort="descending"`

#### Scenario: Sort kept on refresh
- **WHEN** the client opens `/queues?sort=pending&order=desc&refresh=5`
- **THEN** every refresh keeps the order by Pending descending

#### Scenario: API sort
- **WHEN** a script requests `/api/queues?sort=consumed&order=desc`
- **THEN** the JSON array is ordered by consumed messages, highest first

## ADDED Requirements

### Requirement: Sortable tables on every page
Every list table of the console SHALL be sortable by every column, with the behaviour of the queues table: server-side, no JavaScript, header links, an arrow and `aria-sort` on the sorted column, a second click reversing the direction, another column starting ascending, an unknown column falling back to the table's default, and the sort kept by auto-refresh. The tables, columns and defaults SHALL be:
- Topics (`sort`/`order`): Name, Consumers, Producers, Published, Discarded; default Name ascending.
- Connections (`sort`/`order`): Connection ID, User, Client, OpenWire, Connected, Sessions, Consumers, Producers; default Connected ascending.
- Queue detail, Consumers (`csort`/`corder`): Consumer ID, Connection ID, Client, Prefetch, Inflight, Selector; default Consumer ID ascending.
- Queue detail, Producers (`psort`/`porder`): Producer ID, Connection ID, Client; default Producer ID ascending.
- Message detail, Properties (`prsort`/`prorder`): Name, Type, Value; default Name ascending.
- Message detail, MapMessage body (`msort`/`morder`): Key, Type, Value; default Key ascending.

Numeric columns SHALL sort numerically; Connected SHALL sort chronologically; Client SHALL sort by IP address and then by port, numerically; text and identifier columns SHALL sort case-insensitively in natural order, with empty values first in ascending order. Rows with equal values SHALL be ordered by the table's first column ascending. Sorting one table SHALL keep the sort of the other tables on the same page, the contents page, the body view and the refresh. `/api/topics` and `/api/connections` SHALL accept `sort` and `order` with the column keys of the page.

#### Scenario: Connections by consumers
- **WHEN** three connections have 0, 12 and 3 consumers and the client opens `/connections?sort=consumers&order=desc`
- **THEN** the rows are in the order 12, 3, 0 and the Consumers header shows the descending arrow with `aria-sort="descending"`

#### Scenario: Connections by client address
- **WHEN** clients connect from `10.0.0.10:5000`, `10.0.0.9:6000` and `10.0.0.9:5001` and the table is sorted by Client ascending
- **THEN** the rows are in the order `10.0.0.9:5001`, `10.0.0.9:6000`, `10.0.0.10:5000`

#### Scenario: Connections by time
- **WHEN** the table is sorted by Connected descending
- **THEN** the most recent connection is first

#### Scenario: Natural order of IDs
- **WHEN** a queue has consumers `ID:h-1:1:1:2` and `ID:h-1:1:1:10` and its Consumers table is sorted by Consumer ID ascending
- **THEN** `ID:h-1:1:1:2` is listed before `ID:h-1:1:1:10`

#### Scenario: Two tables on one page
- **WHEN** the queue detail is sorted by Consumers Prefetch descending and the client then sorts the Producers table by Client
- **THEN** the URL holds both `csort=prefetch&corder=desc` and `psort=client&porder=asc`, and both tables are shown in those orders

#### Scenario: Contents page kept
- **WHEN** the client is on page 3 of a queue's contents and sorts the Consumers table
- **THEN** the contents still show page 3

#### Scenario: Topics API sort
- **WHEN** a script requests `/api/topics?sort=published&order=desc`
- **THEN** the JSON array is ordered by published messages, highest first

#### Scenario: Unknown column
- **WHEN** the client opens `/connections?sort=bogus`
- **THEN** the table is sorted by Connected ascending

### Requirement: Tables that keep their natural order
The queue contents table SHALL keep the FIFO order across its pages, the message headers table SHALL keep its fixed order and the StreamMessage body SHALL keep the order of its values; their headers SHALL NOT be sort links, because the order is part of what they show.

#### Scenario: Contents not sortable
- **WHEN** a logged-in client opens a queue's detail page
- **THEN** the headers of the Messages table are not links and the messages are listed in FIFO order
