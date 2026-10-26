### Added

- `InputLatency` (`Balanced`, `Low`) and `WindowOptions::with_input_latency`. A `Low` window on the
  web listens to `pointerrawupdate` where the browser has it, so a stroke gets every sample instead
  of one per frame. Other backends ignore it.
