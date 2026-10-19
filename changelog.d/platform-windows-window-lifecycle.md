### Added

- **`flui-platform`, Windows**: a window opens hidden, in its opening mode and at its opening position,
  and is shown after the first presented frame with the show command of its mode, so a maximized or
  fullscreen window is first seen that way. `WindowOptions::resizable` and `decorated` set the frame's
  style bits, and `WindowCommand::SetResizable`, `SetDecorated`, `SetPosition`, `SetConstraints` and
  `Hide` now apply at run time. Min and max size hold at the operating system (`WM_GETMINMAXINFO`, in
  the window's DPI), and the snapshot reports the title, limits, `resizable` and `decorated` as the
  system has them.
- **`flui-platform`, Windows**: the platform delivers session end (`WM_QUERYENDSESSION` and
  `WM_ENDSESSION`, blocking through `ShutdownBlockReasonCreate` until the host releases it), sleep and
  wake (`WM_POWERBROADCAST`), session lock and unlock (`WM_WTSSESSION_CHANGE`) and `Critical` memory
  pressure (`CreateMemoryResourceNotification`) through `Platform::on_session_end`, `on_host_notice` and
  `release_session_end`.

### Fixed

- **`flui-platform`, Windows**: fullscreen entered from a maximized or minimized window restores the
  window first and no longer flags it maximized on the way back; a resize no longer ends fullscreen,
  and a minimized fullscreen window comes back fullscreen; leaving fullscreen keeps a shown window
  shown.
