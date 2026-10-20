### Added

- `flui::interaction` and `flui_sdk::interaction` re-export `AxisLock` and `TransformLock`, which
  `DragSlot::axis_lock`, `TransformSlot::axis_lock` and `TransformSlot::transform_lock` take, and the
  swipe recognizer (`SwipeGestureRecognizer`, `SwipeStart`, `SwipeDetails`, `PhysicalEdge`).

### Changed

- A type used where a `GestureRecognizer` is required now gets a diagnostic naming what a recognizer
  implements (`GestureRecognizer` and its supertrait `GestureArenaMember`) instead of a bare unsatisfied
  bound.
