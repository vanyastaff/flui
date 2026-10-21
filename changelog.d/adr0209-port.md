### Added

- **Scroll steps carry where they point** (`flui-widgets`): `ScrollStep` and `FlingStep` have
  `focal_point` (the contact or the wheel's pointer, else the middle of the leaf) and
  `device_pixel_ratio`; a fling or a settle keeps its release's.
- **Programmatic opt-in** (`flui-widgets`): `NestedScrollConnection::accepts_programmatic`. A
  connection takes part in a jump, an animation or a reveal only by returning `true`; by default
  it hears nothing of them.
- **Wheel latch end** (`flui-interaction`): `GestureArena::dispatched_latch_end` reports when the
  binding's latch of the wheel or trackpad packet being dispatched ends (`ScrollLatchEnd`), and
  `GestureArena::register_deadline` registers a deadline source that joins no contest.

### Changed

- **Each nested scroll participant works in its own plane** (`flui-widgets`): a step's
  `available`, `focal_point` and `consumed_by_child`, and a `FlingStep`'s velocity, are in the
  receiving participant's local coordinates, so participants above and below a scaled, rotated
  or projected `Transform` or a zoomed `InteractiveViewer` share one motion on screen. A
  participant behind a transform that cannot be crossed is offered nothing.
- **A wheel or trackpad scroll sequence ends when it goes idle** (`flui-widgets`): participants
  hear `sequence_ended(Released)` at the binding latch's idle deadline (1500 ms, 3000 ms for a
  phased source) or with a trackpad's end phase, instead of only when the next sequence starts.
- **`NestedScrollConnection` participants no longer see programmatic steps by default**
  (`flui-widgets`): a connection that reacted to `ScrollCause::Programmatic` must return `true`
  from `accepts_programmatic`. The settle after a reveal no longer moves the scrollables above the
  one it reveals in; each of them moves for the reveal on its own.
