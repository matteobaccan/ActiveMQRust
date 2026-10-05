## Why

On the overview page of the admin console the OpenWire and admin listen addresses are shown inside the figure cards, together with counters such as connections and queues. They are not figures: they are where the broker can be reached, and in a card they are cramped (long URLs wrap) and look like metrics. They read better as plain text right under the page title.

## What Changes

- The overview page shows the OpenWire address and the admin console URL as two lines directly below the `<h1>`, outside the cards, each with a label (`OpenWire` and `Admin console`) and the address in the monospace font.
- The cards keep only figures: uptime, connections, queues, topics, message memory, process memory, compression counters.
- `/api/overview` is unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `admin-console`: "Overview page" states where the listen addresses are shown.

## Impact

- `src/admin/pages.rs` (overview page), possibly `src/admin/style.css` (a small rule for the address lines).
- `tests/admin_http.rs`: a check that the addresses appear under the title and not inside a card.
