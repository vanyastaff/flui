### Added

- **`flui-widgets`**: `Stack::clip_behavior` and `IndexedStack::clip_behavior` choose how a
  positioned child that reaches outside the stack is painted: clipped to the stack's box with
  `Clip::HardEdge` (the default), or painted outside with `Clip::None`. Semantics and input
  regions are cut where paint is; a pointer outside the stack still never reaches a child.
