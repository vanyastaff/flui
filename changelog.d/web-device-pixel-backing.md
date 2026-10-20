### Fixed

- **Web canvas backing store is the laid-out device size** (`flui-platform`, `flui-app`): where
  the browser observes the `device-pixel-content-box` (Chrome, Firefox), the canvas's backing
  store and `physical_size` come from the `ResizeObserver` entry's `devicePixelContentBoxSize`,
  the device pixels the browser snapped the box to, instead of
  `round(CSS size × devicePixelRatio)`, which can be a pixel off and blur or stretch the surface.
  Safari, and a canvas with any padding (whose window is its padding box), keep the estimate.
  The Web runner and `run_direct` (every platform) configure the renderer from the window's
  `physical_size` instead of `size × scale`, so the surface follows.
- **Web window follows its canvas's padding** (`flui-platform`): the window's size is the
  canvas's padding box, which no observed box measured, so padding added around a `content-box`
  canvas left the window, its backing store and the renderer at the old size. A border-box
  observer reports it, and beside the device-pixel box a content-box observer reports a reflow
  that leaves the rounded device-pixel box unchanged.
