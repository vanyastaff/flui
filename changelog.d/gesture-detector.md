### Added

- `GestureDetector::on_multi_tap(pointers, callback)` for a tap of several
  contacts, and `GestureDetector::force_press(ForcePressSlot)` for pressure
  presses with start and peak thresholds.
- `DragSlot::vertical()`: a drag that claims only vertical motion.

### Changed

- `GestureDetector` takes typed slots and mounts their recognizers through
  `RawGestureDetector`. A drag is one `DragSlot` (`pan()`, `horizontal()` or
  `vertical()`, with `on_down`, `on_start`, `on_update`, `on_end`, `on_cancel`,
  `allowed_devices` and `pointer_strategy`), set with `.drag(slot)`; scale and
  rotation is one `TransformSlot`, set with `.transform(slot)`. Migrate
  `.on_pan_start(f)` to `.drag(DragSlot::pan().on_start(f))`,
  `.on_horizontal_drag_*` to `DragSlot::horizontal()`, `.on_scale_*` to
  `TransformSlot::new().on_*`, and `.drag_pointer_strategy(s)` to
  `DragSlot::pointer_strategy(s)`. The tap, secondary-tap, long-press and
  double-tap methods are unchanged.
- A detector composes its tap with its double tap only when both are set; a
  lone tap joins the arena uncomposed, as the gesture inspector now shows.
- A vertical `Dismissible` drags on the vertical axis only, so a horizontal
  motion across it reaches whatever encloses it.

### Removed

- `GestureDetector::exclusive_drags()` and the debug assertion for pan and
  horizontal-drag callbacks on one detector: a detector holds one drag slot,
  so that combination cannot be configured.
