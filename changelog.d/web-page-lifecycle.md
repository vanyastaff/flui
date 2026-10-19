### Added

- **Web page lifecycle** (`flui-platform`, `flui-app`): a frozen page (`freeze`) suspends the
  window and `resume` runs it again; `pagehide` reaches the session-end hook as the session
  ending, and a page restored from the back/forward cache cancels it, so the application can save (a write started there is best effort, the browser can stop
  the page first); `fullscreenchange` republishes the window state, so a fullscreen the user left
  with Escape shows in `WindowState`.

### Fixed

- **A lost GPU on the Web rebuilt only at the next animation frame** (`flui-platform`,
  `flui-app`, `flui-engine`): `webglcontextlost` is now cancelled, which is what asks the browser
  to restore the context, and a lost context, a restored one or a lost WebGPU device wakes the
  owner at once, so a hidden tab rebuilds without waiting for a frame. Frames stop while the
  context is lost, and the rebuilt renderer paints the whole presentation again.
  `Renderer::on_device_lost` lets a host hear a lost device.
