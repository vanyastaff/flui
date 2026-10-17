### Added

- **`flui-widgets`**: `CollapsingHeader`, a header above scrolling content that the content's
  scroll collapses and expands through the nested scroll channel
  ([ADR-0209](/docs/adr/ADR-0209-nested-scroll-connection.md)). `HeaderExtents` sets its
  collapsed and expanded extents (equal extents give a fixed header; invalid ones are a
  `HeaderExtentsError`), `HeaderOrder` says whether it collapses and expands before the
  content moves or with what the content leaves at its edge, and a `SnapPolicy` settles it on
  an extent. Flings collapse it under their releasing sequence and hand it their residual.
  It is a labelled banner landmark whose scroll actions expand and collapse it.

### Changed

- **`flui-widgets`**: focus moved by keyboard traversal or assistive technology onto a widget
  inside a `Scrollable` scrolls it into the viewport, through every enclosing scrollable, as a
  programmatic step of the scroll channel; a `CollapsingHeader` expands when such focus lands
  on content it clips.
