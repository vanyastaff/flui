### Added

- **Locate and actionability through the development agent** (`flui-view`, `flui-runtime`;
  ADR-0238): `AgentWindow::locate` resolves a locator over the window's committed semantics tree,
  and `AgentWindow::actionability` reads an element's attached, visible, stable, enabled and
  receives facts, the render hit-test run in the same owner turn outside every semantics borrow.
  `SemanticsAgent` has the same two calls. A hit-test that cannot run because the render tree is
  busy answers `busy`; a render object that panics in it answers `platform` before the panic
  continues on the owner.
