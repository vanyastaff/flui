### Added

- **Display snapshot** (`flui-platform-api`, `flui-platform`): `Platform::display_snapshot` returns an
  immutable `DisplaySnapshot` of every display: id, name, bounds, usable bounds, scale factor and the
  refresh period of the display's current mode. The period is a validated `RefreshPeriod` (1 ms to
  1 s) and `None` when the OS reports nothing that describes a cadence; it is never a guessed 60 Hz.
  A window names its display with `PlatformWindow::display_id` and hints at a change with
  `PlatformWindow::on_display_changed`.
- **Framework-issued display ids** (`flui-platform-api`): `DisplayId` is opaque and non-zero, issued by
  a `DisplayRegistry` from the OS's own output identity (`DisplayKey`). The same output keeps its id
  within a session, two identical monitors differ, and an exhausted id space refuses new displays
  instead of wrapping. It cannot be built from a number.
- **Windows follow their display** (`flui-app`, `flui-runtime`): the host samples the displays
  before the first window and on every owner wake or window hint, commits the snapshot before
  anyone is told, and delivers each window the display it is on as
  `WindowObservation::Display(DisplayUpdate)`. A newer "unknown" display replaces an older known
  one instead of losing to it, and an apply that fails part-way is offered again on the next
  sample. `UiRuntime::window_display`, `refresh_interval` and `scaled_refresh_interval`
  (`DEFAULT_REFRESH_PERIOD` is the 60 Hz policy while a display reports nothing) read the result.
- **Frame budget and frame span follow the display** (`flui-scheduler`):
  `FrameDuration::try_from_period` keeps a fractional rate exact,
  `UpdateScheduler::set_budget_frame_duration` points the stats-only budget at it, and the `frame`
  span carries the period it judged the frame by as `period_us`.
- **`JankLine`** (`flui-devtools`): `JankLine::FramePeriod` judges a frame against the period the
  `frame` span reports; `JankLine::Fixed(ms)` keeps one threshold. `Profiler::set_frame_period`
  feeds it.
- **Injectable displays in the headless platform** (`flui-platform`): `HeadlessPlatform::with_displays`
  takes `DisplaySpec`s, `HeadlessDisplayControls::simulate_display_change` switches a display's mode,
  and `MockWindow::simulate_move_to` moves a window between displays.

### Changed

- **Breaking: `PlatformDisplay`, `Platform::displays`, `Platform::primary_display`,
  `PlatformWindow::display` and `PlatformWindow::refresh_period` are removed** (`flui-platform-api`,
  `flui-platform`). Read the snapshot and `PlatformWindow::display_id` instead; `DisplayId` no longer
  has a public field and `MockDisplay` is gone. `PlatformError` gains a `Displays` variant.
- **Breaking: `ProfilerConfig::jank_threshold_ms` is replaced by `jank_line: JankLine`**
  (`flui-devtools`). The default is `JankLine::FramePeriod`, so the profiler's jank line is the
  display's own frame period (8.3 ms on a 120 Hz display) instead of a fixed 16.7 ms; use
  `JankLine::Fixed(ms)` to keep a fixed threshold.
- **Pacing follows a display change without a resize** (`flui-app`, `flui-runtime`): the desktop
  fallback wake, the scheduler's jank statistics and the frame-aligned pointer resampler's
  window read the presentation's refresh interval, on the main and every secondary window, and
  return to 60 Hz when the window moves to a display that reports no period.
- **The macOS and winit backends feed the snapshot** with the refresh period they already read, so
  120 and 144 Hz panels keep their real period. Windows, Android, iOS and the Web report an unknown
  period until their native sources land; Windows no longer answers the primary display for every
  window and instead reports no display for a window until its native source lands.
