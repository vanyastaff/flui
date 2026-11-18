### Fixed

- **`flui-native`**: on X11, Wayland and macOS, a window asked to change mode publishes the change
  once, from the mode it left to the mode asked. The steps the window manager, the compositor or
  AppKit's full-screen transition take on the way (a fullscreen window shown normal for a moment
  before it is maximized again, or maximized before it is normal) are no longer published as
  modes, while the window's size follows the window. The wait is bounded: on X11 and Wayland it
  ends a second after the request at most, on macOS three seconds while the transition animates
  and a second after it ends. A newer request replaces the wait of an older one, and a window
  restored from minimized is published at once. Every snapshot of the change names the
  rectangle the window returns to, so a remembered placement is never a step's fullscreen-sized
  rectangle. On X11 a window maximized again out of fullscreen keeps the size it is restored to.
  On macOS a full-screen transition the user starts with the green button is published the same
  way: into full screen, and back into the zoomed or normal state the window had.

### Changed

- **`flui-platform`**: `WindowPlacement::from_state` remembers a window by its
  `WindowState::normal_geometry` whenever the snapshot names one, in normal mode too, and by its
  outer rectangle otherwise.
