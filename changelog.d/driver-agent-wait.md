### Added

- **Waits through the development agent** (`flui-view`, `flui-runtime`, `flui-semantics`;
  ADR-0238): `AgentWindow::wait` holds out for a condition on the window's committed tree
  (`Appears`, `Gone`, `Matches`, `Settled`, `Frames`, `All`, `Any`) without polling. It answers at
  the frame that makes the condition hold, or at the deadline on the window's clock with
  `satisfied: false`, the last observation and why the window had not settled. A window admits
  256 waits and answers the next `busy`; a closing window answers its waits `gone`.
  `UiRuntime::next_wake` counts a parked wait's deadline.
