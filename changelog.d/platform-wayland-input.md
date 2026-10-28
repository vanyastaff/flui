### Added

- The native Wayland host (`WaylandPlatform`) delivers input from the compositor's seats: keys
  through the compositor's XKB keymap (libxkbcommon, loaded at run time) with modifiers and
  client-side repeat, the keyboard layout's name and its changes (`keyboard_layout`,
  `on_keyboard_layout_change`), pointers with wheel steps in detents and phased touchpad
  scrolling, touchscreens in pointer frames, pens and erasers through `tablet-v2`, touchpad
  pinches through `pointer-gestures`, and each window's cursor through `cursor-shape`, or from the
  cursor theme (`XCURSOR_THEME`, `XCURSOR_SIZE`) where the compositor has none. A window's
  capability snapshot follows the compositor's globals and the devices present. `current_platform`
  still picks winit while it is built: the native host has no text input, clipboard or drag and
  drop yet.
