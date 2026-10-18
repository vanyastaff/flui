### Added

- **`flui-platform-api`**: a window publishes an immutable `WindowState` snapshot with a
  `WindowRevision` (`PlatformWindow::state`, `on_state_changed`) and takes `WindowCommand`s
  through `PlatformWindow::apply`. `WindowCapabilities` says which commands a window honours,
  `apply` answers `WindowCommandError::Unsupported` exactly when the capability is `false`, and
  `WindowCommand::validate` refuses non-finite or non-positive geometry before the OS is called
  ([ADR-0237](/docs/adr/ADR-0237-window-state-and-host-notices.md)).
- **`flui-view`**: `LifecycleContext::window()` hands a lifecycle hook a `WindowHandle` that reads
  the window's state, queues checked commands (`set_title`, `set_position`, `set_size`,
  `set_constraints`, `set_resizable`, `set_decorated`, `set_mode`, `show`, `hide`, `activate`,
  `close`) and observes state changes (`subscribe`, `WindowSubscription`); it answers
  `WindowClosed` once the window closes. `LifecycleHandle::subscribe_notices` delivers the host's
  `HostNotice`s (session ending, cancelled and ended, sleep, wake, session lock and unlock, memory
  pressure) with the same FIFO order and panic containment as `subscribe`. A subscriber to
  `HostNotice::SessionEnding` calls `SessionEndRequest::block` to hold the session end; the blocks
  of every presentation add up.
- **`flui-app`**: `AppConfig::with_position` and `AppConfig::with_mode`, and `window_handle` to turn
  the address a `WindowRequest` delivers into the window's handle. Commands apply on the owner's
  next turn, before its frame, in the order they were queued.
- **`flui-platform-api`**: `WindowPlacement` remembers where a window was (the display by its
  `persistent_key`, the offset from its usable area, the size, the mode) as a versioned text
  encoding, and `resolve` fits it to the displays and capabilities of the next run: a missing
  display falls back to the primary one, a window too large is cut to the usable area, one off the
  edge is moved back, a minimized one opens normal or maximized, and without `restores_position`
  the size and mode stay but the position does not.
- **`flui-app`**: `AppConfig::restore_placement(name)` reads the placement before the first reveal
  and keeps it in the host storage, at most once every two seconds while the window moves and at
  once on close and when the session ends. `AppConfig::with_memory_trim` registers an application
  cache (for example a `flui_assets::AssetRegistry`) to give back on memory pressure.
- **`flui-foundation`**: `memory::MemoryTrim` (and `memory::trim_all`) for a holder of rebuildable
  memory; `MemoryPressureLevel` now lives in `flui_foundation::memory`. On
  `HostNotice::MemoryPressure` the UI runtime trims its text service, `AssetRegistry`, the
  registered receivers and the raster thread's pools before any subscriber hears the notice;
  `Critical` also evicts every pooled GPU buffer and resets the glyph atlas. `RasterHandle` and
  `AssetRegistry` implement `MemoryTrim`.
- **`flui-testing`**: `window_conformance`, the checks every window backend runs against its
  `PlatformWindow`: capabilities are honest, accepted commands converge into the state, revisions
  change only with the state, bad numbers are refused, and a closed window refuses.
- **`WindowOptions`** gains `position`, `mode` and `normal_geometry`; **`DisplayInfo`** gains
  `persistent_key` (`with_persistent_key`), a string that names a display across restarts.
- **`flui-platform`**: the headless platform takes `with_window_capabilities` and
  `with_host_limits` (`HostLimits`: a largest window size, a host that declines to maximize), and
  its windows gain `simulate_move`.

### Changed

- **`WindowMode`** is now `Normal`, `Minimized`, `Maximized` or `Fullscreen`, without the saved
  bounds the old enum carried.
- **`flui-platform`**: `OpenWindowError::LaneFull` and `OwnerGone` carry the rejected
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
- **`flui-platform`**: the second window trait `flui_platform::window::Window` with
  `WindowBuilder` and `WindowManager`, the macOS `WindowManager`, and `WindowBounds`.
- **`flui-view`**: `WidgetsBindingObserver::did_have_memory_pressure` and `did_request_app_exit`,
  `WidgetsBinding::handle_memory_pressure` and `handle_request_app_exit`, and `AppExitResponse`.
  Memory pressure is `HostNotice::MemoryPressure`; the exit question is a session end that
  subscribers hold with a `SessionEndBlock`.
