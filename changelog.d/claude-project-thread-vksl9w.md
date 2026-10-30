### Changed

- **A frame that keeps failing backs off.** After a dropped frame (a layout or paint
  panic, a pipeline error) a presentation waits its display's refresh period, doubled for
  each further consecutive drop, up to `flui_runtime::frame_failure::FAILURE_RETRY_CAP`
  (1 s), instead of retrying on every wake. A key press or pointer button press retries at
  once, once per wait.

### Added

- **`HeadlessHost::time_to_next_frame`, `pump_next_frame` and `set_window_display`**: wait
  as a host's loop does before its next frame, and deliver the display a window is on.
