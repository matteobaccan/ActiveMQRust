## Context

The console applies the dark palette under `@media (prefers-color-scheme: dark)` for `:root:not([data-theme="light"])` and under `:root[data-theme="dark"]`; a `GET /theme/{mode}` route sets the `mqrust_theme` cookie and pages render `data-theme` from it.

## Goals / Non-Goals

**Goals:** the theme follows the operating system or browser only; nothing for the user to choose; no theme cookie.

**Non-Goals:** changing the palettes, contrast or layout.

## Decisions

1. **Stylesheet**: dark tokens only under `@media (prefers-color-scheme: dark) { :root { … } }`; remove every `[data-theme]` selector. Keep `<meta name="color-scheme" content="light dark">`.
2. **Server**: remove the `/theme/{mode}` route (it then answers 404 through the normal fallback), the selector markup in the top bar and on `/login`, and the cookie handling; ignore an existing `mqrust_theme` cookie.
3. **Links**: sort and refresh links no longer carry theme state (they never carried a parameter; only the cookie existed).

## Risks / Trade-offs

- [A user who forced a theme loses that choice] → They change the theme in the operating system or browser; documented in the README.
