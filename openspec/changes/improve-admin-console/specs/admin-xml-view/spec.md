## ADDED Requirements

### Requirement: XML detection
The message page SHALL treat a body as XML when it is shown as text (a TextMessage, after inflating a compressed body) and its text, after skipping a byte-order mark and leading whitespace, starts with `<?xml`, `<!--`, `<!DOCTYPE` or `<` followed by a letter or `_`, and the whole text is well-formed. Detection SHALL run only when the message page is rendered, never on the message path.

#### Scenario: XML text
- **WHEN** a pending TextMessage contains `<?xml version="1.0"?><order id="7"><item qty="2">A</item></order>`
- **THEN** its message page offers the "Raw" and "Formatted" views

#### Scenario: Plain text
- **WHEN** a pending TextMessage contains `hello <world>`
- **THEN** the message page shows only the text, with no view switch

#### Scenario: Malformed XML
- **WHEN** a pending TextMessage contains `<order><item></order>`
- **THEN** the message page shows the raw text and a notice "Not well-formed XML" with the line and column of the first error, and no "Formatted" view

### Requirement: Raw and formatted views
For an XML body the message page SHALL offer two views selected by links, with no JavaScript: "Raw" (the default, the text exactly as stored, as today) and "Formatted" (`?view=xml`). The chosen view SHALL be kept by the auto-refresh link. Both views SHALL be HTML-escaped.

#### Scenario: Switch view
- **WHEN** an authenticated client opens the message page of an XML body and follows "Formatted"
- **THEN** the URL carries `view=xml`, the formatted body is shown, and a "Raw" link returns to the stored text

#### Scenario: Refresh keeps the view
- **WHEN** the formatted view is open with `refresh=5`
- **THEN** the refreshed page still shows the formatted view

### Requirement: Formatting rules
The formatted view SHALL put each element on its own line, indented by two spaces per nesting level; keep the XML declaration, processing instructions, comments and CDATA sections, each on its own line; keep attribute order and values unchanged; keep an element whose content is only text on one line (`<item qty="2">A</item>`); keep the text of mixed-content elements unchanged; and drop whitespace-only text between elements. Entity and character references SHALL be shown as written, not expanded. Formatting SHALL NOT change the stored message, and a consumer SHALL receive it byte-for-byte as sent.

#### Scenario: Indented output
- **WHEN** the body is `<order id="7"><item qty="2">A</item><item qty="1">B</item></order>`
- **THEN** the formatted view shows
  ```
  <order id="7">
    <item qty="2">A</item>
    <item qty="1">B</item>
  </order>
  ```

#### Scenario: Content preserved
- **WHEN** the body contains a comment, a CDATA section with `<b>&amp;</b>` and the reference `&#233;`
- **THEN** the formatted view shows the comment, the CDATA section and `&#233;` exactly as written

#### Scenario: Message unchanged
- **WHEN** the formatted view of a message has been opened and the message is then consumed
- **THEN** the consumer receives the original text byte-for-byte

### Requirement: Safe parsing
The XML formatter SHALL NOT read DTDs, resolve external entities or fetch any resource; a `<!DOCTYPE>` SHALL be shown as written and its internal subset SHALL NOT define entities that are expanded. Formatting SHALL be limited to bodies of at most 1 MB of text; above that the page SHALL show the raw view with the notice "Too large to format (limit 1 MB)". The formatted output SHALL be truncated at 256 KB with a visible notice. Nesting deeper than 256 levels SHALL stop formatting and show the raw view with a notice.

#### Scenario: External entity not resolved
- **WHEN** the body is `<!DOCTYPE x [<!ENTITY e SYSTEM "file:///C:/Windows/win.ini">]><x>&e;</x>`
- **THEN** the formatted view shows `&e;` literally and the broker opens no file and no network connection

#### Scenario: Entity expansion bomb
- **WHEN** the body is a "billion laughs" document of 1 KB
- **THEN** the page renders the text without expanding entities and the request completes in under one second

#### Scenario: Large body
- **WHEN** a pending XML TextMessage has 2 MB of text
- **THEN** the page shows the raw view (first 64 KB) and "Too large to format (limit 1 MB)"

### Requirement: Syntax colouring
The formatted view SHALL mark element names, attribute names, attribute values, comments, CDATA sections and processing instructions with CSS classes, coloured by the stylesheet with readable contrast (at least 4.5:1 against the background) in both the light and the dark theme.

#### Scenario: Classes in output
- **WHEN** the formatted view of `<a b="c"/>` is rendered
- **THEN** the element name, attribute name and attribute value are each wrapped in an element with its own CSS class

### Requirement: Formatted view in the API
The JSON API SHALL add `GET /api/queues/{name}/messages/{id}`, returning the headers, properties and rendered body of one pending message as on the message page (`404` when it is no longer pending). With `?view=xml` it SHALL add a field `formattedBody` holding the formatted text (without colouring markup), or `null` with a field `formatError` when the body is not XML, malformed or too large.

#### Scenario: API formatted body
- **WHEN** a script requests the message of the indented-output scenario with `view=xml`
- **THEN** `formattedBody` holds the four indented lines separated by `\n`
