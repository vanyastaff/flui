### Added

- **Linux display source** (`flui-platform`): the outputs of a Wayland compositor (`wl_output`
  versions 2 to 4 with `zxdg_output_v1`) or an X server (RandR 1.5 monitors), with the refresh rate
  of each output's current mode and a notice when they change. The `wayland` and `x11` features
  carry the protocol clients (`wayland-client`, `wayland-protocols`, `x11rb`, all already in the
  lock through winit); `winit-backend` enables both on Linux. The protocol is chosen the way winit
  chooses it, and the source reads its own connection on a thread of its own.
- `LinuxPlatform::is_wayland` and `is_x11` answer from that choice instead of panicking.
- `DisplayKey::WaylandOutput` (the version 4 connector name) and `DisplayKey::X11Output` (the XID of
  a monitor's first RandR output).

### Changed

- **winit display snapshot** (`flui-platform`): the winit backend no longer enumerates winit's
  monitors. Linux reads the compositor directly, Windows reads `QueryDisplayConfig` as before, and
  macOS reads `NSScreen` with the screen-parameters notification. A Linux window names its display
  by its monitor's native number (`MonitorHandleExtX11::native_id`, the CRTC; or
  `MonitorHandleExtWayland::native_id`, the `wl_output` global name), so mirrored outputs stay two
  displays. A Wayland window that has no output yet is looked at again each time the event loop goes
  idle. `Platform::display_snapshot` on the winit backend now answers before `Platform::run`, and
  reports an error, not an empty list, when there is no display server to ask.
- On Wayland winit reports no event when a window moves between outputs of equal scale, so a refresh
  rate that differs between them is not followed until another event re-derives it.

### Removed

- `DisplayKey::Backend` (`flui-platform-api`): the label key of the winit display source, which
  keyed a monitor by a name a backend could reuse. The Wayland and X11 keys replace it on Linux.
