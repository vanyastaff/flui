### Added

- `WindowPosition`: a window's outer top-left as a point of one display, named by its
  `DisplayId`, in that display's logical pixels from its corner. A position that is not finite
  is refused where it is built (`WindowPosition::try_new`).
- `DisplayBounds` (a display's extent and usable area in its own logical pixels, the usable area
  inside the extent, checked by `try_new` with `InvalidDisplayBounds`) and `DesktopOrigin` (where
  a display sits in its window system's desktop, which orders the displays and is never a logical
  position). `DisplayDescription::from_device` builds a display a window system lays out in device
  pixels.
- `ApplyError::DisplayGone`: a `SetPosition` that names a display the window's host no longer has
  is refused with it, and the window stays where it is.

### Changed

- A window's position names its display. `WindowState::outer`, `WindowState::normal_geometry`,
  `WindowCommand::SetPosition`, `WindowHandle::set_position`, `WindowOptions::position` and
  `ResolvedPlacement::position` are `WindowPosition`s, and the same position lands on the same
  spot of the same display whichever display the window is on: on a desktop of mixed scales a
  Win32 window no longer moves by its own scale, and the position it publishes converts back to
  its corner. The snapshot's position is measured on the display the window is on
  (`WindowState::display`, which follows it); a window spanning displays has coordinates outside
  that display's extent. Wayland, Android, iOS and the web still publish no position.
- `DisplayDescription` carries `origin: DesktopOrigin` and `bounds: DisplayBounds` in place of the
  `bounds` and `usable_bounds` device-pixel rectangles, whose unit changed with the platform
  (logical on Wayland, points times the screen's scale on macOS).
- `WindowCommand::SetSize`, `WindowHandle::set_size` and `WindowOptions::size` take a
  `WindowExtent`; a size with no area is still refused at admission or open. The `AppConfig::size`
  field is a `WindowExtent`, which its builder (`size(width, height)` in whole logical pixels, as
  before) fills. `WindowPlacement` holds its size as a `WindowExtent` and its scale as a
  `DevicePixelRatio`; its stored format is unchanged.
- `AppConfig::position(x, y)` takes whole logical pixels of the primary display (the field is a
  `Point<i32>`), and every window the configuration opens is placed there: the main window,
  `run_direct`'s and a secondary window. No configured position refuses a start.
- A window placement is not written from a window whose host reports positions and published
  none for it (a Win32 sample that missed a monitor plugged in since the last read): the placement
  stored before stays. A Win32 window on such a monitor has the monitors read again before its
  state is published, so it publishes its position.
- The headless host keeps a window's position on its display: `simulate_move` moves it within its
  display, maximize fills the display's usable area and fullscreen the whole display.
- Native recordings write a position as `{display, at}` and a display as `origin`, `extent` and
  `usable`; the agent protocol is 0.25, which does not serve 0.24 clients.

### Removed

- `WindowOptions::display` and its builder: a position names its display.
- `WindowOption::Position`: a position is a place by its type.
- `DisplayDescription::with_usable_bounds`.
- `From<&AppConfig> for WindowOptions`: `AppConfig::window_options(displays)` builds a window's
  options, with the configured position placed on the primary display of `displays`.
