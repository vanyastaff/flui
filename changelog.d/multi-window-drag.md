### Added

- **`flui-platform-api`**: `PlatformWindow::window_at` answers which window of the platform
  instance has its content topmost under a point given in one window's coordinates (`WindowAt`).
  Win32 implements it through the desktop's stacking order; the other backends answer
  `WindowAt::Unknown`. **`flui-platform`**: `HeadlessPlatform::with_window_placement` and
  `MockWindow::simulate_move` place headless windows on one desktop.

### Changed

- **`flui-interaction`**: a drag session that leaves its window over another window of its own
  runtime stays on its source's pointer capture and reaches that window's receivers without an OS
  session; it escalates only for what lies outside the runtime, or where the backend cannot place
  its windows ([ADR-0211](/docs/adr/ADR-0211-unified-drag-and-drop.md) §2). Closing a window a drag
  hovers leaves its receivers at once.
- **`flui-interaction`**: `DragOwner::attach_presentation` takes the presentation's window and hit
  test, and a session started without `DragStart::hit_test` hit-tests its source presentation's
  tree.
