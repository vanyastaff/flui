### Changed

- **`DragReach` and `DragMoment` are `#[non_exhaustive]`** (`flui-platform-api`): a match on
  either outside the crate needs a wildcard arm.

### Fixed

- **A window's held input is bounded** (`flui-platform`): a window whose input callback was never
  registered, or whose capability snapshot failed every pass, held every later event until it
  closed. It now holds at most 256 entries, evicting the oldest ordinary input, and refuses new
  input with the default result while no registration can deliver it.
- **Win32 pen and touch drags stay in their window** (`flui-platform`): escalating a drag that no
  mouse button holds, or that a pen or touch contact makes, started a `DoDragDrop` session that
  cancelled at once. The start now fails with `DragError::Unavailable`, so the drag stays
  in-window (ADR-0211 §11).
- **A panicking drag provider no longer aborts on Win32** (`flui-platform`): the session's
  provider is retired under containment when COM releases the data object.
- **Win32 COM objects are no longer agile** (`flui-platform`): the drag objects, the
  DirectManipulation handler and the TSF text store hold owner-thread state, so they no longer
  answer `IAgileObject` or aggregate the free-threaded marshaler, and another apartment reaches
  them through a proxy.
- **A foreign drag's format list is read to a bound** (`flui-platform`): an enumerator that never
  ends no longer holds `DragEnter` and its source.
- **Pen haptics never play late in a burst** (`flui-platform`): requests made while a controller
  call stalled played back to back afterwards. Only the newest pending request plays, and a
  waveform reached more than two frames late is skipped.
