### Fixed

- **`flui-native`**: an AppKit window sent back to normal mode from the Dock and made fixed-size in
  the same turn was refused: it still read as miniaturized while it animated. A fixed size is now
  admitted for the mode the window is heading to and set once it gets there.
