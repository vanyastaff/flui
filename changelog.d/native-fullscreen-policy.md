### Changed

- **`flui-native`**: pressing F11 no longer switches a Windows window to fullscreen. The Windows
  host had F11 built in, on Windows only, and an application that handled F11 without preventing
  its default also switched fullscreen. No host now switches fullscreen on its own decision: an
  application that wants a key for it calls `WindowHandle::set_mode(WindowMode::Fullscreen)`, and
  the mode to go back to, from its own key handling. A change the system makes itself (the macOS
  green button, the browser's Escape) is still reported in the window's state.

### Fixed

- **`flui-native`**: on Windows, a window leaving fullscreen goes to the mode asked in one step:
  maximized with the size it had before fullscreen as the one it restores to, without flashing
  that size on the way, and a minimized fullscreen window no longer shows fullscreen on its way
  to another mode. Its state reads only the mode it left and the mode asked. A window whose
  monitor was disconnected while it was fullscreen comes back on a monitor that exists. No report
  of the round trip says the user placed the window, so a remembered placement is not overwritten
  as the user's.
