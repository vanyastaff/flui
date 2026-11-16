### Removed

- **`flui-native`**: a Windows window no longer switches to fullscreen on F11. The host had
  that key built in, on Windows only, and an application that handled F11 without preventing
  its default also left or entered fullscreen. Fullscreen is now `WindowCommand::SetMode` on
  every host, and binding a key to it is up to the application.

### Fixed

- **`flui-native`**: a fullscreen Windows window given `SetMode(Maximized)` goes straight to
  maximized, with the size it had before fullscreen as the one it restores to. It publishes
  one snapshot and does not show its normal size along the way. No report of the round trip
  says the user placed the window, so a remembered placement is not overwritten as the
  user's.
