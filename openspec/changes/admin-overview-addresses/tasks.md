## 1. Overview page

- [ ] 1.1 Move the OpenWire address and the admin URL out of the cards into two labelled lines directly below the page title; keep only figures in the cards
- [ ] 1.2 Add the style for the address lines (muted label, monospace value, wrapping on narrow screens, both themes)
- [ ] 1.3 Test in `tests/admin_http.rs`: addresses after the `<h1>` and before the cards, not inside any card
- [ ] 1.4 `cargo test --release` passes and `openspec validate admin-overview-addresses` passes
