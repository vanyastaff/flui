### Added

- The native X11 host (`flui_native::X11Platform`) has input: XInput 2's mouse, touchpad, touch
  screen and pen (pressure and tilt, the eraser) with smooth scrolling in wheel detents, the
  device hierarchy as devices come and go, XKB's keys with compose and the keyboard layout
  (`Platform::keyboard_layout` and its change), and each window's cursor from the cursor theme
  (`PlatformWindow::set_cursor`, the core cursor font where no theme has it). Its windows report
  pen input as available; smooth scrolling has no phases and there are no trackpad gestures.
