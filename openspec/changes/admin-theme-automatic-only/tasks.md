## 1. Console

- [ ] 1.1 Remove the theme selector from the top bar and from `/login`, the `/theme/{mode}` route, the `mqrust_theme` cookie handling and the `data-theme` rendering
- [ ] 1.2 Stylesheet: dark palette only under `@media (prefers-color-scheme: dark)`; remove the `[data-theme]` rules; contrast values unchanged
- [ ] 1.3 Tests in `tests/admin_http.rs`: no selector, no theme cookie, no `data-theme` on any page or `/login`; `/theme/dark` answers 404
- [ ] 1.4 README: the console follows the system theme; `cargo test --release` and `openspec validate admin-theme-automatic-only` pass
