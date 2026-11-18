### Changed

- The application's retry of a GPU device whose rebuilds were given up now runs the backend probe again in fresh instances on the owner thread, and the raster executor adopts the device it answers instead of rebuilding on the instance whose backend failed. `RasterCommand::RetryDevice` carries that device (`RetryDevice { device: ProbedDevice }`), `GpuContext::probe` returns it with the factory the windows' new surfaces are made with, and `RasterBackend::adopt_device` is the new backend step. How long the probe blocked is logged as `gpu_probe` on the `flui.gpu` target.
