## MODIFIED Requirements

### Requirement: Server-side HTML with embedded assets
Pages SHALL be generated server-side as HTML, with CSS embedded in the executable and no JavaScript. Every value taken from broker data (destination names, IDs, properties, bodies, usernames) SHALL be HTML-escaped. Responses SHALL carry `Content-Security-Policy: default-src 'none'; style-src 'self'; form-action 'self'; frame-ancestors 'none'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin` and, on pages behind login, `Cache-Control: no-store`. Every page behind login SHALL show the logged-in username and a logout button in its top bar. Every page SHALL support optional auto-refresh every 5 seconds, enabled with the query parameter `refresh=5` and kept on the page's links.

#### Scenario: Escaped content
- **WHEN** a queue named `Q<script>` holds a TextMessage whose text and a property value are `<script>alert(1)</script>`
- **THEN** the queues page, the queue detail page and the message page contain the escaped text `&lt;script&gt;` and no `<script>` element

#### Scenario: Auto-refresh
- **WHEN** a logged-in client requests `/queues?refresh=5`
- **THEN** the page contains `<meta http-equiv="refresh" content="5">` and the links on the page keep `refresh=5`

#### Scenario: No asset files on disk
- **WHEN** the broker runs from a folder that contains only `mqrust.exe`
- **THEN** every console page renders with its stylesheet

#### Scenario: User and logout in the top bar
- **WHEN** a client logged in as `admin` opens any page
- **THEN** the top bar shows `admin` and a logout button

#### Scenario: Pages not cached
- **WHEN** a logged-in client requests `/queues`
- **THEN** the response carries `Cache-Control: no-store`
