### Added

- **`flui-native`**: the native Wayland host (`WaylandPlatform`) takes drops on its windows
  through each seat's `wl_data_device` (up to version 3), from file managers, browsers and other
  Wayland clients. A drop's text, HTML, URI list and other media types are fetched on demand, each
  held to the request's limit and five seconds, and the source is told the action the target chose.
