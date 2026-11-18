### Added

- `KeyboardPlacement` says where the soft keyboard rests over a window: hidden, docked with the
  inset it covers, or floating with its frame (an iPad's floating, undocked or split keyboard).
  `SoftKeyboard::placement` and `SoftKeyboard::floating_frame` read it.
- `MediaQuery::floating_keyboard_of` reads the frame of a keyboard floating over the window, so
  a layout can keep a field or a toolbar clear of it. `FloatingKeyboard` is that frame as a
  value a view's `provide` hook publishes to its subtree, as each window's root does while a
  keyboard floats. `MediaQueryData` is unchanged.
- `HeadlessWindow::with_insets` opens a test window with insets already covering it.

### Changed

- `KeyboardInset` is `SoftKeyboard`: it also holds keyboards that cover no edge.
- `SoftKeyboard::try_at_rest`, `SoftKeyboard::try_moving` and `HeadlessHost::move_keyboard` take
  a `KeyboardPlacement` in place of the covered edges; a docked placement that covers no edge is
  hidden, and a floating frame that is not finite or has no area is refused with
  `InvalidInsets::NonFinite` or the new `InvalidInsets::EmptyFrame`.
- The agent protocol is revision 0.27: a recorded keyboard carries its floating frame, and a
  recording without it reads as before.

### Fixed

- On iOS, frames keep coming while UIKit tracks a touch (an edit menu, a native scroll view or
  control, an interactive keyboard dismissal).
- On iOS, a floating keyboard no longer shrinks a layout that resizes for the keyboard to the
  strip above it, dragging it no longer makes the inset flicker to none, a keyboard the user drags
  or undocks is published where it comes to rest, a keyboard hiding with
  its end frame still partly on screen leaves no inset behind, and neither does a shortcuts bar
  dropped away from the bottom edge. The Web host reads a keyboard rectangle by the same rule.
