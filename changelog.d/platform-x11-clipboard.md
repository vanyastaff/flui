### Added

- **`flui-native`**: the native X11 host (`X11Platform`) has a clipboard. It owns and reads the
  `CLIPBOARD` selection on a connection and a thread of its own, so an owner that answers slowly
  never holds up a frame. It answers `TARGETS`, `TIMESTAMP` and the text forms other X11
  applications ask for, and sends and receives large text in `INCR` chunks.
