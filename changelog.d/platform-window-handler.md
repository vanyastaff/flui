### Added

- `WindowHandler`: what a window delivers, one required method per obligation (`input`,
  `state_changed`, `frame_requested`, `close_requested`, `closed`, `surface_changed`), each called
  on the thread that opened the window. `CloseDecision { Close, KeepOpen }` answers a
  close request; `SurfaceChange { Release, Rebuild }` names a surface change.
- `WindowRequest` and `WindowHandlerFactory`: a window opened from another thread through
  `PlatformProxy::open_window` carries a `Send` factory that builds its handler on the owner
  thread.
- `AppConfig::input_latency` and `AppConfig::with_input_latency`: every window the application
  opens asks for that input latency (read by the web and Android hosts).
- `flui_testing::window_handler`: `RecordingWindowHandler`, which records each call and reacts as a
  test tells it, and `HandlerCall`.

### Changed

- `Platform::open_window`, `OwnerPlatform::open_window` and the core host's window constructors take
  the window's `Rc<dyn WindowHandler>`; the capability snapshot is the handler's first input,
  delivered before the open returns. `PlatformProxy::open_window` takes a `WindowRequest`.
- A panicking handler method no longer drops what the window queued behind it: the rest is
  delivered and the first panic resumes afterwards.
- iOS: the platform's `on_scene_event` also takes what makes the handler of each session's window,
  asked before UIKit's scene gets the window; a scene whose handler cannot be made is refused.
- `MockWindow::simulate_surface_change(SurfaceChange)` replaces `simulate_surface_status(bool)`.

### Removed

- `PlatformWindow::on_input`, `on_state_changed`, `on_request_frame`, `on_close`,
  `on_should_close` and `on_surface_status_change`, with the core host's `WindowCallbacks`,
  `InputCallback` and `InputWake`. The host no longer holds input for a callback registered
  later: the handler exists from the open. An application window's input, state change, frame
  request and surface change that arrive before its presentation exists are kept by the runner
  and handed to the presentation in order; a close request in that gap keeps the window.
