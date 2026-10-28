### Added

- The native X11 host (`flui_native::X11Platform`) has input: XInput 2's mouse, touchpad, touch
  screen and pen (pressure and tilt, the eraser) with smooth scrolling in wheel detents, a
  touchpad's pinch on a server with XInput 2.4, the device hierarchy as devices come and go,
  XKB's keys with compose and the keyboard layout (`Platform::keyboard_layout`, XKB's name of
  the layout, and its change), and each window's cursor from the cursor theme
  (`PlatformWindow::set_cursor`, the core cursor font where no theme has it). Events carry the
  server's time while it keeps the monotonic clock. Its windows report pen input as available,
  and trackpad gestures on XInput 2.4; smooth scrolling has no phases.
