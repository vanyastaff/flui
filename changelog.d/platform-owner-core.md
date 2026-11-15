### Added

- `HeadlessInputDevices::set_keyboard_layout` in `flui-native`: a test switches the headless
  platform's keyboard layout and the registered layout callback hears it.

### Changed

- `Platform::on_keyboard_layout_change` replaces the callback registered before it on every host;
  on X11 and Wayland both used to be called on every change, forever.
- An application handler's `exit_policy` that panics is answered as `ExitAnswer::HostDecides` on
  every host: Win32 used to treat it as a veto (a loop with no window left then waited forever),
  AppKit quit, and X11 and Wayland unwound out of `run`.
- On Wayland, a handler's panic no longer drops the rest of the turn's input: every window still
  hears what the turn recorded, the clipboard and drag steps still run, and the first panic is
  raised after them, as on X11.
- `FLUI_HEADLESS=0` and an empty `FLUI_HEADLESS` no longer select the headless platform; any
  other value still does.

### Fixed

- A session end the application answered stays answered when a report delivered right after the
  answer panics; on Win32 a blocked session end used to become `Proceed`.
- A report the platform raised on the owner thread after `run` bound the application's handler
  and before `ready` ran reached the handler before its bootstrap; it now waits for `ready`.
