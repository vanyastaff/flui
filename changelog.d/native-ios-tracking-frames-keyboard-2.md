### Added

- `KeyboardPlacement` says where the soft keyboard rests over a window: hidden, docked with the
  inset it covers, or floating with its frame (an iPad's floating, undocked or split keyboard).
  `KeyboardInset::placement` and `KeyboardInset::floating_frame` read it, and
  `MediaQueryData::floating_keyboard` carries a floating keyboard's frame, so a layout can keep a
  field or a toolbar clear of it.

### Changed

- `KeyboardInset::try_at_rest`, `KeyboardInset::try_moving` and `HeadlessHost::move_keyboard`
  take a `KeyboardPlacement` in place of the covered edges; a docked placement that covers no
  edge is hidden, and a floating frame that is not finite or has no area is refused with
  `InvalidInsets::NonFinite` or the new `InvalidInsets::EmptyFrame`.
- The agent protocol is revision 0.27: a recorded keyboard carries its floating frame, and a
  recording without it reads as before.

### Fixed

- On iOS, frames keep coming while UIKit tracks a touch (an edit menu, a native scroll view or
  control, an interactive keyboard dismissal).
- On iOS, a floating keyboard no longer shrinks a layout that resizes for the keyboard to the
  strip above it, dragging it no longer makes the inset flicker to none, a keyboard hiding with
  its end frame still partly on screen leaves no inset behind, and neither does a shortcuts bar
  dropped away from the bottom edge. The Web host reads a keyboard rectangle by the same rule.
