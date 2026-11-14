### Changed

- The web draws its canvas through a slot of the application's raster executor, as every native
  window does, and applies each frame's damage instead of drawing every frame in full. A lost
  WebGPU device or WebGL context retires the page's GPU context with its slot, and a new one is
  awaited and attached before the next frame runs.
- `flui_engine::SlotBackend` is renamed `RasterBackend`: the executor is
  `RasterExecutor<B: RasterBackend = GpuContext>`.

### Removed

- The per-window `flui_engine::RasterBackend` trait that `Renderer` implemented, with
  `PrePresentHook` and `Renderer::set_pre_present_hook`: a slot calls its surface target's
  `pre_present_notify` itself. `PresentDisposition` stays.
