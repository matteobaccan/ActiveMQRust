## 1. Overview page

- [x] 1.1 Move the OpenWire address and the admin URL out of the cards into two labelled lines directly below the page title; keep only figures in the cards
- [x] 1.2 Add the style for the address lines (muted label, monospace value, wrapping on narrow screens, both themes)
- [x] 1.3 Test in `tests/admin_http.rs`: addresses after the `<h1>` and before the cards, not inside any card
- [x] 1.4 `cargo test --release` passes and `openspec validate admin-overview-addresses` passes
