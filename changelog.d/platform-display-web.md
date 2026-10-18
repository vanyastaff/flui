### Added

- **Web display period and screens** (`flui-platform`): the Web platform estimates the refresh period
  from the page's own `requestAnimationFrame` timestamps (the median of the last 30 frame intervals,
  background intervals left out, published only when it leaves a 3% band) and reports it in the
  display snapshot, so a 120 Hz browser tab paces by its real period instead of the 60 Hz default. The
  value is an estimate of the cadence the browser presents at; it is unknown until about ten frames have
  run and is measured again after the tab becomes visible and after a pixel-ratio change.
- **Web pixel-ratio changes are observed** (`flui-platform`): page zoom, a window moved to a display with
  another scale and an OS scale change now reach the snapshot, through a
  `(resolution: Xdppx)` media query armed again after each change.
- **Web lists every screen when access is already granted** (`flui-platform`): where the browser has the
  Window Management API and its permission is granted, the snapshot lists every screen, each keyed by its
  JS screen object so two identical monitors stay two displays, and follows `screenschange` and
  `currentscreenchange`. The permission is read, never requested; without it the snapshot holds the one
  screen.
- **`browser_display_probe` example** (`flui-platform`): logs the Web display snapshot and every change to
  it, for checking a real browser by hand.

### Changed

- **The Web window names its display** (`flui-platform`): `PlatformWindow::display_id` answers the
  screen the page is on instead of `None`, so the window's pacing follows the snapshot.
