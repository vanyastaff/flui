### Added

- **`WindowMetrics`** (`flui-foundation`): a window's drawable area as device pixels plus the
  ratio to logical pixels. Both extents are at least one pixel and the ratio is finite and
  positive, so a report that is not a drawable area has no value to carry. `PlatformWindow::metrics`
  reads the window's current one.

### Changed

- **Window resizes carry `WindowMetrics`** (`flui-platform-api`, `flui-platform`, `flui-runtime`):
  `PlatformWindow::on_resize`, `WindowObservation::Metrics` and `OwnerEffects::resize_surface`
  take the metrics instead of a logical size and a scale. The recording format is version 2: its `metrics` row
  holds the device-pixel size.

### Fixed

- **Swapchain one pixel short of the window** (`flui-app`): the native runners rebuilt the surface
  size as `logical × scale` truncated to pixels (115 px at 1.75 became 114). They configure it
  with the device pixels the platform reported.
- **Invalid window sizes reach layout** (`flui-platform`, `flui-runtime`): a zero, negative or
  non-finite size or scale is dropped where the backend reports it.
