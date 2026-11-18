### Fixed

- **`flui-native`**: on X11, Wayland and macOS, a window asked to change mode publishes the change
  once, from the mode it left to the mode asked. The steps the window manager, the compositor or
  AppKit's full-screen transition take on the way (a fullscreen window shown normal for a moment
  before it is maximized again, or maximized before it is normal) are no longer published as
  modes: until the window system reaches the mode asked, or a second has passed, the window's
  state keeps the mode it left, while its size follows the window. On X11 a window maximized
  again out of fullscreen keeps the size it is restored to. On macOS a full-screen transition the
  user starts with the green button is published the same way: into full screen, and back into
  the zoomed or normal state the window had.
