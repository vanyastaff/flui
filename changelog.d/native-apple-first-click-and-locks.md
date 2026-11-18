### Added

- **`flui-widgets`**: `FirstClick` lets the click that brings an inactive window forward act
  on its child (a toolbar button, a drag source). On macOS that click is delivered inside the
  widget's box as a press marked `PointerPress::is_first_click` and only activates the window
  anywhere else; the gesture binding admits a marked press only when its hit path runs
  through a `FirstClick` box, and hands it only to the box's subtree, so a widget painted
  over the box does not receive it, whether it takes the hit or lets it through.
- **`flui-platform`**: `InputRegionKind::FirstClick`, `PointerButtonEvent::is_first_click`
  and `PointerPress::with_first_click`, the `first_click` input capability (native on
  macOS), and `DeviceInputRegion::contains`, which counts a rectangle's points from its
  minimum edges up to, not including, its maximum ones.
- **`flui-interaction`**: `pointer::FirstClickTarget`, the hit-test payload of a box that
  accepts the first click, and `HitTestEntry::subtree_entries`, the entries a target's
  subtree put on the path just before it.
- **`flui-rendering`**: `RenderObject::joins_hits_below`, for a node that joins every hit
  path running through its subtree, a translucent target's included.
- **`flui-protocol`**: the input recording carries a press's first-click mark and the
  `first_click` capability; the agent protocol is 0.28.
