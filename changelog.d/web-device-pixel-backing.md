### Fixed

- **Web canvas backing store is the laid-out device size** (`flui-platform`): where the browser
  observes the `device-pixel-content-box` (Chrome, Firefox), the canvas's backing store and
  `physical_size` come from the `ResizeObserver` entry's `devicePixelContentBoxSize`, the device
  pixels the browser snapped the box to, instead of `round(CSS size × devicePixelRatio)`, which
  can be a pixel off and blur or stretch the surface. That observer also reports a pixel-ratio
  change, so such a window no longer arms a pixel-ratio media query of its own. Safari keeps the
  estimate and the query.
