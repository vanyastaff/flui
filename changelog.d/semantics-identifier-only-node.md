### Fixed

- A `Semantics` container named only by its identifier is published as a group, so `find::id`
  and every OS automation client (UI Automation's `AutomationId`, `AXIdentifier`) find it: it was a
  generic container, which AccessKit's filter drops with its identifier.
