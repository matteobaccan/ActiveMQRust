## Why

The admin console offers a theme selector (Automatic, Light, Dark). The theme must be an automatic choice, not the user's: the console should simply follow the light/dark setting of the operating system or browser, with nothing to configure and nothing stored.

## What Changes

- **BREAKING (console only)**: the theme selector is removed from every page and from the login page; `GET /theme/{mode}` is removed; the `mqrust_theme` cookie is no longer set and an existing one is ignored; `<html>` never carries a forced `data-theme`.
- The palette is chosen only by `@media (prefers-color-scheme: dark)`, as today for the Automatic choice.
- Sort, refresh and other links no longer need to preserve a theme parameter.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `admin-ui`: "Theme follows the operating system" (the only way to choose the theme), "Modern visual design" and "Sortable queues table" (no theme selector); "Manual theme choice" is removed.
- `admin-console`: "Server-side HTML with embedded assets" (no theme selector in the top bar).

## Impact

- `src/admin/pages.rs` and `src/admin/mod.rs` (selector, `/theme` route, cookie and `data-theme` rendering removed), `src/admin/style.css` (`[data-theme]` rules removed).
- `tests/admin_http.rs`: theme tests replaced by a check that no selector, cookie or `data-theme` is present and that `/theme/dark` answers 404.
