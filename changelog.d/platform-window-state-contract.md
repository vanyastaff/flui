### Added

- **`flui-platform-api`**: a window publishes an immutable `WindowState` snapshot with a
  `WindowRevision` (`PlatformWindow::state`, `on_state_changed`) and takes `WindowCommand`s
  through `PlatformWindow::apply`. `WindowCapabilities` (`sets_title`, `sets_position`,
  `sets_size`, `sets_constraints`, `sets_resizable`, `sets_decorated`, `minimizes`, `maximizes`,
  `enters_fullscreen`, `shows`, `hides`, `activates`, `observes_position`, `observes_minimized`,
  `places_at_open`; `NONE` and `ALL`) says which commands a window honours, `apply` answers
  `WindowCommandError::Unsupported` exactly when the capability is `false`, and
  `WindowCommand::validate` refuses non-finite or non-positive geometry before the OS is called
  ([ADR-0237](/docs/adr/ADR-0237-window-state-and-host-notices.md)). `PlatformWindow::state`,
  `capabilities` and `apply` have no default: a backend states them.
- **`flui-view`**: `LifecycleContext::window()` hands a lifecycle hook a `WindowHandle` that reads
  the window's state, queues checked commands (`set_title`, `set_position`, `set_size`,
  `set_constraints`, `set_resizable`, `set_decorated`, `set_mode`, `show`, `hide`, `activate`,
  `request_close`) and observes state changes (`subscribe`, `WindowSubscription`); it answers
  `WindowClosed` once the window closes, and `WindowCommandError` converts from it. `LifecycleHandle::subscribe_notices` delivers the host's
  `HostNotice`s (session ending, cancelled and ended, sleep, wake, session lock and unlock, memory
  pressure) with the same FIFO order and panic containment as `subscribe`. A subscriber to
  `HostNotice::SessionEnding` calls `SessionEndRequest::block` to hold the session end; the blocks
  of every presentation add up. The host answers the platform's query only after every subscriber
  has run (a query that arrives while the owner is busy is answered "proceed" with a diagnostic),
  the answer closes the request's book (`block` then returns `SessionEndBlockError::Answered`), and
  releasing the last block tells the platform (`Platform::release_session_end`).
- **`flui-app`**: `AppConfig::with_position` and `AppConfig::with_mode`, and `window_handle` to turn
  the address a `WindowRequest` delivers into the window's handle. Commands apply on the owner's
  next turn, before its frame, in the order they were queued.
- **`flui-platform-api`**: `WindowPlacement` remembers where a window was (the display by its
  `persistent_key`, the outer offset from its usable area in that display's logical pixels, the
  content size, the mode) as a versioned text encoding (`to_bytes`, `from_bytes`, `from_state`),
  and `resolve` fits it to the displays, capabilities and window frame of the next run, by the
  outer size: a missing display falls back to the primary one, a display with no usable scale or
  area is skipped, a window too large is cut to the usable area, one off the edge is moved back, a
  minimized one opens normal or maximized, and without `places_at_open` the size and mode stay but
  the position does not. `ResolvedPlacement` stays relative to its display.
- **`flui-native`**: `Platform::window_capabilities` and `Platform::window_frame_inset` say what
  a platform's windows can do and how large their frame is before one exists;
  `Platform::release_session_end`; `Platform::on_host_notice` takes a `SystemNotice` (the notices
  without a session end), and a `Wake` first marks the platform's displays and every window's state
  stale. Win32 places a window at open (`WindowOptions::{display, position, mode,
  normal_geometry}` through `SetWindowPlacement`), sizes it by its content, resizes a maximized or
  minimized window's normal bounds, and gives each display a `persistent_key`.
- **`flui-app`**: `AppConfig::restore_placement(name)` reads the placement before the first reveal
  and keeps it in the host storage, at most once every two seconds while the window moves (a change
  held back is written when the interval is over), and at once on close and when the session ends,
  within one budget for all windows. `AppConfig::with_memory_reclaim` registers an application
  cache (for example a `flui_assets::AssetRegistry`) to give back on memory pressure.
- **`flui-foundation`**: `memory::ReclaimMemory` (and `memory::reclaim_each`) for a holder of
  rebuildable memory, and `panic::FirstPanic` to run a list of callbacks and raise the first panic
  afterwards; `MemoryPressureLevel` now lives in `flui_foundation::memory`. On
  `HostNotice::MemoryPressure` the host reclaims its application receivers (`AssetRegistry` and
  others) and the raster thread's pools once, and each UI runtime its text service, before any
  subscriber hears the notice; `Critical` also evicts every pooled GPU buffer and resets the glyph
  atlas. `RasterHandle` and `AssetRegistry` implement `ReclaimMemory`.
- **`flui-testing`**: `window_conformance`, the checks every window backend runs against its
  `PlatformWindow`: capabilities are honest, accepted commands converge into the state, revisions
  change only with the state, bad numbers are refused, and a closed window refuses.
- **`WindowOptions`** gains `display`, `position`, `mode` and `normal_geometry`, is
  `#[non_exhaustive]` and is built with `with_*` methods, and its `size` is the content size;
  **`DisplayInfo`** gains `persistent_key` (`with_persistent_key`), a string that names a display
  across restarts.
- **`flui-native`**: the headless platform takes `with_window_capabilities` and
  `with_window_policy` (`HeadlessWindowPolicy`: a largest window size, a host that declines to
  maximize via `refusing_maximize`), and its windows gain `simulate_move`.

### Changed

- **`WindowCommandError::Backend`** carries `{ message }`.
- **`WindowOptions::size`** is the content size on every backend, not the outer size: reopening a
  window with the size it last reported no longer shrinks it by its frame on Win32.
- **`WindowMode`** is now `Normal`, `Minimized`, `Maximized` or `Fullscreen`, without the saved
  bounds the old enum carried.
- **`flui-native`**: `OpenWindowError::LaneFull` and `OwnerGone` carry the rejected
  `WindowOptions` boxed, and `SessionEndPhase` and `SessionEndAnswer` live in `flui-platform-api`.
  The session-end and notice callbacks are registered on `OwnerPlatform` and `SharedPlatform`
  (`on_session_end`, `on_host_notice`), and `AppConfig` converts to `WindowOptions` with its
  position and mode and without choosing `visible` or `reveal`.

### Removed

- **`PlatformWindow`** loses `set_title`, `get_title`, `minimize`, `maximize`, `restore`,
  `toggle_fullscreen`, `resize`, `bounds`, `content_size`, `window_bounds`, `is_maximized`,
  `is_fullscreen` and `on_moved`; `WindowEvent` loses `Moved`, `Minimized`, `Maximized`,
  `Restored`, `Fullscreen` and `ExitFullscreen`. Read the snapshot and apply a `WindowCommand`
  instead.
- **`flui-native`**: the second window trait `flui_native::window::Window` with
  `WindowBuilder` and `WindowManager`, the macOS `WindowManager`, and `WindowBounds`.
- **`flui-view`**: `WidgetsBindingObserver::did_have_memory_pressure` and `did_request_app_exit`,
  `WidgetsBinding::handle_memory_pressure` and `handle_request_app_exit`, and `AppExitResponse`.
  Memory pressure is `HostNotice::MemoryPressure`; the exit question is a session end that
  subscribers hold with a `SessionEndBlock`.
