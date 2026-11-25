### Added

- **`flui-driver`**: the Android lane reads the tree TalkBack reads (`AccessibilityApi::UiAutomator`):
  `Window::os_tree` and `Window::compare_projection` work through it. `accesskit_android` sets no
  `viewIdResourceName`, so the lane matches a `uiautomator dump` to the app's own tree: the root
  where `FluiView.install` puts the app's view, the children in the order the adapter adds them
  (those its filter keeps under a clipping parent first), by role (the class `accesskit_android`
  gives each AccessKit role, `lane::android::roles`), name, toggled state where the app's read
  reports it (a radio button's selection included), enabled state and bounds. Two candidates the
  order does not tell apart answer `Error::Ambiguous`, and what a dump cannot show (a range's
  value, the announcer nodes, a toggled state the app's read does not report) is no difference
  (`lane::android::tree::Unobservable`). The roles scenario checks the match itself.
- **`flui-driver`**: the Android lane locates the app's window from one `dumpsys window -a`,
  which the window manager dumps under one lock, `-a` printing the windows' frames: the default
  display's focused window, its frame found by the window's hash, and the default display's
  rotation, read from the reply's live parts, so pressing, typing and capturing go on while an
  accessibility service runs. Another display holding the device's input focus answers
  `not_supported` naming `android_default_display`. A press, key, text or scroll goes out only
  while the window the lane located holds the focus, at the frame and turn it located it at:
  another window of the app's title answers `Error::NotForeground`, the same window moved or
  turned `Error::Busy`; a window over the point that takes no focus is not checked
  (`docs/gaps.md`). Only the read of the tree takes a `uiautomator dump`,
  which suspends every accessibility service: while any service is on (an empty service list
  being none), on a device or an emulator, or while the settings it reads show neither way, the
  read answers `not_supported` naming `android_tree_dump` and the services. A window replaced or
  moved, or a display that turned, during the dump answers `Error::Busy`. `Window::os_invoke`
  answers `not_supported` naming `os_invoke`, since UI Automator performs no accessibility
  action.

### Changed

- **`flui-driver`**: `Lane::read_tree` takes the window's tree as the app published it beside
  the placement, so a lane whose OS tree names no identifier can match the two
  (`AccessibilityApi::matches_app_tree`); `Window::os_tree` reads the app's tree only for such a
  lane and hands the others an empty one.
