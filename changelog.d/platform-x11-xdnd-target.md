### Added

- **`flui-native`**: the native X11 host (`X11Platform`) takes drops on its windows over XDND
  (version 5), from file managers, browsers and other X11 applications. A drop's text, HTML, URI
  list and other media types are fetched on demand, large ones in `INCR` chunks, each held to the
  request's limit.
