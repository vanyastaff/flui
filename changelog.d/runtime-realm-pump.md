### Changed

- **`flui-runtime`**: every runner now drives a frame through one method, `UiRealm::pump`
  (apply commands, begin frame, draw frame, end frame;
  [ADR-0083](/docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) §1), and a
  backgrounded wake through `UiRealm::pump_background`.
- **`flui-runtime`**: implicit animations (controllers registered through the widget layer's
  `VsyncScope`) advance by the frame's timestamp, the one instant the runner's wake sampled,
  instead of the wall clock read when the tick ran, as Flutter's tickers see the frame's
  timestamp. Controllers built on the scheduler's `Ticker` still read the wall clock.
- **`flui-app`** (web): no frame runs before the renderer exists. Post-frame callbacks and
  animations wait for the first animation frame after the renderer arrives, instead of running
  frames with nothing to draw into.
