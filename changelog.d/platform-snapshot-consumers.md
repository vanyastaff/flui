### Added

- `WindowCommand::Close` and `WindowCapabilities::closes`: closing a window is a command like
  `Show` and `Activate`. It is native on Win32, macOS, iOS, winit and the headless host, emulated
  on the Web (the canvas retires), and unavailable on Android, whose one window the application
  cannot close.
- `WindowState::is_seen`: the window is visible and not minimized.
- `CloseRequestError::Refused`: `request_presentation_close` reports a window that refused to close
  (an Android window) instead of answering `Ok`.
- `flui_native::HeadlessWindowSystem` and `MockWindow::window_system`: what the headless window
  system holds for a window (metrics, focus, visibility), for a conformance fixture to
  read back.

### Changed

- The Web window reads its state again on each owner wake, so a move to another screen reaches
  its snapshot's `display`.

### Removed

- The window getters (`is_focused`, `is_visible`, `physical_size`, `logical_size`, `scale_factor`,
  `metrics`, `appearance`, `display_id`, `safe_area_insets`, `execution_state`, `is_active`,
  `is_hovered`, `capabilities`, `mouse_position`, `modifiers`) and their callbacks (`on_resize`,
  `on_active_status_change`, `on_visibility_status_change`, `on_hover_status_change`,
  `on_appearance_changed`, `on_display_changed`, `on_safe_area_change`,
  `on_execution_state_change`): read `PlatformWindow::state` and listen with `on_state_changed`,
  comparing the snapshot with the last one seen to tell which fact changed. Pointer position and
  modifiers come from the input stream.
- `PlatformWindow::show`, `activate` and `close`, and `WindowShowError`: apply
  `WindowCommand::Show`, `Activate` or `Close`.
- `WindowEvent` and `Platform::on_window_event`, which no consumer above the host used.
- `WindowControl::capabilities` and `WindowHandle::capabilities`: the capabilities are a field of
  the window's state.
