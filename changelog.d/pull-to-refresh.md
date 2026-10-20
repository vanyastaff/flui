### Added

- `RefreshIntent`, the intent a `RefreshIndicator` binds, and
  `RefreshIntent::command(label)`, the refresh command with F5 and the
  platform's reload chord (Cmd+R on macOS and iOS, Ctrl+R elsewhere).
- `RefreshIndicator::axis(Axis)`: pull right from the left edge of a
  horizontal scrollable.

### Changed

- `RefreshIndicator` is a participant of the nested scroll channel instead of
  a scroll view of its own: wrap a `Scrollable`, `ListView` or any scrollable
  child, and it pulls with what the child leaves past its leading edge while a
  contact drags it. A wheel, a trackpad pan and its momentum, keys, assistive
  steps and a fling into the top never pull; a cancelled contact settles back
  without refreshing; a pull released short of the threshold settles back over
  200 ms instead of snapping.
- `RefreshIndicator::new(child, alternatives)` takes the scrollable child and
  its `GestureAlternatives`, whose first command performs `RefreshIntent`:
  assistive technology's custom action, the command's keys and its visible
  control or menu refresh as the pull does.
- `RefreshIndicator::on_refresh` returns a future; the refresh runs until it
  completes, and the completion reaches the next frame. While a refresh runs,
  every path that would start another is refused.
- The settling run's controller is labelled `RefreshIndicator.settle`.

### Removed

- `RefreshIndicator::child`, `scroll_controller` and `physics`: the
  scrollable child owns its controller and physics.
- `RefreshController::finish`: completing the future `on_refresh` returned
  ends the refresh, and unmounting the indicator cancels it.
