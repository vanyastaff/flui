### Added

- **`flui`**: a widget reaches the window it is presented in: it acquires
  `flui::native::NativeWindowCapability` with `ctx.capability::<NativeWindowCapability>()` in
  `create_state` and runs a closure on the window with `NativeWindow::with`, which answers `None`
  once the window has closed. The primary window is reachable from a widget; `with_window` stays
  for code that holds a presentation's address. An application's test stands in for it with
  `Application::capability` and `NativeWindow::absent()`. `examples/native_window.rs` uses it on
  Windows and macOS (`cargo run --example native_window --features native`).

### Changed

- **`flui-native`**: every `AppKitWindow` command (`enable_tabbing`, `disable_tabbing`,
  `set_window_level`, `set_collection_behavior`, `set_has_shadow`, `set_alpha`) returns
  `Result<(), AppKitWindowError>` and answers that the window is closed instead of doing nothing;
  `window_level` returns `Result<MacOSWindowLevel, AppKitWindowError>` and answers `UnnamedLevel`
  for a level no `MacOSWindowLevel` names instead of `Normal`. `add_tab_to_window` answers a
  closed window as the others do, `TabJoinError::Unsupported` with `NoWindow`, in place of
  `TabJoinError::Closed`.
- **`flui-native`**: `AppKitWindow::set_alpha` takes a `MacOSWindowAlpha`, which cannot be NaN or
  outside `0..=1`, and the window keeps it: a window opened with `WindowReveal::AfterFirstFrame` is
  revealed at the application's alpha instead of at 1. `AppKitWindow::alpha` reads it back.
- **`flui-native`**: `MacOSCollectionBehavior` is a struct of named choices (`spaces`,
  `mission_control`, `cycle`), each one AppKit accepts, and every window keeps the full-screen
  behavior `WindowMode::Fullscreen` needs. Its bit constants, which named the wrong AppKit flags,
  `from_bits` and `bits` are gone.
- **`flui-native`**: `Win32Window::set_title_bar_color` takes an `Option<Color>` instead of an
  `(u8, u8, u8)` tuple, and refuses a colour that is not opaque with `Win32WindowError::NotOpaque`:
  the desktop window manager draws a title bar without alpha.

### Removed

- **`flui-native`**: `MacOSWindowLevel::to_ns_value`, and the `TornOffMenu` and `FloatingPanel`
  levels (the first was `Floating`'s value, the second no AppKit level).
- **`flui-native`**: `AppKitWindow::toggle_native_fullscreen`; full screen is
  `WindowCommand::SetMode(WindowMode::Fullscreen)` (`WindowHandle::set_mode`), which keeps the
  window's state in step.
- **`flui-native`**: `AppKitWindow::backing_scale_factor`, `convert_point_from_backing` and
  `convert_point_to_backing`, and `Win32Window::convert_point_from_device` and
  `convert_point_to_device`; the scale is the window state's metrics.
- **`flui-native`**: `Win32Window::enable_snap_layouts`, `disable_snap_layouts` and
  `is_snap_layouts_enabled`, which added or removed the maximize box the window's frame owns:
  Windows offers Snap Layouts over that box, which a resizable window has; make the window
  resizable or not with `WindowCommand::SetResizable` (`WindowHandle::set_resizable`), and read it
  in `WindowState::resizable`.
- **`flui-native`**: `Win32Window::enable_blur_behind`, which has drawn no blur since Windows 8;
  `WindowsBackdrop::Acrylic` (`Win32Window::set_backdrop`) blurs what is behind a window.
- **`flui-native`**: `TabJoinError::NotOnTheMainLane`, which could not happen: an AppKit window is
  made on the main thread and is neither `Send` nor `Sync`.
