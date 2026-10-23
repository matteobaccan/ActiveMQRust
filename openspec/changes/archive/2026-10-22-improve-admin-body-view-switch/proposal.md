## Why

On the message page of an XML body, "Raw" and "Formatted" are links: every switch reloads the whole page from the broker (headers, properties, body, formatting). The reload is slow on large bodies, moves the scroll position back to the top, and can even land on "message no longer pending" when the message was consumed in the meantime. Switching between two views of a body that is already shown should be instant.

## What Changes

- The page sends both views of an XML body once, and the switch only changes which one is visible: no request, no reload, scroll position kept.
- Still no JavaScript (the CSP has no `script-src`): the switch is made of two radio buttons styled as the current segmented control, and CSS shows the selected view.
- `?view=xml` still opens the page on "Formatted", so links and bookmarks keep working.
- With auto-refresh on, the page is reloaded every 5 seconds anyway: there the switch stays a pair of links, so that the chosen view survives the refresh, as today.
- The API is unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `admin-xml-view`: "Raw and formatted views" switches without reloading the page when auto-refresh is off.

## Impact

- Code: `src/admin/pages.rs` (`message_detail`: both views in the page, radio inputs, links kept for auto-refresh), `src/admin/style.css` (segmented control on radio inputs, `:checked` rules, focus ring), tests in `tests/admin_http.rs`.
- No new crates, no JavaScript, CSP unchanged.
- The page carries both views: at most the raw body as shown today plus the formatted view, which is already capped at 256 KB.
