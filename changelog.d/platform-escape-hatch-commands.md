### Added

- **`flui`**: `flui::native::NativeWindowCapability` and its handle `NativeWindow`: a widget
  acquires its window with `ctx.capability::<NativeWindowCapability>()` in `create_state` and runs
  a closure on it with `NativeWindow::with`, which answers `None` once the window has closed. The
  primary window is now reachable from a widget; `with_window` stays for code that holds a
  presentation's address. `examples/native_window.rs` uses it on Windows and macOS
  (`cargo run --example native_window --features native`).
- **`flui-native`**: on macOS, `AppKitWindowError`, `MacOSWindowAlpha` (with `InvalidWindowAlpha`),
  `MacOSSpaces`, `MacOSMissionControl` and `MacOSWindowCycle`.

### Changed

- **`flui-native`**: every `AppKitWindow` command (`enable_tabbing`, `disable_tabbing`,
  `set_window_level`, `set_collection_behavior`, `set_has_shadow`, `set_alpha`) returns
  `Result<(), AppKitWindowError>` and answers that the window is closed instead of doing nothing;
  `window_level` returns `Result<MacOSWindowLevel, AppKitWindowError>` and answers `UnnamedLevel`
  for a level no `MacOSWindowLevel` names instead of `Normal`.
- **`flui-native`**: `AppKitWindow::set_alpha` takes a `MacOSWindowAlpha`, which cannot be NaN or
  outside `0..=1`, and the window keeps it: a window opened with `WindowReveal::AfterFirstFrame` is
  revealed at the application's alpha instead of at 1. `AppKitWindow::alpha` reads it back.
- **`flui-native`**: `MacOSCollectionBehavior` is a struct of named choices (`spaces`,
  `mission_control`, `cycle`), each one AppKit accepts, and every window keeps the full-screen
  behavior `WindowMode::Fullscreen` needs. Its bit constants, which named the wrong AppKit flags,
  `from_bits` and `bits` are gone.
- **`flui-native`**: `Win32Window::set_title_bar_color` takes an `Option<Color>` instead of an
  `(u8, u8, u8)` tuple; the colour's alpha is not drawn.

### Removed

- **`flui-native`**: `MacOSWindowLevel::to_ns_value`, and the `TornOffMenu` and `FloatingPanel`
  levels (the first was `Floating`'s value, the second no AppKit level).
- **`flui-native`**: `AppKitWindow::toggle_native_fullscreen`; full screen is
  `WindowCommand::SetMode(WindowMode::Fullscreen)`, which keeps the window's state in step.
- **`flui-native`**: `AppKitWindow::backing_scale_factor`, `convert_point_from_backing` and
  `convert_point_to_backing`, and `Win32Window::convert_point_from_device` and
  `convert_point_to_device`; the scale is the window state's metrics.
- **`flui-native`**: `Win32Window::set_snap_layouts` and `snap_layouts`, which added or removed
  the maximize box the window's frame owns, and `Win32Window::set_blur_behind`, which has drawn no
  blur since Windows 8; `WindowsBackdrop::Acrylic` blurs what is behind a window.
- **`flui-native`**: `Win32WindowError::NotOnTheOwnerThread`, `TabJoinError::NotOnTheMainLane` and
  `MacOSBackdropError::NotOnTheMainLane`, which could not happen: a window is neither `Send` nor
  `Sync`, so its view is used only on the thread that made it.
