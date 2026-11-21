### Removed

- `flui_engine::WgpuPainter`, the immediate-mode painter below the scene. A frame is drawn on a
  `flui_painting::Canvas`, its display list put into a `flui_layer::Scene` as a picture layer, and
  the scene presented through a `RasterExecutor` over a `GpuContext` (or captured by
  `HeadlessRenderer`). An application draws through a `CustomPaint`, as `examples/painting_demo`
  now does, and `flui-app`'s runners present its scenes. The painter is re-exported only under
  the `testing` feature, for the engine's benches.
- `flui_engine::ExternalTextureRegistry`, `ExternalTextureEntry`, `ExternalTextureDescriptor`,
  `ExternalSampling`, `ExternalAlpha` and `ExternalColorEncoding`, reached only through the
  painter; a presentation's textures go through `flui_layer::TextureRegistry`. They are
  re-exported only under `testing`, with the painter.
- `flui_engine::GpuCapabilities` and `flui_engine::ResolvedSurfaceColor`, which no public
  operation returned.
- The module paths `flui_engine::error`, `flui_engine::raster`, `flui_engine::raster_executor` and
  `flui_engine::headless`. Their items are reached from the crate root only, as
  `flui_engine::EngineError`, `flui_engine::PresentDisposition`, `flui_engine::RasterExecutor`,
  `flui_engine::HeadlessRenderer` and so on.

### Changed

- `flui_engine::LIFECYCLE_CAPACITY` and `flui_engine::SlotDomain` (the marker of `SlotId`) are
  re-exported from the crate root, which they were reached from only through
  `flui_engine::raster_executor` before.
