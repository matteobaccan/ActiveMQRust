## Context

`message_detail()` in `src/admin/pages.rs` renders either the raw body or the formatted XML, chosen by `?view=xml`; the switch is `<nav class="views">` with two links built by `Ctx::with("view", …)`. The CSP is `default-src 'none'; style-src 'self'; form-action 'self'; frame-ancestors 'none'`, so no script can run. Auto-refresh is a `<meta http-equiv="refresh" content="5">` that reloads the current URL.

## Goals / Non-Goals

**Goals:**
- Instant switch between Raw and Formatted, no request, scroll position kept.
- Keep the console free of JavaScript and the CSP unchanged.
- Links with `?view=xml` keep opening the formatted view.

**Non-Goals:**
- Updating the URL when the view is switched without a reload.
- Remembering the chosen view across messages.

## Decisions

### D1. Both views in the page, CSS radio switch
When the body is formatted XML, the page contains both `<pre class="body">` (raw) and `<pre class="body xml">` (formatted), each in a panel, preceded by a `<fieldset class="views">` with a `<legend>` "Body view" (visually hidden) and two `<input type="radio" name="body-view">` with their `<label>`s "Raw" and "Formatted". CSS (`.views input:checked` + sibling selectors on the panels) shows only the panel of the checked input. The checked input at load time is "Formatted" when `view=xml`, otherwise "Raw".
*Alternative 1*: a small script with `script-src 'self'` that swaps the views and updates the URL with `history.replaceState`. Rejected: it would be the first JavaScript in the console and would loosen the CSP for one switch.
*Alternative 2*: `<details>`/`<summary>` for the formatted view. Rejected: it shows both views at once when open and does not read as a two-option switch.
*Alternative 3*: keep the links. Rejected: that is the reported problem.

### D2. Accessibility
Native radio inputs give keyboard arrows, focus and the "1 of 2, selected" announcement for free. The inputs are visually hidden but focusable; the label of the focused input gets the existing focus ring; the label of the checked input gets the current "selected" style (accent background). The hidden panel uses `display: none`, so screen readers read only the visible view.
*Alternative*: ARIA tabs (`role="tablist"`). Rejected: correct tab behaviour needs JavaScript.

### D3. Links when auto-refresh is on
When `refresh=5` is active, the switch stays the current pair of links (`view` in the URL), because the page is reloaded every 5 seconds and a CSS-only choice would be lost at each refresh. Only the selected view is rendered in that case, as today.
*Alternative*: radios also with auto-refresh. Rejected: the view would jump back every 5 seconds.

### D4. Page size
Both views are sent only for bodies that have a formatted view. The raw part is the same as today; the formatted part is already capped at 256 KB with its truncation notice, which is placed inside the formatted panel.
*Alternative*: fetch the second view lazily. Rejected: impossible without JavaScript.

## Risks / Trade-offs

- [The URL does not show the view chosen without a reload] → accepted; "Formatted" can still be linked with `?view=xml`, and the switch with auto-refresh keeps the URL in sync.
- [Larger page for big XML bodies] → bounded by the existing 256 KB cap of the formatted view.
