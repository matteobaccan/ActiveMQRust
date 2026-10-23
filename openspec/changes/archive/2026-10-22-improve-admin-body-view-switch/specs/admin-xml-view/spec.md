## MODIFIED Requirements

### Requirement: Raw and formatted views
For an XML body the message page SHALL offer two views, with no JavaScript: "Raw" (the text exactly as stored, as today) and "Formatted". When auto-refresh is off, the page SHALL contain both views and a switch made of two labelled radio buttons grouped under the name "Body view"; selecting one SHALL show its view and hide the other without sending any request, reloading the page or changing the scroll position. The view selected when the page opens SHALL be "Formatted" when the URL carries `view=xml`, otherwise "Raw". When auto-refresh is on, the switch SHALL be a pair of links that set or remove `view=xml`, the page SHALL contain only the selected view, and the auto-refresh SHALL keep that view. Both views SHALL be HTML-escaped. Only the visible view SHALL be exposed to assistive technologies.

#### Scenario: Switch without reload
- **WHEN** auto-refresh is off and an authenticated client opens the message page of an XML body and selects "Formatted"
- **THEN** the formatted body is shown, no HTTP request is sent, and selecting "Raw" shows the stored text again

#### Scenario: Both views in one response
- **WHEN** an authenticated client requests the message page of an XML body without `refresh`
- **THEN** the single response contains the raw text and the formatted view, two radio inputs labelled "Raw" and "Formatted", and "Raw" is checked

#### Scenario: Link to the formatted view
- **WHEN** an authenticated client opens the message page with `view=xml` and auto-refresh off
- **THEN** "Formatted" is checked and the formatted body is the visible one

#### Scenario: Keyboard
- **WHEN** the focus is on the "Raw" option and the user presses the right arrow key
- **THEN** "Formatted" becomes selected and visible, with a visible focus indicator

#### Scenario: Refresh keeps the view
- **WHEN** the formatted view is open with `refresh=5`
- **THEN** the switch is made of links, and the refreshed page still shows the formatted view
