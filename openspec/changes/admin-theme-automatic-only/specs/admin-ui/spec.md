## MODIFIED Requirements

### Requirement: Theme follows the operating system
The stylesheet SHALL define its colours as CSS custom properties for a light and a dark palette and SHALL select the palette with `@media (prefers-color-scheme: dark)`, so the console follows the light/dark setting of the operating system or browser with no JavaScript. The theme SHALL be chosen only this way: the console SHALL NOT offer a theme selector, SHALL NOT set a theme cookie and SHALL NOT render a forced `data-theme`. Pages SHALL declare `<meta name="color-scheme" content="light dark">` so form controls and scrollbars match the theme. When the system setting changes while a page is open, the page SHALL switch theme without a reload.

#### Scenario: Dark system
- **WHEN** a browser with dark mode active opens `/`
- **THEN** the page uses the dark palette (dark background, light text)

#### Scenario: Light system
- **WHEN** a browser with light mode active opens `/`
- **THEN** the page uses the light palette

#### Scenario: No manual choice
- **WHEN** any console page or `/login` is rendered
- **THEN** it contains no theme selector, sets no theme cookie, and `<html>` carries no `data-theme` attribute

### Requirement: Modern visual design
The console SHALL use a consistent visual system defined once in the stylesheet: the system font stack (`system-ui`, `Segoe UI`, …) and a monospace stack for IDs and bodies; spacing, radius and colour tokens; a top bar with the product name, the navigation, the logged-in user and logout; cards for overview figures; tables with sticky headers, zebra rows and right-aligned numbers; badges for states; visible keyboard focus. No external fonts, images, scripts or stylesheets SHALL be loaded; everything stays embedded in the executable and the `Content-Security-Policy` stays `default-src 'none'; style-src 'self'` plus `form-action 'self'` and `frame-ancestors 'none'`. The stylesheet SHALL stay under 20 KB.

#### Scenario: No external resources
- **WHEN** any console page is loaded with the network to the internet blocked
- **THEN** it renders completely and the browser makes no request outside the console's host

#### Scenario: Keyboard use
- **WHEN** an operator moves through a page with the Tab key
- **THEN** every link, button and form field shows a visible focus outline

### Requirement: Sortable queues table
Every column of the queues table SHALL be sortable by clicking its header: Name, Pending, Inflight, Consumers, Producers, Enqueued, Consumed and Expired, and any column added later. Sorting SHALL be done server-side through `sort=<column>&order=asc|desc`, with no JavaScript; the default is Name ascending. Numeric columns SHALL sort numerically and Name SHALL sort case-insensitively; rows with equal values SHALL keep a stable order by name ascending. The sorted column SHALL show an arrow for its direction and carry `aria-sort`; clicking it again SHALL reverse the direction, clicking another column SHALL sort that column ascending. The sort SHALL be kept by auto-refresh, and an unknown `sort` value SHALL fall back to Name. `/api/queues` SHALL accept the same `sort` and `order` parameters.

#### Scenario: Every column sortable
- **WHEN** a logged-in client opens `/queues`
- **THEN** each of the eight column headers is a link that sorts by that column

#### Scenario: Numeric order
- **WHEN** three queues have 9, 10 and 100 pending messages and the client sorts by Pending ascending
- **THEN** the rows are in the order 9, 10, 100

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

## REMOVED Requirements

### Requirement: Manual theme choice
**Reason**: The theme must be an automatic choice: the console always follows the light/dark setting of the operating system or browser.
**Migration**: Change the theme in the operating system or browser settings. `GET /theme/{mode}` is removed (it answers `404`) and an existing `mqrust_theme` cookie is ignored.
