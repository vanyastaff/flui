### Changed

- **`flui-driver`**: the Android lane locates the app's window from `dumpsys window` (the focused
  window's frame, which the app's view covers) instead of a `uiautomator dump`, so pressing, typing
  and capturing go on while an accessibility service (a password manager, an automation app)
  runs; only reading the tree is refused then, and on an emulator not at all.
  `Window::os_tree` and `Window::compare_projection` read the app's tree again while it moves, at
  least three times and for two seconds after the first reading (one reading through a phone can
  outlast that), and also when the lane could not match a tree that moved; then they answer
  `Error::Busy`, which an app republishing every frame always does on Android.

### Fixed

- **`flui-driver`**: the Android lane takes the focused window's frame by the window's id, so a
  leaving window of the same title or split screen gives no wrong origin, and the rotation from
  the default display's section wherever its word stands in the line.
- **`flui-driver`**: the Android lane matches a grid cell, and a list row, tab, tree item or list
  box option that publishes a selection, whose toggled state the app's read does not report and
  the adapter publishes as checkable. A node's children are cut to the node budget before they
  are matched, so a dump wider than the budget costs no more than it.
