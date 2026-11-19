### Changed

- A lost GPU device's replacement is asked for on a worker: the turn that starts the request no
  longer waits for the driver, and the turn after its answer installs the device. A request
  carries a `DeviceTicket` and wakes the executor through a `DeviceAnswered`, and
  `RasterExecutor::new` takes a fourth argument, the host's wake from any thread.
