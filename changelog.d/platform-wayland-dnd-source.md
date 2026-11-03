### Added

- **`flui-native`**: the native Wayland host (`WaylandPlatform`) starts drag sessions from its
  windows through each seat's `wl_data_device` (version 3), into file managers, browsers and
  other Wayland clients. The session's text, HTML, URI list and other media types are served on
  demand from its provider, and it ends with the action the target took, or cancelled.
