### Added

- **`WindowMetrics`** (`flui-foundation`): a window's drawable area as device pixels plus the
  ratio to logical pixels. Both extents are at least one pixel and the ratio is finite and
  positive, so a report that is not a drawable area has no value to carry. `PlatformWindow::metrics`
  reads the window's current one.

### Changed

- **Window resizes carry `WindowMetrics`** (`flui-platform`, `flui-native`, `flui-runtime`):
  `PlatformWindow::on_resize`, `WindowObservation::Metrics` and `OwnerEffects::resize_surface`
  take the metrics instead of a logical size and a scale. The recording format is version 2: its `metrics` row
  holds the device-pixel size.

### Fixed

- **Swapchain one pixel short of the window** (`flui-app`): the native runners rebuilt the surface
  size as `logical × scale` truncated to pixels (115 px at 1.75 became 114). They configure it
  with the device pixels the platform reported.
- **Invalid window sizes reach layout** (`flui-native`, `flui-runtime`): a zero, negative or
  non-finite size or scale is dropped where the backend reports it.
- **Animation frozen on a second window's late wake** (`flui-app`): the desktop fallback deadline
  was dropped when the loop first read it more than a display period late, for instance while
  another window blocked in `present`, which parked a window whose ticker still ran. A deadline
  is now dropped only after it was reported due and went a full period unanswered.
- **Held window placement written while minimized** (`flui-app`): the write throttle's deadline
  no longer waits for frames, so a placement moved and then minimized is stored when its
  interval is over, not when the window is restored.
- **Text sizing woke the owner twice a second in a backgrounded app** (`flui-app`, Android and
  iOS): an unavailable native window now parks the capture without a deadline, and failing native
  calls back off from 500 ms to 8 s with one warning per streak.
- **Error view factory is UI runtime configuration** (`flui-view`, `flui-runtime`, `flui-app`):
  `set_error_view_builder`, `clear_error_view_builder` and the process-wide `ERROR_VIEW_BUILDER`
  are gone. Set the factory with `AppConfig::with_error_view_builder` (or
  `UiRuntime::set_error_view_builder` / `BuildOwner::set_error_view_builder`); it applies to every
  presentation of the runtime, including windows opened later, and two runtimes in one process no
  longer share it.
