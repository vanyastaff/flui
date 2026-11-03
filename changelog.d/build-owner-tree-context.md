### Added

- Contained build panics carry the ancestor path and creation site of the view that panicked:
  `flui_view::RecoveredPanic::context` and the `context` field of
  `FrameFailureKind::RecoveredPanic`, with `ElementTree::tree_context` to read one for any element.
