### Added

- `HeadlessHost::focus_window` (`flui-testing`) reports a window focused, as a backend's focus
  callback does, so a test can move keyboard input between windows.

### Fixed

- **Frame-aligned haptics played with frames nobody saw** (`flui-runtime`): a
  `HapticTiming::FramePresented` request now plays only with a frame the sink reports
  `Presented`; a frame the surface could not show (`NotShown`) or one with no damage
  (`NoPresent`, the screen keeps the previous frame) leaves it to a later presented frame or
  its deadline, where it resolves `Suppressed`.
- **Recorded keys named the window they were stamped for** (`flui-runtime`): a key is
  dispatched to the window that has keyboard focus, and its recording row now names that
  window, so a replay delivers it there. Keyboard, IME, drag-and-drop and capability rows name
  the window rather than the embedded surface that reported them.
