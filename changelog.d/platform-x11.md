### Added

- `flui_native::X11Platform`, the native X11 host: one libxcb connection read by a loop on the
  thread that builds it, which waits with no timeout but the application's wake deadline;
  windows that publish what the X server and the window manager hold and take every window
  command, honouring what the window manager says it supports; the RandR displays, the
  Settings portal, logind and memory pressure; and an AT-SPI bridge on every window. Input,
  text input, the clipboard and drag and drop are not on it yet, and `current_platform` still
  picks the winit host on Linux.
- `flui_native::testing::x11::run_for`, which turns the X11 loop by hand for a test.

### Changed

- flui-native's `desktop` feature, a default one, builds the X11 host on Linux (the `x11`
  feature). `zbus` and `accesskit_unix` are Linux dependencies of every build, no longer
  behind `winit-backend`.
