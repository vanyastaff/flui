### Added

- **`flui-interaction`**: `TapDetails::pointer`, the contact a tap movement or cancellation
  belongs to.
- **`flui-widgets`**: `ButtonChange` re-exported beside `PointerEvent`, for `Listener`'s
  `on_pointer_move`.

### Changed

- **`flui-widgets`**: `GestureDetector::on_tap_cancel` callbacks receive the `TapDetails` of the
  cancelled contact.

### Fixed

- **`flui-interaction`**: a tap that loses the gesture arena without moving (to a long press, or a
  scrolling ancestor) now reports `on_tap_cancel`.
- **`flui-material`**: `InkWell` fades only the rejected contact's ripple, ends a press when the
  primary button comes up under another held button, shows a press on the state layer when no
  `VsyncScope` clock can grow a wave, and detaches its painter once its feedback has settled.
