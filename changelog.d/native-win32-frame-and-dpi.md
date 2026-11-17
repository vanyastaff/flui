### Fixed

- A Win32 window keeps its content's logical size when it moves to a display of another scale or
  the scale changes under it: it answers `WM_GETDPISCALEDSIZE` with the frame measured at the new
  DPI, where the system's linear scaling left the content a few pixels off at each change. A
  fullscreen window comes back from fullscreen at its logical size after a scale change too.
- A maximized Win32 window with an application title bar no longer hides an auto-hide taskbar:
  it leaves two pixels at the taskbar's edge, so the taskbar slides in again, also after the
  taskbar's setting or edge changes while the window stays maximized.
- A maximized Win32 window without a frame covers its monitor's work area instead of the whole
  monitor, so the taskbar stays visible, and leaves an auto-hide taskbar its edge the same way.
- Several scale changes in a row bring a Win32 window back at the size it left, without a pixel
  of drift.
- A caption button of an application title bar stops showing hover once the pointer moves on to
  the title bar or a border, or leaves the window from the button.
- The caption buttons of an application title bar take the mouse where the process runs without
  `EnableMouseInPointer`, instead of the system tracking a classic button over them.
- The caption buttons the system draws on a Win32 window keep their non-client pointer messages
  (touch, pen, and the mouse under `EnableMouseInPointer`): they reach the system instead of being
  taken for the application's input.
