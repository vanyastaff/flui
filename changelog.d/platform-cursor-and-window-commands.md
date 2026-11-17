### Added

- A window shows a cursor image of the application's or no cursor at all over the region that
  asks for it: `PlatformWindow::set_cursor` takes a `Cursor` (`Icon`, `Image`, `Hidden`), and
  `CursorImage` checks its size, pixels and hotspot where it is built. Windows, macOS, Wayland,
  X11 and the Web show images (`WindowCapabilities::cursor_image`).
- A window holds the pointer inside its content area or in place while it has the keyboard
  focus: `WindowCommand::SetCursorGrab(CursorGrab::{None, Confined, Locked})`,
  `WindowHandle::set_cursor_grab`, and the snapshot's `WindowState::cursor_grab`, which shows
  the grab ending when the focus goes. A locked pointer is hidden and its motion arrives as
  `PointerEvent::RawMotion` (`MotionDelta`, `MotionAcceleration`) instead of moves.
- A window is kept above or below the others (`WindowCommand::SetLevel`, `WindowLevel`,
  `WindowState::level`), and the user's resize keeps steps and a proportion
  (`WindowCommand::SetResizeIncrements`, `WindowCommand::SetAspectRatio`, `AspectRatio`), with
  their `WindowHandle` methods and a capability each, absent with its reason where the window
  system has none.
- `WindowInteraction::UserResizing` says the user is resizing the window by its frame (Win32,
  macOS, Wayland), and `WindowInteraction::is_user` reads either of the user's interactions.

### Changed

- `PlatformWindow::set_cursor` takes `&Cursor` instead of a `CursorIcon`; a window built outside
  the hosts passes `&Cursor::Icon(icon)` on. `CursorError` moved beside `Cursor` at the same path.
- The agent protocol is 0.18 and the input recording format 5: a recording may carry a locked
  pointer's `raw_motion`, which older clients and readers refuse.

### Fixed

- On macOS and Wayland, a window's placement is remembered after the user resizes it by its
  frame, as on Windows.
