### Changed

- **`flui-widgets`**: `CollapsingHeader::collapse` and `CollapsingHeader::expand` are renamed
  `collapses` and `expands`; they configure when a scroll moves the header and do not move it.
- **`flui-widgets`**: `RefreshIndicator`'s default spinner takes `ActivityIndicator`'s own
  colour instead of a hard-coded Material blue.

### Removed

- **`flui-widgets`**: `PageSnap::velocity_tolerance`, `AnchorSnap::velocity_tolerance`,
  `DraggableAnchors::iter` and `TryFrom<f64> for ViewportFraction`; use
  `ViewportFraction::new`.

### Fixed

- **`flui-widgets`**: a nested scroll participant that panics when an `InteractiveViewer`
  contact lifts no longer leaves the sequence unended: the viewer's inertia still runs as its
  motion, every other participant hears its end, and the failure resumes afterwards.
- **`flui-widgets`**: a `CollapsingHeader` counts itself on an extent within half a device
  pixel at every device pixel ratio, as the scroll physics do; below a ratio of one it was
  held to half a logical pixel.
