### Changed

- Every window of a desktop application (Win32, AppKit, Wayland, X11) draws through a slot on the
  application's one GPU context and raster executor, instead of a `Renderer` with its own
  instance, device, pipelines and glyph atlas per window. A lost device is rebuilt once for every
  window, on the executor's backoff.
- `flui_app::testing::StandInRenderer`: the windows of a headless run share one device, as they
  share one GPU context, so `lose_device` fails every window's frames until a rebuild and
  `recovery_attempts` counts the run's rebuilds.
