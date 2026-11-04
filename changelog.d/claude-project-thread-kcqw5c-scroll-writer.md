### Added

- `ScrollOffsetWriter` and `OffsetWriteCause` (`flui_rendering::view`): the one write of a scroll
  offset. `ScrollPosition::with_writer(initial)` returns a position and its single writer, which
  is neither `Clone` nor `Send`; every write names its cause (`UserInput`, `Momentum`,
  `Ballistic`, `Programmatic`). `ScrollPosition::fixed(pixels)` builds a position nobody writes.
- `ScrollController::place_for_test` (behind the `testing` feature) rests a position at an
  offset, overscrolled included, without the owner.

### Changed

- `ScrollController` holds its position's writer; programmatic scrolling from user code is
  `ScrollController::jump_to` and `animate_to`, which go through the scrollable's owner and the
  nested-scroll chain.
- Keyboard focus that reaches an unbuilt list row and `AnchoredDrag` resting on its nearest
  anchor now scroll through the enclosing scrollable's controller, so nested-scroll
  participants see the move as a `Programmatic` sequence.
- `PageView`'s `jump_to_page` goes through `ScrollController::jump_to`.
- `ScrollPositionScope::new` takes the `ScrollController` instead of a position.
- `RenderViewport` and `RenderShrinkWrappingViewport` default their offset type to
  `ScrollPosition`.

### Removed

- The public raw offset writes: `ScrollPosition::{new, zero, set_pixels,
  set_pixels_with_recovery, notify, notify_with_recovery, set_is_scrolling,
  set_is_scrolling_with_recovery, set_user_scroll_direction, flush_now}`,
  `ScrollController::set_pixels` and `ViewportOffset::{jump_to, animate_to, move_to}`. Use
  `ScrollController::jump_to`/`animate_to`, or `ScrollPosition::with_writer` for a position you
  own.
- `ScrollableViewportOffset` and `ScrollableViewportLayout`; use `ScrollPosition`.
