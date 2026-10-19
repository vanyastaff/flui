### Added

- **Hold on Down** (`flui-interaction`, ADR-0208 rule 4): `HoldGestureRecognizer` joins its
  contact as a hold (`ArenaMembership::holding_contact`); the contact's members that do not move,
  a tap or a long press, lose to it, and drags and scrolls still take the contact from it.
  `Scrollable` admits one while it moves, so a press on a flinging list or a settling pager stops
  it and is not a tap of the content under it; released, the position settles from rest.
- **Hover freezes under a latched scroll** (ADR-0208 rule 6): while a moving member owns a
  contact, or a consumer claimed a trackpad pan-zoom session, no `MouseRegion` is entered or
  exited and the cursor holds; the first refresh after the latch hit-tests where the pointer
  went. `GestureBinding::is_hover_frozen` reports it.
- **`AxisLock`** (`flui-interaction`): `DragGestureRecognizerBuilder::axis_lock` and
  `ScaleGestureRecognizerBuilder::axis_lock` rail a free motion that starts within 22.5° of an
  axis onto it, deltas and release velocity alike, for touch and touchpad.
- **`TransformLock`**: `ScaleGestureRecognizerBuilder::transform_lock` keeps rotation or scale at
  identity in a scale gesture that begins as a pan.
- **`SwipeGestureRecognizer`** with `SwipeStart::{Anywhere(Axis), FromEdge(PhysicalEdge)}`: an
  axial arena member that reports a fling-fast release along its axis, with the edge it heads
  towards.
- **`LaidOut::replay` and `replay_with`** (`flui-testing`): replay a `PointerScript` on a mounted
  tree. `assert_ownership_table` gains widget rows: a `PageView` over a vertical drag and a tap, an
  `InteractiveViewer` in a `PageView`, and a press or a vertical drag on a settling pager.
