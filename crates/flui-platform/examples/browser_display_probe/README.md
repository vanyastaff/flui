# Browser display probe

Logs the Web platform's display snapshot and every change to it, to the page and
the browser console.

Build, generate the page and serve it from the repository root:

```sh
cargo build --locked -p flui-platform --example browser_display_probe --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir target/browser-display-probe \
  target/wasm32-unknown-unknown/debug/examples/browser_display_probe.wasm
cp crates/flui-platform/examples/browser_display_probe/index.html target/browser-display-probe/
trunk serve target/browser-display-probe/index.html --dist target/browser-display-probe-dist \
  --port 8765 --no-autoreload --offline
```

`wasm-bindgen` must be the version in `Cargo.lock`. `trunk serve` runs until it
is interrupted; it only copies the generated files (the page's `data-trunk`
links) and serves them, it does not build the example. Any static server over
`target/browser-display-probe` serves the page as well. Open
`http://localhost:8765/` in Chrome (a secure context is required for Window
Management, and `localhost` is one).

What to check, each as a new log line:

- **Refresh period.** The first snapshot has `period=unknown`; about thirty
  frames later a line shows the estimate (16.667 ms on a 60 Hz display). The
  value is an estimate of the cadence the browser presents at.
- **Page zoom.** Ctrl-plus and Ctrl-minus change `scale`; each step is one line,
  and the period is measured again afterwards. The scale is the window's pixel
  ratio, zoom included, with and without screen access.
- **Another monitor.** Drag the window to a display with another scale, size or
  refresh rate; `scale`, `bounds` and `period` follow (Chromium raises `change`
  on `window.screen` for this without any permission).
- **Screens.** Without access the snapshot lists one display. "Grant screen
  access" calls `getScreenDetails()` from the page, which prompts; once granted
  the probe lists every screen (a permission granted from the site settings is
  picked up too). Dragging the window between screens moves `window_display`,
  and the period is measured on the screen the window is on.
