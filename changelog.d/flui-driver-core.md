### Added

- `flui-driver` (Evolving, `0.1`): one API that drives a running FLUI app (ADR-0238). `Driver`
  launches an app (`Launch::executable`, `Launch::cargo_example`, `Launch::headless` behind the
  `headless` feature) or attaches to one, and reaches every method of the protocol's table by type
  (`Driver::call`) or by wire name (`Driver::call_value`). `Window`, `ElementLocator` and `Element`
  locate by semantics and compose the gestures (`tap`, `double_tap`, `long_press`, `drag_to`,
  `swipe`, `fling`, `pinch`, `scroll_into_view`, `type_text`, `press_keys`) from the agent's
  locate, actionability and input methods; `expect` and `expect_soft` are retrying assertions on
  the app's frame timeline. Failures are typed (`Error`, one variant per wire code), and a
  scenario ends in a `Report` with a `pass`, `fail` or `cannot_verify` verdict, exit codes 0, 1
  and 2, and a final `FLUI_DRIVER_VERDICT=` line.
- OS input in `flui-driver` is never the default: the session's policy is synthetic, a fallback
  never takes the OS path, and OS input through a `Lane` is announced before its first event,
  watched by the lane's `ActivityGuard` (the first foreign event stops the run before its next
  event, releases everything and ends it `cannot_verify`), refused on a shared desktop by a lane
  without a guard, and held to one session per host by `OsInputLock`. A scenario fixes its window
  (`WindowSpec`) and refuses a window of another size or scale; a lane receives every move as a
  single step followed by a pause (`steps::os_events`).
- `flui-testing`: `HeadlessAgentService` and `HeadlessConversation` serve the driver agent's
  conversation in process, polled at the instants of the caller's clock.
- `flui-protocol`: `Placement::new` and its `visible`, `focused` and `position` builders.

### Changed

- The app's agent answers `window.placement`: the window's logical size and scale as its root
  media query holds them, and whether it is shown and focused. The position is left out until an
  OS lane locates windows.
