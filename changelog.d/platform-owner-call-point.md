### Changed

- **`flui-native`**: `InputCallback` and `InputWake` are named at the crate root, beside
  `PlatformHandlers` and `WindowCallbacks`; `flui_native::shared` no longer names any of the four.

### Removed

- **`flui-native`**: `PlatformHandlers`' `invoke_*` methods and `run_session_end`. The platform
  alone calls a handler, through one call point that holds none of its locks
  ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md) §10).

### Fixed

- **`flui-native`**: an Android quit handler no longer runs while the platform holds its handler
  lock, so it can register another handler or call back into the platform when the Activity's
  loop ends.
- **`flui-native`**: the headless platform announces a new window (`WindowEvent::Created`) after it
  releases its state, so a window-event handler can open another window, register a handler or
  quit from inside that event instead of deadlocking.
- **`flui-native`**: replacing a handler (`on_quit`, `on_window_event`, `on_session_end`,
  `on_reopen`, `on_keyboard_layout_change`, the exit-policy and wake-deadline hooks, and every
  window callback such as `on_request_frame`) drops the replaced one after the platform released
  its locks: a handler whose captured state calls the platform from its destructor no longer
  deadlocks Android, iOS, macOS, winit or the headless platform. The same holds when a
  session-end or system-notice hook replaces itself from inside its call.
- **`flui-native`**: a handler that replaces itself and then panics keeps that panic as the failure
  reported, and its captures are kept rather than dropped into a second failure; Win32's
  exit-policy and session-end hooks and AppKit's hooks dropped them.
