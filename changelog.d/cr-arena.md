### Added

- `MenuSpec::dismiss` closes the menu a presenter shows when the long press that opened it hands its contact to a drag; `ContextMenuArea` closes its menu this way.
- `DragSlot::axis_lock`, `TransformSlot::axis_lock` and `TransformSlot::transform_lock` put `AxisLock` and `TransformLock` on a `GestureDetector`'s drag and transform.
- `GestureArena::admits_like` tells whether two arena handles admit members into the same arena, composition branch and `PanClaim` region.

### Changed

- `GestureArenaMember::poll_deadline_with_recovery` is a documented hook: a member whose deadline commits a transition and then calls user code overrides it with the `RecoveryScope` the arena hands it. The member's type name is no longer a trait method.

### Removed

- `GestureBinding::handle_pointer_event_on`; call `GestureBinding::dispatch_pointer_on`.
- `GestureSettings::exceeds_touch_slop`, `exceeds_pan_slop` and `is_fling_velocity`; use the per-kind `exceeds_*_slop_for` checks and `min_fling_velocity`.

### Fixed

- A list inside an `InteractiveViewer` (or any channel leaf) owns a drag along its axis; the viewer used to take every drag.
- A context menu's long press and a long-press `Draggable` beneath it are one hold: the menu opens, and a finger moving past the slop drags the item and closes the menu, instead of two holds competing for the contact.
- Recognizers follow their `GestureArenaScope`: a scope handing out another arena handle rebuilds the recognizers of `RawGestureDetector`, `GestureDetector`, `Draggable` and the back gesture, and a rebuilt `PanClaimRegion` no longer rebuilds the recognizers inside it.
