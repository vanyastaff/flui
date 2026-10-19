### Added

- **`flui-platform`** (Linux, winit backend): a window takes every `WindowCommand` its shell
  honours through `PlatformWindow::apply` (`set_title`, `set_outer_position`,
  `request_inner_size`, `set_min_inner_size`/`set_max_inner_size`, `set_resizable`,
  `set_decorations`, minimize, maximize, borderless fullscreen, show, hide, focus) and publishes a
  `WindowState` that follows what the window system reports. X11 windows have every capability;
  Wayland windows lack `sets_position`, `hides`, `shows`, `activates`, `observes_position`,
  `observes_minimized` and `places_at_open`, and `Platform::window_capabilities` says so before
  a window exists.
- **`flui-platform`** (Linux, X11): the snapshot reads the mode from `WM_STATE` and
  `_NET_WM_STATE` through a connection of its own, so a minimize, which winit reports no event for,
  is published, with `on_state_changed`. An X11 window opens in its `WindowOptions` position and
  mode (a maximized or fullscreen one with the rectangle it returns to), created unmapped and
  mapped by `reveal_after_first_frame`, so the first thing it shows is a frame. A Wayland window
  has no buffer, and is not shown, until its first frame. `Platform::window_frame_inset` returns the
  frame measured from a decorated window.
- **`flui-platform`** (Linux): sleep, wake, session lock and unlock, shutdown and memory pressure
  reach `Platform::on_host_notice` and `on_session_end`. logind's `PrepareForSleep` and
  `PrepareForShutdown` are held by a delay inhibitor until the owner has delivered the notice (at
  most four seconds), then released; `Lock`, `Unlock` and `LockedHint` of the process's session give
  the lock notices. A shutdown is a notification only: Linux cannot ask logind to wait for the
  user, so the answer to the query is not read. Memory pressure comes from kernel pressure
  triggers on `/proc/pressure/memory` (`some` is `Moderate`, `full` is `Critical`), with a two
  second window for a process without `CAP_SYS_RESOURCE`. A source that cannot start (no system
  bus, no pressure information) is logged and leaves the others running. A wake marks the
  displays and every window's state stale.
- **`flui-platform`** (Linux): `DisplayInfo::persistent_key` is the RandR connector and the
  panel's EDID identity on X11 (`HDMI-1|DEL4115-0000002A`) and the connector name on Wayland.
