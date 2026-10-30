### Added

- **`flui-platform`**: `PreferenceParts`, `GestureParts` and `WheelParts`, the public fields of
  `SystemPreferences`, `GesturePreferences` and `WheelPreferences` (`into_parts`, and back through
  `PreferenceParts::try_into_preferences` with the same validation as the `with_*` methods). Every
  serialization of a preferences snapshot goes through them, so a new preference fails to compile
  in each mirror until it carries it.
- **`flui-platform`**: `DisplayParts` and `ColorParts`, the public fields of `DisplayInfo` (apart
  from its id) and of `DisplayColor` with its colour space, for the same reason
  (`DisplayInfo::into_parts`, `DisplayParts::into_info`).
- **`flui-native`**: `HeadlessDisplayControls::replace` reports a whole new set of displays, and
  `HeadlessDisplayControls::fail_next_read` fails the next display read, as a native display
  source does.
- **`flui-platform`**: `WindowStateParts` and `CapabilityParts`, the public fields of `WindowState`
  (apart from its revision) and of `WindowCapabilities` (apart from its owner and revision), for
  the same reason (`WindowState::into_parts`, `WindowStateParts::into_state`,
  `WindowCapabilities::into_parts`, `CapabilityParts::into_capabilities`).
- **`flui-native`**: `MockWindow::simulate_state` publishes a whole snapshot as the window system's
  report, through the window's successor rules.
- **`flui-runtime`**: `FrameFailureKind::DeviceLost { recovery }` reports a frame dropped on a lost
  GPU device, with `DeviceRecovery` (`Pending`, `Failed { attempts }`, `Abandoned { attempts }`)
  saying where the device rebuild stands; `UiRuntime::note_primary_device_recovery`,
  `UiRuntime::primary_device_recovery` and `UiRuntime::retry_render_device` carry it (ADR-0267).
- **`flui-app`**: `AppHandle::retry_render_device` asks every window whose device rebuild was given
  up to try again. `DeviceRecovery` is re-exported.

### Changed

- **`flui-app`**: a GPU device rebuild that keeps failing at the one-second retry ceiling is given
  up after ten such failures, about twelve seconds after the loss: the window reports
  `DeviceRecovery::Abandoned` once and attempts no more frames until the application asks again.
- **`flui-log`, `flui-testing`, `flui-painting`, `flui-assets`**: constructors that can fail are named `try_new` (`cargo xtask try-new` lists the rest):
  `flui_log::AppIdentity` and `AppleBundleId`, `flui_testing` `RecordingReader` and
  `RecordingWriter`, `flui_painting` `CornerRounding` and `RoundedPolygon`, and
  `flui_assets::NetworkLoader`.
- **`flui-objects`, `flui-interaction`**: the same for `AspectRatioFactor`, `FractionFactor`,
  `CarouselGeometry`, `RenderSliverConstrainedCrossAxis`, `MultiTapCount`, `OneEuroTuning` and
  `PolynomialFit`.

### Fixed

- **`flui-runtime`**: a frame dropped on a lost GPU device no longer asks for the next frame at
  once: it waits out the dropped-frame backoff, so a device that does not come back no longer
  costs a frame per vsync, and a headless run no longer spins on it.
