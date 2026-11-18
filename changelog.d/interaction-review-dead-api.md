### Removed

- `HorizontalDragGestureRecognizer`, `VerticalDragGestureRecognizer` and `PanGestureRecognizer`
  with their `drag_variants` builders: a drag recognizer is `DragGestureRecognizer::builder(arena,
  DragAxis::…)`, one form for every axis.
- `GestureSettings::try_new`, the eight-argument constructor:
  `GestureSettings::try_from_values(GestureValues { … })` names each field.
- Methods no caller used: `Velocity::from_offset_over_duration`, `Velocity::negate`,
  `VelocityEstimate::is_reliable`, `VelocityTracker::has_sufficient_data`,
  `GestureArena::reject_member`, `GestureDisposition::is_rejected`,
  `ForcePressDetails::normalized_pressure`, `HitTestEntry::with_transform_unchecked`,
  `EventPropagation::should_continue`,
  `MouseTracker::mouse_is_connected`, `GestureBinding::cancel_pointer_sequence`, `close_arena` and
  `sweep_arena`, `TextInputOwner::active_count`, `FocusScopeNode::set_traps_focus` and the testing builder
  `make_move_event_with_button`.

### Changed

- Focus-node and interaction-lane identities come from `AtomicIdMint`, and the traversal, drag-move
  and haptic-observer generation counters refuse to wrap.
