### Added

- **`HeadlessHost::defer_presents`** (`flui-testing`): defers a headless host's next submits, as
  a surface not ready to show its window does, so a test can deliver pointer input that waits for
  the first presented frame.

### Fixed

- **Turning haptics off cancels queued feedback** (`flui-interaction`): a frame-aligned haptic
  queued while haptics were on no longer plays on the next presented frame after the user turns
  them off; turning them off resolves every queued request to `HapticOutcome::Suppressed` at
  once, delivered to the presentation's observers, as hiding the window does
  ([ADR-0212](/docs/adr/ADR-0212-system-preferences-for-input.md)).
- **Recordings keep the sequence of input held for a commit** (`flui-runtime`, `flui-testing`):
  pointer input that arrived before its presentation's tree was committed was recorded at arrival
  with no `InputSequenceId`, so its rows could not be joined with its verdicts. Its row is now
  recorded where the frame dispatches it, with its arrival time and the sequence the dispatch
  placed it in, ahead of that dispatch's decision rows; held input the runtime discards instead is
  recorded where it is discarded as `InputOutcome::Queued` in no sequence, and
  `HeadlessHost::replay` does not apply it
  ([ADR-0215](/docs/adr/ADR-0215-input-recording-and-backend-conformance.md)).
