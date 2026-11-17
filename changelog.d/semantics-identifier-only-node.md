### Fixed

- A `Semantics` container named only by its identifier is published as a group, so `find::id`, the
  agent and OS automation clients (UI Automation's `AutomationId`, `AXIdentifier`, AT-SPI) find it:
  it was a generic container, which AccessKit's filter drops with its identifier. Inside a button,
  checkbox, radio button, link or menu item it stays generic and the control carries its
  identifier, so the control keeps the name its label gives it.
