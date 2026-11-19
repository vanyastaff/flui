### Added

- A widget asks for any cursor: `MouseRegion::cursor` takes a `Cursor` (a `CursorIcon` as before,
  an application's `Cursor::Image`, or `Cursor::Hidden`), and the window shows it while the
  pointer is over the region. A window that refuses the image (it shows none, or its window
  system failed to make the cursor) shows the arrow there instead of the cursor of the region the
  pointer left. `Cursor` and `CursorImage` are re-exported from `flui-widgets`.
- The pointer locks on an iPad from iPadOS 14 (`WindowCommand::SetCursorGrab(CursorGrab::Locked)`,
  for a scene that fills its screen; another answers `ApplyError::Platform`) and on Android from
  API 26, with the mouse's motion arriving as `PointerEvent::RawMotion`; each lets the pointer go
  when the window loses the focus.
- `HeadlessWindow::without_cursor_images` plays a platform that shows no cursor image, and
  `HeadlessWindow::failing_cursor_images` one whose window system fails to make one.

### Changed

- `CursorRequest` is `Defer` or `Show(Cursor)`; `Icon(CursorIcon)` is gone. `HitTestResult::resolve_cursor`
  and `MouseTracker::device_cursor` return a `Cursor`, `CursorChangeCallback` receives a `&Cursor`,
  `HitTestEntry::cursor` takes `impl Into<Cursor>`, `RenderMouseRegion::cursor` returns a
  reference, and `HitTestOutcome` is no longer `Copy`.
- The Android Java classes state protocol 9: an APK packed with an older FLUI dex refuses to start.
