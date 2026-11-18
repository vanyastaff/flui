### Changed

- **`flui-native`**: a window's log events have one message and one field set on Win32 and
  AppKit, the native handle always under `handle`: `a window opened` (`handle`, `width`,
  `height`, `scale`), `window moved` (`handle`, `x`, `y`), `window resized` (`handle`, `width`,
  `height`) and `window scale changed` (`handle`, `scale`; Win32's `DPI changed` with its `dpi`
  field is gone). The formatted log lines of the Android loop, the X11 clipboard thread and the
  direct render mode carry their values as fields.
- **`flui-native`**, level changes: Android's `window resized` from `info` to `debug`; the
  `a window is opening` line of Win32, the Web and the headless host from `info` to `debug`;
  AppKit's `window resized` from `debug` to `trace`.

### Fixed

- **`flui-native`**, **`flui-app`**: the application's start-up lines and the window-opening
  lines of the Win32, Web and headless hosts no longer log the window's title; a URI list that
  leaves a path out no longer logs the path, and the Linux single-instance forwarder logs a
  relative path it refuses by its extension only.
