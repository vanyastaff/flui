### Added

- **Animated `Switch`** (`flui-material`): toggling slides the thumb between the track's ends,
  grows it from 8 to 12 and blends the track, outline and thumb colours, over 200 ms by default.
  The thumb's travel is mirrored under right-to-left `Directionality`, a toggle that interrupts a
  running one continues from the shown position, and reduced motion shows the new value on the
  first frame. `Switch` implements `MotionBuilder` (`duration`, `curve`, `spring`, `motion`).
- **Animated `NavigationBar` indicator** (`flui-material`): a selection change expands the new
  destination's indicator from its centre while the previous one collapses, over 500 ms along
  `EASE_IN_OUT_CUBIC_EMPHASIZED` by default; reduced motion shows the new selection on the first
  frame. `NavigationBar` implements `MotionBuilder`.
