### Added

- **`flui-widgets`**: `Stack::clip_behavior` and `IndexedStack::clip_behavior` choose how a
  child that reaches outside the stack is clipped: `Clip::HardEdge` by default, or `Clip::None`
  to let it paint outside. The clip governs paint and semantics only; a pointer outside the
  stack still never reaches a child.

### Fixed

- **`flui-objects`**: an overflowing `RenderStack` or `RenderIndexedStack` that clips its paint
  now clips its children's semantics and input regions to the same box. A screen reader no
  longer reaches a positioned child the stack cuts off from view.
