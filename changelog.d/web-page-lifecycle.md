### Added

- **Web page lifecycle** (`flui-native`, `flui-app`): a frozen page (`freeze`) suspends the
  window and `resume` runs it again; `pagehide` reaches the session-end hook as the session
  ending, and a page restored from the back/forward cache cancels it, so the application can
  save (a write started there is best effort, the browser can stop the page first);
  `fullscreenchange` republishes the window state, so a fullscreen the user left with Escape
  shows in `WindowState`, and a fullscreen element that holds the canvas counts as the window
  being fullscreen.
- **`Renderer::on_device_lost`** (`flui-engine`): a host hears a lost graphics device when it
  happens instead of at its next frame.

### Fixed

- **A lost GPU on the Web rebuilt only at the next animation frame** (`flui-native`,
  `flui-app`, `flui-engine`): `webglcontextlost` is now cancelled, which is what asks the browser
  to restore the context, and a lost context, a restored one or a lost WebGPU device wakes the
  owner at once, so a hidden tab rebuilds without waiting for a frame. Frames stop while the
  context is lost, and the rebuilt renderer paints the whole presentation again. A renderer
  whose first build failed is built again at the next wake.
