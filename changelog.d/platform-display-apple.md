### Added

- **ProMotion on iOS** (`flui-platform`, `flui-cli`): the display link asks for the screen's best
  rate (`preferredFrameRateRange` from 60 to `maximumFramesPerSecond` on iOS 15 and later,
  `preferredFramesPerSecond` before it), and `flui build` writes
  `CADisableMinimumFrameDurationOnPhone` into the app's `Info.plist`, which an iPhone needs to run
  the link above 60 Hz.
- **iOS display identity and period** (`flui-platform`): a window reports its screen through
  `display_id()` and `on_display_changed`, and the snapshot carries the cadence the display link
  measures, published only when it leaves a 5% band so adaptive ProMotion does not republish every
  frame.

### Changed

- **macOS display snapshot** (`flui-platform`): displays are keyed by their UUID
  (`DisplayKey::MacosUuid`), so a replugged monitor keeps its `DisplayId`; the snapshot is resampled
  after `NSApplicationDidChangeScreenParametersNotification` and window display changes instead of
  on every owner wake.
