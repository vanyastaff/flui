### Added

- **`flui-widgets`**: `FirstClick` lets the click that brings an inactive window forward act
  on its child (a toolbar button, a drag source). On macOS that click is delivered inside the
  widget's box as a press marked `PointerPress::is_first_click` and only activates the window
  anywhere else; the gesture binding admits a marked press only when its hit path runs
  through a `FirstClick` box, so a widget painted over the box does not receive it.
- **`flui-platform`**: `InputRegionKind::FirstClick`, `PointerPress::is_first_click` and
  `with_first_click`, the `first_click` input capability (native on macOS), and
  `DeviceInputRegion::contains`, which counts a rectangle's points from its minimum edges up
  to, not including, its maximum ones.
- **`flui-interaction`**: `pointer::FirstClickTarget`, the hit-test payload of a box that
  accepts the first click.
