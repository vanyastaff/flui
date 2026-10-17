### Added

- **`flui-widgets`**: `SwipeActions`, a row whose leading and trailing actions a horizontal
  swipe reveals (`SwipeAction`, `SwipeEdge`, `SwipeAlternative`, `SwipeActionsError`). A
  release past halfway or a fling leaves the actions shown, tapping one closes the row and
  performs its command, and `full_swipe` lets a release far past an edge's actions perform its
  first one. Every action is also the row's custom action, has its command's keys while the
  row has focus, and is reached with one pointer through visible controls, the theme-free
  `ContextMenuArea` menu or a design package's presenter.
- **`flui-widgets`**: `AnchoredDrag::semantics` names the drag's slider for assistive
  technology (its label, and a description of the current anchor as its value) and makes the
  drag a stop of focus traversal; `AnchoredDragController::offset_listenable` hears every move
  of the offset as it happens.
- **`flui-objects`**: `TransformMotion::Translate`, a `RenderAnimatedTransform` translation by
  an offset in logical pixels.

### Changed

- **`flui-widgets`**: `Dismissible` drags its card with an `AnchoredDrag`: the card stops at
  its extent, settles through the scrollable's spring, and moves without rebuilding. A
  cancelled drag returns the card, a fling against a threshold of `1.0` settles back, and a
  touch going down in the window's system-gesture strip is left to the operating system's edge
  swipe (`SwipeActions` does the same). `Dismissible` and its types moved from
  `flui_widgets::interaction` to the crate root only.
- **`flui-widgets`**: an `AnchoredDrag` is hit only where its child is painted, so the strip a
  drag uncovers reaches what lies under it; its child moves by patching a transform layer
  instead of rebuilding.

### Removed

- **`flui-widgets`**: `Dismissible::movement_duration`, `Dismissible::cross_axis_end_offset`
  and `Dismissible::behavior`: the settle follows the release's velocity, the card slides on
  its axis only and is hit where it is painted.
