### Added

- **Title bar widgets** (`flui-widgets`): `WindowTitleBar` marks its box as the window's move
  area, `WindowCaptionButton::{minimize, maximize, close}` draws a caption button that acts on
  the window through its `WindowHandle` and announces what it does now ("Maximize" or
  "Restore"), `WindowResizeArea` marks a border the window is resized from, and
  `WindowMovePassThrough` keeps a subtree's presses moving the window. They mark nothing while
  the system draws the title bar (ADR-0286 §6). `WidgetsLocalizations` gains
  `minimize_window_label`, `maximize_window_label`, `restore_window_label` and
  `close_window_label`.
- **Press takers leave a move area** (`flui-rendering`, `flui-objects`):
  `InputRegionSink::claim_presses` and `pass_presses_through`; a `RenderListener` with a
  pointer-down handler or gesture recognizers is cut out of a move area above it as
  `WindowContent`, and `RenderPressPassThrough` keeps its subtree moving the window
  (ADR-0286 §5). `RenderListener::set_claims_presses` says which listeners take presses.

### Changed

- **Test kit window** (`flui-testing`): the kit's `HeadlessWindow` honours frame and mode
  commands (`sets_frame`, `app_title_bar`, `minimizes`, `maximizes` are `Native`), reports them
  in its state from the next pump, and counts the close requests its `WindowHandle` receives
  (`HeadlessWindow::close_requests`).
