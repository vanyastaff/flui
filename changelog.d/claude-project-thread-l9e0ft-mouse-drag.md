### Added

- `LaidOut::dispatch_pointer_down_with_kind` presses with a chosen device; the contact's moves,
  release and cancel keep its kind.

### Changed

- A scroll view that names no devices no longer drags with the mouse:
  `DefaultScrollBehavior::drag_devices` admits touch, pen, trackpad and virtual pointers, and the
  mouse scrolls by its wheel and the scrollbar. `allowed_devices(PointerKindSet::all())` or a
  `ScrollConfiguration` brings the mouse drag back. `AnchoredDrag` (sheets, swipe rows) still
  takes every device.
- `LaidOut::dispatch_pointer_down`, `dispatch_pointer_move`, `dispatch_pointer_up` and
  `dispatch_pointer_cancel` drive a touch contact instead of a mouse one.
