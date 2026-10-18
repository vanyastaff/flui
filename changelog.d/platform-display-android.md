### Added

- **`flui-platform-api`**: `PlatformWindow::set_frame_rate_hint(FrameRateHint)`, a request for the
  display's highest refresh rate (`High`) or none (`Default`); a backend without such a request ignores it.
- **`flui-runtime`**: a presentation asks for the highest refresh rate while it animates or a pointer
  is down and lets go 250 ms after both end, published to the host through
  `OwnerEffects::frame_rate_demand_changed`; on Android the window passes it to
  `ANativeWindow_setFrameRate` (API 30+).

### Changed

- **`flui-platform` (Android)**: the display snapshot reports the real size, density, display id and
  refresh rate of every display, read through JNI, instead of a fixed 1080x2340 placeholder; the
  rate follows an adaptive 60/120 Hz switch through the choreographer's refresh-rate callback
  on API 30+ and configuration changes and resume below. The idle loop waits for the display's
  period instead of 16 ms.
