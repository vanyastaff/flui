### Added

- **`flui-platform-api`**: `DropFeedback::resolve`, the one action a drop takes from a target's
  answer and the actions the source permits, and `TransferFormat::is_drag_identity`; every backend
  resolves its drops through them. `InputCapabilities::to_builder` changes one capability and
  keeps the others.

### Changed

- **`flui-native`**: `HeadlessDragAndDrop::start_foreign`, `hover` and `drop_here` return
  `Result<_, DragError>` instead of panicking when a test drives them out of order
  (`Unavailable` while a session already runs, `SessionClosed` when none does), and `drop_here`
  over no window cancels the drag instead of leaving it running.

### Fixed

- **`flui-native`**: a headless drop delivered every action the target and source shared
  (copy and move at once); it now delivers one, as Win32, the web and Android do.
- **`flui-native`**: a headless drag's provider that panicked was lost for every later request;
  the request now fails with `SourceGone` and the provider serves the next one. A provider's
  captures retire outside the double's lock, so they may call it.
- **`flui-native`**: an input callback replaced or cleared during a call that panicked, and a
  window's input callback destroyed by an unwind, are retained instead of dropped, so their
  captures never run inside a failed operation
  ([ADR-0127](/docs/adr/ADR-0127-exceptional-path-retention.md)).
