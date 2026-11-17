### Changed

- **`flui-app`**: on the Wayland host the desktop loop draws every window on its raster thread
  in debug and release builds: one frame presents while the next is built, and each window's
  wake settles what the thread finished
  ([ADR-0305](/docs/adr/ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)
  §10). X11 stays inline until a run on a real X server, as do Windows, macOS, the headless host
  and `flui_app::testing`. No environment variable chooses the placement. A window whose frames
  the raster thread has settled no longer wakes the loop every 50 ms while it waits for its next
  frame.
- **`flui-native`**: the Wayland loop wakes for the application's wake deadline only once a
  window can take the frame it serves, so a window its compositor does not draw is not woken for
  at a deadline's pace while its frame callback is out.
