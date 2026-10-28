## ADDED Requirements

### Requirement: Messages in memory
The overview page SHALL show a card "Messages in memory" with the number of pending plus inflight messages over every destination, where a topic message counts once for each subscription that holds it, computed when the page is requested; `/api/overview` SHALL report the same value as `messagesInMemory`. Computing it SHALL NOT add any work to storing or delivering messages.

#### Scenario: Queue messages
- **WHEN** 300 messages are pending on a queue with no consumers and nothing else is stored
- **THEN** the overview shows "Messages in memory" 300

#### Scenario: Topic copies
- **WHEN** one message is published to a topic with three subscribers that have not yet received it
- **THEN** "Messages in memory" increases by 3

#### Scenario: Consumed
- **WHEN** the 300 queue messages are consumed and acknowledged
- **THEN** "Messages in memory" returns to its previous value

### Requirement: Memory history
While the admin console is enabled, the broker SHALL record the last hour of message memory and process memory (Working Set and Private Bytes on Windows; resident size and physical footprint on macOS) in 360 buckets of 10 seconds, by a task that runs outside the message path. Message memory SHALL be read every second, and each bucket SHALL hold the minimum, maximum and last of its readings; process memory SHALL be read once per bucket. The history SHALL use fixed memory, SHALL start empty at each broker start, and SHALL include the bucket still in progress. Recording SHALL NOT change the code that stores, delivers or releases messages.

#### Scenario: Peak of a few seconds
- **WHEN** message memory rises from 10 MB to 400 MB, stays there for 3 seconds and falls back to 10 MB
- **THEN** the bucket covering that moment has a maximum of at least 400 MB

#### Scenario: Older than one hour
- **WHEN** the broker has been running for two hours
- **THEN** the history holds exactly 360 closed buckets plus the one in progress, the oldest starting about one hour ago

#### Scenario: Recent start
- **WHEN** the broker started 5 minutes ago
- **THEN** the history holds only the buckets of those 5 minutes

### Requirement: Memory charts on the overview
The overview page SHALL show, below the cards, two charts of the last hour drawn as inline SVG without JavaScript: message memory in MB (minimum–maximum band and last-value line, plus a dashed "limit" line when `max_memory_mb` is set) and process memory in MB (Working Set and Private Bytes lines). Each chart SHALL have a time axis covering 60 minutes with local-time ticks every 15 minutes, a value axis from 0, a legend, and the text "Peak <value> at <HH:MM:SS>" giving the highest value of the hour and when it was recorded. Periods without data SHALL be left empty. Each bucket SHALL show its time and values as a native tooltip. The charts SHALL scale with the page width without horizontal scrolling at 360 px, use theme colours in light and dark mode, distinguish series by line style as well as colour, carry `role="img"` with an `aria-label` summarising current value, peak and limit, and be followed by a collapsible "Data" table of the buckets. The overview's auto-refresh SHALL redraw them.

#### Scenario: Peak shown
- **WHEN** message memory reached 410 MB at 10:42:10 and is now 120 MB
- **THEN** the message memory chart shows "Peak 410 MB at 10:42:10" and its `aria-label` mentions now 120 MB and the peak

#### Scenario: Limit line
- **WHEN** `max_memory_mb = 1024`
- **THEN** the message memory chart has a dashed line at 1024 MB labelled "limit"; with `max_memory_mb = 0` there is no limit line

#### Scenario: No JavaScript
- **WHEN** the overview page is loaded
- **THEN** the response contains `<svg` elements and no `<script>` element, and the CSP header is unchanged

#### Scenario: Narrow screen
- **WHEN** the overview is opened at 360 px wide
- **THEN** the charts fit the width with no horizontal scroll bar

### Requirement: History API
`GET /api/memory/history` SHALL return, with the console's authentication, `intervalSeconds` (10), `limitBytes` (the message memory limit, or `null`), and `buckets` oldest first, each with `start` (RFC 3339), `messageMemory` (`min`, `max`, `last`), `workingSet` and `privateBytes` in bytes. The bucket in progress SHALL be the last one.

#### Scenario: History as JSON
- **WHEN** a script requests `/api/memory/history` after the broker has run for 30 seconds
- **THEN** the response holds 3 or 4 buckets with increasing `start` times and the fields above
