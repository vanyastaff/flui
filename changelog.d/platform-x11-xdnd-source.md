### Added

- **`flui-native`**: a drag from a window of the native X11 host (`X11Platform`) that leaves the
  window becomes an XDND session, so file managers, browsers and other X11 applications can take
  it. The window offers the drag's text and other formats, finds the target under the pointer,
  drops where the target accepts, and leaves on refusal or Escape. A payload larger than one X
  request is not offered yet.
