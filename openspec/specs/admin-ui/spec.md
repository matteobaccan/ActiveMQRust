# admin-ui Specification

## Purpose
Defines the look and usability of the admin console: light and dark themes following the operating system or a manual choice, readable contrast, responsive and accessible layout, sortable queues table, footer and a single version shown everywhere.
## Requirements
### Requirement: Theme follows the operating system
The stylesheet SHALL define its colours as CSS custom properties for a light and a dark palette and SHALL select the palette with `@media (prefers-color-scheme: dark)`, so the console follows the light/dark setting of the operating system or browser with no JavaScript. Pages SHALL declare `<meta name="color-scheme" content="light dark">` so form controls and scrollbars match the theme. When the system setting changes while a page is open, the page SHALL switch theme without a reload.

#### Scenario: Dark system
- **WHEN** a browser with dark mode active opens `/`
- **THEN** the page uses the dark palette (dark background, light text)

#### Scenario: Light system
- **WHEN** a browser with light mode active opens `/`
- **THEN** the page uses the light palette

### Requirement: Manual theme choice
Every page SHALL offer a theme selector with three choices: Automatic (default, follows the system), Light and Dark. Choosing one SHALL request `GET /theme/{auto|light|dark}?next=<path>`, which sets a cookie `mqrust_theme` (`SameSite=Strict`, `Path=/`, one year) and redirects to `next` under the same rules as the login redirect. Pages SHALL render `<html data-theme="light|dark">` when the cookie forces a theme, and the stylesheet SHALL let `data-theme` win over the media query. The login page SHALL honour the choice too. Changing the theme SHALL NOT change broker state.

#### Scenario: Force dark on a light system
- **WHEN** a browser in light mode chooses Dark
- **THEN** every following page, including `/login`, is rendered with `data-theme="dark"` and the dark palette

#### Scenario: Back to automatic
- **WHEN** the client then chooses Automatic
- **THEN** the cookie is removed and pages follow the system setting again

### Requirement: Readable contrast in both themes
Text, links, table borders, status badges and focus outlines SHALL have a contrast ratio of at least 4.5:1 for text and 3:1 for non-text elements against their background, in both palettes. Status SHALL never be conveyed by colour alone: badges also carry a word (for example "expired", "in flight").

#### Scenario: Contrast check
- **WHEN** the palettes are checked with a contrast calculator
- **THEN** every text/background pair defined in the stylesheet reaches 4.5:1 in both themes

### Requirement: Responsive layout
Pages SHALL be usable without horizontal page scrolling from 360 px to 2560 px wide, with `<meta name="viewport" content="width=device-width, initial-scale=1">`. Below 768 px the navigation SHALL collapse into a menu opened by a `<details>` element (no JavaScript); the overview figures SHALL be cards that wrap into one column on narrow screens and a grid on wide screens; wide tables SHALL scroll horizontally inside their own container; message IDs and long destination names SHALL wrap or be shortened with the full value available as the link target or title. Touch targets SHALL be at least 40×40 px.

#### Scenario: Phone width
- **WHEN** `/queues` is opened at 360 px wide with 20 queues whose names are 80 characters long
- **THEN** the page has no horizontal scroll bar, the navigation is a collapsed menu, and the queue table scrolls inside its own box

#### Scenario: Wide screen
- **WHEN** `/` is opened at 1920 px wide
- **THEN** the overview cards are laid out in a grid of several columns and the content width is limited for readability

### Requirement: Modern visual design
The console SHALL use a consistent visual system defined once in the stylesheet: the system font stack (`system-ui`, `Segoe UI`, …) and a monospace stack for IDs and bodies; spacing, radius and colour tokens; a top bar with the product name, the navigation, the theme selector, the logged-in user and logout; cards for overview figures; tables with sticky headers, zebra rows and right-aligned numbers; badges for states; visible keyboard focus. No external fonts, images, scripts or stylesheets SHALL be loaded; everything stays embedded in the executable and the `Content-Security-Policy` stays `default-src 'none'; style-src 'self'` plus `form-action 'self'` and `frame-ancestors 'none'`. The stylesheet SHALL stay under 20 KB.

#### Scenario: No external resources
- **WHEN** any console page is loaded with the network to the internet blocked
- **THEN** it renders completely and the browser makes no request outside the console's host

#### Scenario: Keyboard use
- **WHEN** an operator moves through a page with the Tab key
- **THEN** every link, button and form field shows a visible focus outline

### Requirement: Accessible markup
Pages SHALL use semantic HTML: one `<h1>` per page, `<nav>` for navigation, `<main>` for content, `<th scope>` on table headers, `<label>` on every form field, and `lang="en"` on `<html>`. The login form SHALL work with password managers.

#### Scenario: Labels
- **WHEN** the login page is checked with an accessibility checker
- **THEN** the username and password fields each have an associated label and no error is reported

### Requirement: Sortable queues table
Every column of the queues table SHALL be sortable by clicking its header: Name, Pending, Inflight, Consumers, Producers, Enqueued, Consumed and Expired, and any column added later. Sorting SHALL be done server-side through `sort=<column>&order=asc|desc`, with no JavaScript; the default is Name ascending. Numeric columns SHALL sort numerically and Name SHALL sort case-insensitively; rows with equal values SHALL keep a stable order by name ascending. The sorted column SHALL show an arrow for its direction and carry `aria-sort`; clicking it again SHALL reverse the direction, clicking another column SHALL sort that column ascending. The sort SHALL be kept by auto-refresh and by the theme selector, and an unknown `sort` value SHALL fall back to Name. `/api/queues` SHALL accept the same `sort` and `order` parameters.

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

### Requirement: Footer
Every console page, including the login page, SHALL end with a footer showing the project name with its version (`ActiveMQRust <version>`), the link to the project repository `https://github.com/matteobaccan/ActiveMQRust` (opened with `rel="noopener noreferrer"`), and the text `by Matteo Baccan`. The repository URL SHALL come from the crate metadata (`CARGO_PKG_REPOSITORY`), not be typed in the page code. The footer SHALL follow the theme and the responsive layout, wrapping onto several lines on narrow screens.

#### Scenario: Footer content
- **WHEN** any console page or `/login` is rendered
- **THEN** its footer contains `ActiveMQRust 0.1.0` (the crate version), a link to `https://github.com/matteobaccan/ActiveMQRust` and `by Matteo Baccan`

#### Scenario: Footer on a phone
- **WHEN** a page is opened at 360 px wide
- **THEN** the footer is fully visible without horizontal scrolling

### Requirement: One version everywhere
The broker version SHALL have a single source, the `version` in `Cargo.toml`, and the same value SHALL appear in: the admin footer of every page (`ActiveMQRust <version>`), the overview page, the `version` field of `/api/overview`, the OpenWire `WireFormatInfo` property `ProviderVersion` sent to every client (with `ProviderName = ActiveMQRust`), the output of `mqrust.exe --version`, the start-up log, and the Windows file version of `mqrust.exe`. No other place in the code SHALL contain the version number as a literal.

#### Scenario: Version 0.2.0 everywhere
- **WHEN** `Cargo.toml` says `version = "0.2.0"` and the broker is built and started
- **THEN** the admin footer shows `ActiveMQRust 0.2.0`, `/api/overview` returns `"version":"0.2.0"`, `mqrust.exe --version` prints `0.2.0`, and a Java client reads `ProviderVersion = 0.2.0` from the broker's `WireFormatInfo`

#### Scenario: No stray literals
- **WHEN** the source tree is searched for the version string
- **THEN** it is found only in `Cargo.toml` (and `Cargo.lock`)

