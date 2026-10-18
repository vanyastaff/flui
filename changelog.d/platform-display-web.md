### Added

- **Web display period and screens** (`flui-platform`): the Web platform estimates the refresh period
  from the page's own `requestAnimationFrame` timestamps and reports it in the display snapshot, so a
  120 Hz browser tab paces by its real period instead of the 60 Hz default. The estimate is the trimmed
  mean of the last 30 frame intervals, so a clock quantised to whole milliseconds still reads 120 Hz as
  8.33 ms; intervals longer than three times the window's lower quartile (background or throttled
  frames) are left out; a new value is published only after it has left a 3% band for five consecutive
  frames while the latest intervals agree with it and the value has stopped moving (within 1%), so a
  change between two refresh rates publishes once and never a rate the display did not run at. The value is an estimate of the cadence the browser
  presents at; it is unknown until 30 frames have run and is measured again after the tab becomes
  visible, after a pixel-ratio change and when the window moves to another screen.
- **Web pixel-ratio and screen changes are observed** (`flui-platform`): page zoom, a window moved to a
  display with another scale and an OS scale change reach the snapshot through a
  `(resolution: Xdppx)` media query armed again after each change (read again if the ratio moved
  meanwhile, and retried with a growing wait, and on window resize, if the browser refuses it). A move between screens of the
  same pixel ratio is seen through `change` on `window.screen` where the browser has it.
- **Web lists every screen when access is already granted** (`flui-platform`): where the browser has the
  Window Management API and its permission is granted, the snapshot lists every screen, each keyed by its
  JS screen object so two identical monitors stay two displays, and follows `screenschange`,
  `currentscreenchange` and `change` on each screen. The permission is read, never requested (a grant
  made later is noticed); without it the snapshot holds the one screen. The screen the window is on
  reports the window's pixel ratio as its scale, page zoom included, with and without screen access,
  while bounds stay the physical screen's under zoom. When screen access is granted, the window's
  display keeps its id and gains its name.
- **`browser_display_probe` example** (`flui-platform`): logs the Web display snapshot and every change to
  it, for checking a real browser by hand.

### Changed

- **The Web window names its display** (`flui-platform`): `PlatformWindow::display_id` answers the
  screen the page is on instead of `None`, so the window's pacing follows the snapshot.
