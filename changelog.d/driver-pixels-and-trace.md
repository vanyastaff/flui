### Added

- `flui_driver::pixels`: captures that name their source (`Capture`, `CaptureSource`),
  `Window::capture`, `Window::capture_stable` (until two consecutive captures, masked, are equal)
  and `Window::expect_stable_for`; masks by locator or region (`Mask`); pixelmatch's YIQ metric
  with a threshold, a pixel or ratio budget and opt-in anti-aliasing excuse (`metric`,
  `Tolerance`); PNG baselines keyed by source, backend and OS (`Baselines`,
  `Window::expect_baseline`) that leave `.new.png` and `.diff.png` for review, with the update modes
  `none`, `missing`, `changed` and `all` (`FLUI_DRIVER_UPDATE_BASELINES`, refused under `CI`); and
  predicates on one capture (`expect_color_at`, `expect_uniform`, `expect_same_as`,
  `expect_differs_from`). A PNG is bounded by its file length and declared size before it is
  decoded.
- `flui_driver::trace` (ADR-0241): `Driver::start_trace` and `Driver::finish_trace` write a
  `.flui-trace` directory of every command as the caller wrote it with its result, the app's
  events, content-addressed evidence within a budget (`gap` rows past it), the app's input
  recording and its native recordings, text redacted unless kept, kept always or only on
  failure; `Trace::open` reads one back within its bounds and reports a trace cut short as
  incomplete; `trace::replay` sends its commands again and names the first divergence;
  `trace::codegen` writes a Rust test from it; `trace::native::read` reads a native recording
  through `flui-protocol`'s schema, without the core.
- `ScriptedLane::screen` gives the scripted lane a screen to capture, and
  `ScriptedAgent::start_with` introduces the stand-in with capabilities of the test's own.

### Changed

- `Lane::capture_window` answers a `pixels::Image`, whose fields are read through `width`,
  `height`, `rgba` and `pixel`; `lane::OsCapture` and `Window::capture_os` are gone, replaced by
  `Window::capture(CaptureSource::Os)`.
- `flui record --native` writes its native files with the driver's writer
  (`flui_driver::trace::native::NativeFiles`), the same one a trace uses.
