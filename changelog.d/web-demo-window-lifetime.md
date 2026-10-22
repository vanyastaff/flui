### Fixed

- **`web_demo`**: the demo's window lives as long as the page. It was dropped as soon as the web
  platform's `run` returned, which retired the canvas, so the demo reported no input or resize
  after start-up. Its page now gives the canvas a CSS size: without one, the canvas's box is the
  backing store FLUI writes, and on a display with a pixel ratio above 1 it grew at every report.
