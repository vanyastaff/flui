### Added

- `flui_testing::widgets::LaidOut::pump_report(dt)` drives one frame of the UI runtime and
  reports its build, pipeline and scene-patch counts, which the widget perf scenarios compare
  against their baselines.

### Removed

- `HeadlessBinding::pump_frame_report` and `HeadlessBinding::last_frame_report`: the perf scenarios
  report their frames through `LaidOut::pump_report`.
