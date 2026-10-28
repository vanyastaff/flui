### Added
- `flui_native::os::wayland` (and `flui::native::wayland` with the `native` feature): `WaylandWindowExt::wayland` narrows a window of the native Wayland host to its `WaylandToplevel` view, whose `portal_parent` is the `wayland:<handle>` identifier an XDG Desktop Portal parents its dialog to, from the window's `xdg-foreign` export.
- The native Wayland host asks the compositor for a window's frame through `xdg-decoration` (`WindowOptions::decorated` and `WindowCommand::SetDecorated`), and the snapshot's `decorated` is what the compositor answered.
- The native Wayland host draws a window's buffer at the compositor's fractional scale (`wp_fractional_scale_v1` with `wp_viewport`).
- `flui_native::testing::wayland::{outputs, fullscreen_on}`: the compositor's outputs, and a lever that moves a window to one of them.

### Changed
- On a compositor without `xdg-decoration`, a native Wayland window refuses `SetDecorated` with `NotOnThisDevice` instead of `NotYetOnThisHost`: the host draws no frame of its own.
- A native Wayland window the compositor suspends is given no frame until it is shown again; the frame asked for meanwhile is delivered then.

### Fixed
- The Linux AT-SPI bridge tells the bus when its window is activated or deactivated; it ignored the X11 window's report before, and the Wayland window now reports it too.
