### Added

- **`flui-driver`**: the Android lane reads the tree TalkBack reads (`AccessibilityApi::UiAutomator`):
  `Window::os_tree` and `Window::compare_projection` work through it. `accesskit_android` sets no
  `viewIdResourceName`, so the lane matches `uiautomator dump` to the app's own tree by structure,
  role (the class `accesskit_android` gives each AccessKit role, `lane::android::roles`), name,
  state and bounds; two candidates alike answer `Error::Ambiguous`. The roles scenario reads
  Android's class names through it. The lane takes no dump while a screen reader runs, or while the
  accessibility settings it reads show neither way, since the
  dump suspends it (`not_supported` naming `android_window_dump`), and `Window::os_invoke` answers
  `not_supported` naming `os_invoke`, since UI Automator performs no accessibility action.

### Changed

- **`flui-driver`**: `Lane::read_tree` takes the window's tree as the app published it beside
  the placement, so a lane whose OS tree names no identifier can match the two
  (`AccessibilityApi::matches_app_tree`); `Window::os_tree` reads the app's tree only for such a
  lane and hands the others an empty one.
