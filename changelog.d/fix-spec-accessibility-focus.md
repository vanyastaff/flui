### Added

- **`flui-testing`**: `VirtualScript::press_button` presses a virtual pointer's other buttons (a
  dwell device's right click). `assert_gesture_alternatives` also drives the gesture with keyboard
  focus in the row, opens a menu by a virtual pointer's secondary click and by each pointer
  pressing the revealed control, and chooses a menu item with a virtual pointer.

### Fixed

- **`flui-widgets`**: with keyboard focus in a `GestureAlternatives` row, the focus ring no longer
  takes the pointer: the row's gesture (a swipe, a drag, a double tap, a pull) and its controls
  work under it.
- **`flui-widgets`**: the control a context-menu row reveals on keyboard or assistive-technology
  focus stays until focus leaves the row or a pointer presses the row off it, so a mouse, a
  finger or a virtual pointer can press it; before, the press hid it before its release.
