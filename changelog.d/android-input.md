### Added

- **`flui-platform`**: on Android 14 and later a touchpad's pinch arrives as `PanZoom` scale
  about its cursor and its two-finger scroll as phased `Scroll` from the cursor; its fake
  fingers are no longer touches, and its hovering finger is the mouse. The window's
  `InputCapabilities` report precise scrolling and trackpad gestures from API 34.
- **`flui-platform`**: native recordings journal a contact's scroll step with its resolved
  phase (`ContactRegistry::scroll`), so Android wheel and touchpad scrolling replay through the
  shared decoders on any host
  ([ADR-0215](/docs/adr/ADR-0215-input-recording-and-backend-conformance.md) §4).
- **`xtask`**: `cargo xtask device android-input` installs a probe on an attached Android
  device, injects touch, stylus, mouse, a `uinput` pen and a `uinput` touchpad, holds the
  recordings of what the backend delivered against checked-in goldens, and checks the system
  gesture exclusion, the haptics and long-press settings and haptics on the device.

### Changed

- **`flui-cli`**: the Android manifest template declares `android.permission.VIBRATE`, which
  haptic patterns need.
- **`flui-platform`**: Android haptic patterns carry the touch usage
  (`VibrationAttributes.USAGE_TOUCH` from API 33)
  ([ADR-0210](/docs/adr/ADR-0210-two-tier-haptics.md) §8).

### Fixed

- **`flui-platform`**: an Android pen keeps one pointer identity from hover through contact and
  back; the hover exit Android sends at the instant of a press is no longer a leave.
- **`flui-platform`**: an Android app without the `VIBRATE` permission reports its haptic
  patterns as falling back, and plays the fallback, instead of claiming patterns the vibrator
  refuses.
- **`flui-platform`**: the native recording codec reads every number back to the bits it was
  written from, so a replayed pinch reproduces the delivered scale exactly.
