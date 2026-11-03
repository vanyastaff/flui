### Added

- **`SliverLayoutContext::measure_box_child` and `BoxChildSize`**: a sliver can tell a box child's
  measured size from a stand-in the layout walk substituted (a failed layout, a poisoned child,
  an empty slot).

### Fixed

- A lazy list no longer records a failed or poisoned child's stand-in as that item's extent, so
  the first visible item stays put when a child above it fails and again when it recovers.
