### Added

- `WidgetsBinding::inspect_elements` and `PipelineCell::inspect_render` take typed snapshots of
  the element and render trees, and answer `SnapshotError::Busy` when asked from code the tree's
  owner is running (a `build`, a view's `inspect`, a frame) instead of waiting forever.
- A snapshot records a node whose `inspect` panicked as the fact `inspect_failed`, keeps the
  fields it wrote, and goes on to the other nodes.

### Changed

- `SnapshotError` is `#[non_exhaustive]`.
- `WidgetsBinding::with_element_tree` and `with_element_tree_mut` called from inside a build or a
  snapshot panic with a message naming what the binding was doing, where they used to deadlock.
