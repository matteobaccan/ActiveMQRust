## Context

The overview page renders a heading and a grid of cards (`.cards`), one per figure; today two of the cards hold the OpenWire address and the admin URL.

## Goals / Non-Goals

**Goals:** addresses readable at a glance, on their own lines, without wrapping inside narrow cards; cards only for figures.

**Non-Goals:** changing the JSON API, the other pages or the card layout.

## Decisions

1. **Markup.** A `<p class="addresses">` right after the `<h1>`, with two lines (`<span>` per line, separated by `<br>` or as block elements): `OpenWire <code>tcp://host:port</code>` and `Admin console <code>http://host:port</code>`. Values stay HTML-escaped.
2. **Style.** Muted label colour, monospace value, `overflow-wrap: anywhere` so long addresses wrap on phones; both themes via the existing colour tokens.
3. **Test.** `tests/admin_http.rs` asserts that both addresses appear after the `<h1>` and before the cards container, and that no card contains them.

## Risks / Trade-offs

- None significant; purely presentational.
