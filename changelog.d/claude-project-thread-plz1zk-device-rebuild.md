### Added

- The raster executor rebuilds a lost GPU device on the context's own instance, keeping every
  slot's surface: rebuilds are paced on the executor's clock and given up after ten failures at the
  one-second ceiling, `RasterCommand::RetryDevice` asks again, and `RasterSender::device_health`,
  `device_retry_at` and `take_device_failure` tell the owner where the device stands.

### Changed

- `RasterExecutor::new` takes the clock the device's rebuilds are paced on, and `SlotBackend` gains
  `device_lost` and `rebuild_device`.
