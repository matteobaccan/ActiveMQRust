## 1. Page

- [ ] 1.1 `message_detail`: without auto-refresh, render both panels (raw and formatted, truncation notice inside the formatted panel) and the radio fieldset, checked from `view`
- [ ] 1.2 With auto-refresh, keep the current links and render only the selected view
- [ ] 1.3 Styles: segmented control on visually hidden radio inputs, `:checked` rules for the panels, focus ring on the label, both themes

## 2. Tests

- [ ] 2.1 `tests/admin_http.rs`: one response holds both views and two radios with "Raw" checked; `view=xml` checks "Formatted"; with `refresh=5` the switch is links and only one view is present
- [ ] 2.2 Check by hand in a browser: instant switch, scroll position kept, keyboard arrows, screen reader reads the visible view only

## 3. Documentation

- [ ] 3.1 README: admin console section describes the switch
- [ ] 3.2 CHANGELOG entry under Unreleased
- [ ] 3.3 `cargo fmt` and the test suite
