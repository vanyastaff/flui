### Changed

- **`flui-runtime`**, **`flui-engine`**: the performance overlay's labels are shaped through the
  realm's text context at scene assembly, over the app's font collection, and the engine replays
  them; it no longer shapes text or builds a font collection of its own, so an overlay frame adds
  no face to the glyph registry.
- **`flui-rendering`**: `TextContextHandle::try_with` runs a closure on the realm's text context
  outside layout, returning `None` while it is lent.

### Removed

- **`flui-runtime`**: `PerformanceStats::total_frames`, whose only reader was the overlay's
  removed frame counter.
