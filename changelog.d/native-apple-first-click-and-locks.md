### Added

- **`flui-widgets`**: `FirstClick` lets the click that brings an inactive window forward act
  on its child (a toolbar button, a drag source). On macOS that click is delivered inside the
  widget's box as a press marked `PointerPress::is_first_click` and only activates the window
  anywhere else; the gesture binding admits a marked press only inside an
  `InputRegionKind::FirstClick` region, on every host.
- **`flui-platform`**: `InputRegionKind::FirstClick`, `PointerPress::is_first_click` and
  `with_first_click`, and `InputRegion::contains` / `DeviceInputRegion::contains`, which count
  a rectangle's points from its minimum edges up to, not including, its maximum ones.
- **`flui-interaction`**: `GestureBinding::set_input_regions`, the window's regions the binding
  admits a first click by; the runtime hands it the set of the frame on screen.
