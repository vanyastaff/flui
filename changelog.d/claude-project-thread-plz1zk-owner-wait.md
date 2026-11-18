### Added

- `flui_platform::OwnerWait`, the platform's wait for the windows' thread while it waits on a raster thread, and `OwnerPlatform::owner_wait`. On Win32 it serves the messages a raster thread's DXGI calls send to the windows meanwhile (ADR-0305 §9).

### Changed

- `RasterExecutor::spawn` takes the platform's `OwnerWait`. `ExecutorHandle::detach`, `ExecutorHandle::await_attach` and the stop wait through it where the platform has one, and the raster thread notifies it after each turn and once it stopped.
