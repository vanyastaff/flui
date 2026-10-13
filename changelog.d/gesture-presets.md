### Added

- **Gesture presets** (`flui-interaction`): `GestureSettings::preset(platform, pointer_kind)`
  returns a platform's behavior values for a pointer kind, falling back to its `PointerClass`
  (coarse or fine) row and then the platform row. Rows cite their sources (AOSP
  `ViewConfiguration`, UIKit, documented Windows and GTK defaults, WCAG target sizes) and carry
  `min_target`, `spring_loading_delay`, `swipe_edge_width`, `remainder_handoff`
  (`RemainderHandoff`), `contactless_latch_idle`, `phased_latch_idle`, `max_resolution_delay` and
  `haptic_frame_deadline_factor`.
- **System haptics preference** (`flui-platform-api`): `SystemPreferences::haptics()`, read on
  Android from the touch-feedback setting; an observed "off" suppresses framework haptic feedback.
- **Named presets for presentations** (`flui-runtime`, `flui-testing`):
  `PresentationWindow::with_gesture_preset` and `HeadlessWindow::with_gesture_preset`; a headless
  host defaults to the platform-neutral preset, never the build machine's.

### Changed

- **Accessibility timing floors** (`flui-interaction`, `flui-widgets`): observed long-press,
  double-tap and double-click timings are lower bounds; a shorter authored value, including a
  nested `GestureArenaScope` override, is raised to them for new contacts.
- **Gesture geometry follows the display** (`flui-runtime`): a window moved to another display
  re-reads touch slop and click geometry for new contacts (`WindowObservation::Moved`); active
  contacts keep the values they were admitted with.

### Removed

- `GestureSettings::touch_defaults`, `mouse_defaults`, `pen_defaults`, `android_defaults`,
  `ios_defaults`, `for_device`, `for_platform` and `native`: use `GestureSettings::preset`.
  `GestureSettings::default()` is the platform-neutral touch preset.
