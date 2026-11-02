### Added

- **Window insets** (`flui-platform`, `flui-runtime`, `flui-widgets`, `flui-testing`): a window
  reports `WindowInsets`, its safe area, the soft keyboard's occlusion (`KeyboardInset`, with the
  `InsetMotion` the OS animates it by) and the system gesture regions. Each frame lays out with the
  keyboard's occlusion at that frame's time in `MediaQueryData::view_insets`, and the new
  `MediaQueryData::system_gesture_insets` carries the gesture regions.
  `WindowCapabilities::keyboard_inset()` and `system_gestures()` say whether a window reports them.
  `HeadlessHost::set_window_insets` and `HeadlessHost::move_keyboard` script them in tests.

### Changed

- **`WindowState::safe_area` is `WindowState::insets`** (`flui-platform`), and
  `WindowObservation::SafeArea` is `WindowObservation::Insets` (`flui-runtime`); the safe area is
  `WindowInsets::safe_area()`. The input-recording row `safe_area` is `insets`.
