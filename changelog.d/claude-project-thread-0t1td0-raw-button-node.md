### Added

- `flui_objects::RenderInteractive`: one render object for a control's
  pointer, mouse, semantics and focus-ring behaviours, made of the render
  objects that carry each alone (ADR-0322 §1).
- `StatefulRenderView::did_unmount_render_object`: the view releases what its
  render object registered, after the state is disposed.

### Changed

- `RawButton` mounts one element and one render object for its behaviours
  instead of five wrappers: a button adds 2 elements and 1 render node to its
  child (was 15 and 7), and about a third of the allocations. Its
  accessibility tree, focus traversal and gesture-arena behaviour are
  unchanged.
