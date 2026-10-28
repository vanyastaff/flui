### Added

- `flui_platform::SurfaceTarget` (`HasWindowHandle + HasDisplayHandle + Send + Sync + 'static`,
  with `pre_present_notify`) and `PlatformWindow::surface_target`: what the thread that presents
  a window's frames holds. It answers `HandleError::Unavailable` once the window is gone, and on
  AppKit and UIKit off the main thread and on Android off the looper thread; it does not keep the
  window open, but keeps alive what its handle names. `flui_platform::testing::NoNativeSurface` is
  the target of a window with no native surface.
- `flui_platform::RedrawRequester` and `PlatformWindow::redraw_requester`: how a thread that is
  not the window's owner asks it for a frame. Any `Fn() + Send + Sync + 'static` closure is one.
- `flui_native::OpenedWindow`, what a deferred window open resolves to: the window's identity on
  any thread, and through `window()` an `OpenedWindowReach` that is the window on its owner
  thread, `Closed` once it closed, and `OtherThread` elsewhere.
- `flui_platform::testing`, the module a test double's window is written with.

### Changed

- `PlatformWindow` is neither `Send` nor `Sync`: windows are handed out as
  `Rc<dyn HostWindow>` / `Rc<dyn PlatformWindow>`, and a window method called from another
  thread no longer compiles.
- A renderer is built from `window.surface_target()`, not from the window.
- `PendingWindow::wait` returns the opened window's `WindowId`; `try_take` and the future yield an
  `OpenedWindow`.
- A redraw asked of a macOS window from another thread is queued on its owner lane and returns at
  once.

### Removed

- `PlatformWindow::pre_present_notify`, `PlatformWindow::window_handle` and
  `PlatformWindow::display_handle`, and the `HasWindowHandle`/`HasDisplayHandle` impls on
  `dyn PlatformWindow` and `dyn HostWindow`: raw handles come from the surface target.
