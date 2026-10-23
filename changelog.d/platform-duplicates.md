### Added

- **`flui-platform`**: `RefreshPeriod::from_secs_f64`, the period of a cadence measured as the
  time between two frames; it refuses an interval that is not finite, not positive, or outside
  1 ms to 1 s, as the other `RefreshPeriod` constructors do.

### Changed

- **`flui-native`**: a scroll step's axis that did not move is `+0.0` on every host. The winit,
  AppKit and Android hosts reported `-0.0` there (so `signum()` on it gave `-1.0`), and the Web
  passed a `WheelEvent`'s `-0` delta through.
