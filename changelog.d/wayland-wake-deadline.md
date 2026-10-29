### Fixed

- **`flui-native` (Wayland)**: a wake deadline the application answers again with the same instant
  after a frame served it gets its frame. The Wayland loop offered an instant once and never again,
  so a queue of obligations due at one instant stalled after the first; it now follows the X11 and
  Win32 loops' rule and arms the instant again whenever a frame ran since its delivery. A deadline
  no frame serves still leaves the loop idle.
