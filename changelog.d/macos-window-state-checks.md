### Fixed

- **`flui-native`**: a minimized AppKit window read `visible: false`, because AppKit reports a
  window in the Dock occluded. It now reads as shown, as the user sees it there.
- **`flui-native`**: `SetSize` on an AppKit window applied a content size below the window's
  minimum. The size is now held to the window's constraints, as on the other hosts.
