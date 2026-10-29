### Added

- **`flui-native`**: on Android 14 and later a touchpad's pinch arrives as `PanZoom` scale
  about its cursor and its two-finger scroll as phased `Scroll` from the cursor; its fake
  fingers are no longer touches, and its hovering finger is the mouse. The window's
  `InputCapabilities` report precise scrolling and trackpad gestures from API 34.
- **`flui-native`**: native recordings journal a contact's scroll step with its resolved
  phase (`ContactRegistry::scroll`), so Android wheel and touchpad scrolling replay through the
  shared decoders on any host
  ([ADR-0215](/docs/adr/ADR-0215-input-recording-and-backend-conformance.md) §4).

### Changed

- **`flui-cli`**: the Android manifest template declares `android.permission.VIBRATE`, which
  haptic patterns need.
- **`flui-native`**: Android haptic patterns carry the touch usage
  (`VibrationAttributes.USAGE_TOUCH` from API 33)
  ([ADR-0210](/docs/adr/ADR-0210-two-tier-haptics.md) §8).

### Fixed

- **`flui-native`**: an Android pen keeps one pointer identity from hover through contact and
  back; the hover exit Android sends at the instant of a press is no longer a leave.
- **`flui-native`**: an Android app without the `VIBRATE` permission reports its haptic
  patterns as falling back, and plays the fallback, instead of claiming patterns the vibrator
  refuses.
- **`flui-native`**: the native recording codec reads every number back to the bits it was
  written from, so a replayed pinch reproduces the delivered scale exactly.
