### Changed

- **Assistive requests take one path** (`flui-runtime`): `PresentationDispatcher::dispatch` queues
  `HostInput::Accessibility` in the runtime's command inbox, as the accessibility bridge does, and
  answers `InputOutcome::Queued`; the request runs at the next owner turn's Idle drain. A full
  inbox refuses it with the new `DispatchError::InboxFull` (ADR-0207, amendment). A frame drains
  the inbox before it records and renders, so a recording lists a queued request before the frame
  it preceded; a panicking handler wakes the owner for the requests behind it, and a request whose
  presentation closed first is dropped with a warning.
- **Recordings mark dropped assistive requests** (`flui-runtime`): an AT request no handler ran
  for is recorded `InputOutcome::Unhandled` instead of `Handled`.

### Fixed

- **A failed window move kept its effects** (`flui-runtime`): when the input-region backend or the
  gesture-geometry refresh panics during a window move, the window's state subscribers still
  receive the new state, the recording keeps the move's row, and the committed input regions are
  no longer lost, so the next move hands the backend the same regions again.
- **Finishing a recording inside a tap call** (`flui-runtime`): rows the running delivery pass had
  not offered yet are counted in the final `Gap` and `End` rows instead of being dropped while
  `End` reported none lost.

### Removed

- `UiRuntime::input_capabilities`, which nothing read, and the test-only
  `UiRuntime::system_preferences` (`flui-runtime`). `UiRuntime::haptics` and
  `UiRuntime::window_moved` (hosts send `WindowObservation::Moved`) are crate-private, and
  `UiRuntime::drag_handle` is behind the `testing` feature.
