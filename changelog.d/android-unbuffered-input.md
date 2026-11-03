### Added

- Android: a window opened with `InputLatency::Low` (`AppConfig::with_input_latency`) receives
  touch, stylus and mouse samples as soon as the system has them instead of once per frame
  (`View.requestUnbufferedDispatch`, API 30 and later).
