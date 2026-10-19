### Changed

- A window move reaches the runtime as a window-state change (ADR-0237): a published state whose
  position, display or frame differs from the previous one re-reads the window's gesture geometry
  and re-sends its input regions before any `WindowHandle` subscriber runs. The runners no longer
  register a separate move callback. An input recording keeps such a move as a `Moved`
  observation and never records the payload-free state notification.
- `HeadlessHost` installs a `WindowHandle` for each window it opens, and
  `HeadlessHost::move_to_display` publishes a state on the new display, as a runner does; a move to
  the display the window is already on publishes nothing.
- `MockWindow::simulate_move` takes a `Point` (the outer top-left corner), publishes the moved
  state, and is where `PlatformWindow::window_at` places the window under
  `HeadlessPlatform::with_window_placement`.
