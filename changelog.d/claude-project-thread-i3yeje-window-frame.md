### Changed

- **Window frame** (`flui-platform`, `flui-view`, `flui-app`, `flui-native`, `flui-protocol`):
  `WindowFrame { System, AppTitleBar, None }` replaces the window's `decorated` flag:
  `WindowOptions::frame`, `WindowCommand::SetFrame`, `WindowState::frame`,
  `WindowCapabilities::sets_frame`, `WindowHandle::set_frame` and `AppConfig::frame` take the
  place of `decorated`, `SetDecorated`, `sets_decorated`, `set_decorated` and `decorations`.
  `WindowFrame::AppTitleBar` is a title bar the application draws inside a frame the system
  keeps, admitted where the new `WindowCapabilities::app_title_bar` is available; only the
  headless host takes it so far, and a window asking for it elsewhere opens with the system's
  frame (`WindowFrame::at_open`). `AppTitleBar` keeps, moves (`ControlsOffset`) or hides the
  system's own controls (`SystemControls`).
- **Native recording format 2** (`flui-protocol`): a window snapshot records `frame` in place of
  `decorated`, and its capabilities `sets_frame` and `app_title_bar` in place of
  `sets_decorated`; a format 1 recording is refused.
