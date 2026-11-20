### Added

- `flui-driver` has a Windows lane, `lane::windows::WindowsLane`: real input through `SendInput`
  (mouse, wheel, keys with their scan codes, text as `VK_PACKET`) and synthetic touch and pen
  (`InjectSyntheticPointerInput`), refused off the desktop that takes the user's input, out of the
  foreground, over another window, with physical input held and on a shell hotkey; a user-activity
  guard on the low-level hooks and the digitizers' raw input; the window's UI Automation tree,
  projected onto the wire's nodes and checked against the app's (`Window::os_tree`,
  `Window::compare_projection`); activation through UI Automation (`Window::os_invoke`); Windows
  Graphics Capture of the window (`Window::capture_os`); and a kill-on-close job for every app it
  launches.
- `Lane` gains `check`, asked before a sequence's first event, and `read_tree`, `invoke` and
  `capture_window`, each `not_supported` by default.
- `flui_protocol::tree::normalized_outline`, the projection of ADR-0095 §4, and
  `FRAMEWORK_IDENTIFIER_PREFIX`, the namespace of the identifiers the framework publishes.

### Fixed

- A focused text field on `flui-driver`'s headless target takes the text `element.type_text` and
  an input-method composition commit: the headless window offers text input, as a native one does.
