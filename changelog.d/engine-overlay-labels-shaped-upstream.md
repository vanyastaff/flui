### Changed

- **`flui-layer`**: `PerformanceOverlayLayer` carries its readout as a recorded `DisplayList`
  instead of numbers. `PerformanceOverlayLayer::record(text, bounds, options, &PerformanceSample)`
  composes the fps, frame time and diagnostic line through the caller's `TextContext`, and
  `new(bounds, options, readout)` wraps a readout recorded elsewhere; `update_stats`, `fps`,
  `frame_time_ms`, `total_frames`, `diagnostic_line`, `set_diagnostic_line` and `all_stats` are
  removed ([ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md)).
- **`flui-runtime`**, **`flui-engine`**: the performance overlay's labels are shaped through the
  realm's text context at scene assembly, over the app's font collection; the engine clips the
  readout to the overlay's bounds and replays it, and no longer shapes text or builds a font
  collection of its own, so an overlay frame adds no face to the glyph registry.
- **`flui-rendering`**: `TextContextHandle::try_with` runs a closure on the realm's text context
  outside layout, returning `None` while it is lent.
