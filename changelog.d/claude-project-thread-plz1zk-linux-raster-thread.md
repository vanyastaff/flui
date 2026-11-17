### Changed

- **`flui-app`**: on Linux, the Wayland and X11 hosts alike, the desktop loop draws every window
  on its raster thread in debug and release builds: one frame presents while the next is built,
  and each window's wake settles what the thread finished
  ([ADR-0305](/docs/adr/ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)
  §10). Windows, macOS, the headless host and `flui_app::testing` stay inline. No environment
  variable chooses the placement.
- **`flui-native`**: the Wayland loop wakes for the application's wake deadline only once a
  window can take the frame it serves, so a window its compositor does not draw is not woken for
  at a deadline's pace while its frame callback is out.
