### Added
- The native Wayland host asks the compositor for a window's frame through `xdg-decoration` (`WindowOptions::decorated` and `WindowCommand::SetDecorated`), and the snapshot's `decorated` is what the compositor answered.
- The native Wayland host draws a window's buffer at the compositor's fractional scale (`wp_fractional_scale_v1` with `wp_viewport`).
- The native Wayland host exports each window through `xdg-foreign` v2 where the compositor offers it, so a portal can parent its dialog to the window.
- `flui_native::testing::wayland::{outputs, fullscreen_on, portal_parent, bridge_focus, suspend, OutputName}`: the compositor's outputs and a lever that moves a window to one of them, the window's portal parent, what it last told its accessibility bridge, and a `suspended` configure put to the window; `CompositorReport` carries the compositor's fractional scale and frame mode, and names outputs by `OutputName`.

### Changed
- On a compositor without `xdg-decoration`, a native Wayland window refuses `SetDecorated` with `NotOnThisDevice` instead of `NotYetOnThisHost`: the host draws no frame of its own. A compositor that answered the last request with the other mode (sway, for a tiled window) decides the frame, and the command is refused the same way.
- A native Wayland window the compositor tiles (and has not maximized or made fullscreen) refuses `SetSize` with `NotOnThisDevice`: the tiling holds its size.
- A native Wayland window the compositor suspends is given no frame until it is shown again; the frame asked for meanwhile is delivered then.

### Fixed
- The Linux AT-SPI bridge tells the bus when its window is activated or deactivated; it ignored the X11 window's report before, and the Wayland window now reports it too.
