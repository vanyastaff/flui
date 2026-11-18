### Fixed

- **`flui-native`**: on X11, Wayland and macOS, a window asked to change mode publishes the change
  once, from the mode it left to the mode asked: by `SetMode`, by a `SetSize` of a maximized or
  fullscreen window, or by the maximize toggle of a title bar the window draws. The steps the
  window manager, the compositor or AppKit's transition take on the way (a fullscreen window shown
  normal for a moment before it is maximized again, or maximized before it is normal) are no
  longer published as modes, while the window's size follows the window. The wait is bounded: on
  X11 and Wayland it ends a second after the request at most, on macOS three seconds while a
  transition animates and a second after it ends. A newer request replaces the wait of an older
  one, and a request for the mode a window is already in publishes nothing.
- **`flui-native`**: one case still publishes its steps. A minimized window asked for maximized or
  fullscreen publishes the modes it passes through as it is shown again (normal, then the mode
  asked): a window on screen is never published as minimized, and the window has not reached the
  mode asked yet.
- **`flui-native`**: while a mode change is under way, a snapshot that keeps the mode the window
  left names the rectangle the window returns to, and so does a normal snapshot of a window on its
  way back into normal mode. A remembered placement is therefore never a step's fullscreen-sized
  rectangle. On X11 a window maximized again out of fullscreen keeps the size it is restored to.
- **`flui-native`**: on macOS a full-screen transition or a zoom the user starts from the title bar
  is published the same way: into full screen, and back into the zoomed or normal state the
  window had.

### Changed

- **`flui-platform`**: `WindowPlacement::from_state` remembers a window by its
  `WindowState::normal_geometry` whenever the snapshot names one, in normal mode too, and by its
  outer rectangle otherwise.
