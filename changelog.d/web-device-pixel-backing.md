### Fixed

- **Web canvas backing store is the laid-out device size** (`flui-platform`, `flui-app`): where
  the browser observes the `device-pixel-content-box` (Chrome, Firefox), the canvas's backing
  store and `physical_size` come from the `ResizeObserver` entry's `devicePixelContentBoxSize`,
  the device pixels the browser snapped the box to, instead of
  `round(CSS size × devicePixelRatio)`, which can be a pixel off and blur or stretch the surface.
  The Web runner configures the renderer from `physical_size`, so the surface follows. Safari,
  and a canvas with any padding (whose window is its padding box), keep the estimate.
