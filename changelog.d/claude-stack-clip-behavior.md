### Added

- **`flui-widgets`**: `Stack::clip_behavior` and `IndexedStack::clip_behavior` choose how a
  child that reaches outside the stack is clipped: `Clip::HardEdge` by default, or `Clip::None`
  to let it paint outside. The clip governs paint and semantics only; a pointer outside the
  stack still never reaches a child.
