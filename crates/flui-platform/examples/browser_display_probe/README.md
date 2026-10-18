# Browser display probe

Logs the Web platform's display snapshot and every change to it, to the page and
the browser console.

Build, generate the page and serve it from the repository root:

```sh
cargo build --locked -p flui-platform --example browser_display_probe --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir target/browser-display-probe \
  target/wasm32-unknown-unknown/debug/examples/browser_display_probe.wasm
cp crates/flui-platform/examples/browser_display_probe/index.html target/browser-display-probe/
python -m http.server 8765 --directory target/browser-display-probe
```

`wasm-bindgen` must be the version in `Cargo.lock`. Open
`http://localhost:8765/` in Chrome (a secure context is required for Window
Management, and `localhost` is one).

What to check, each as a new log line:

- **Refresh period.** The first snapshot has `period=unknown`; about ten frames
  later a line shows the estimate (16.667 ms on a 60 Hz display). The value is an
  estimate of the cadence the browser presents at.
- **Page zoom.** Ctrl-plus and Ctrl-minus change `scale`; each step is one line,
  and the period is measured again afterwards.
- **Another monitor.** Drag the window to a display with another scale or
  refresh rate; `scale` and `period` follow.
- **Screens.** Without access the snapshot lists one display. "Grant screen
  access" calls `getScreenDetails()` from the page, which prompts; once granted
  the probe lists every screen (the permission is also picked up if it is
  granted from the site settings). Dragging the window between screens moves
  `window_display`, and the period is measured on the screen the window is on.
