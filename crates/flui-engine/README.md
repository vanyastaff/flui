# FLUI Engine

GPU-accelerated compositor for FLUI. Consumes a `flui_layer::Scene` and lowers
it to wgpu draw calls (Vulkan / Metal / DX12 / WebGPU).

## Entry points

```text
Scene (flui-layer)                    built by the widget tree's Canvas
    |
    v
Renderer (flui-engine)                owns one window's GPU stack (instance,
    |                                 adapter, device, queue, surface)
    v
LayerTree walk -> LayerDispatcher -> WgpuPainter
    |                                 record: batched Command IR (command_ir.rs)
    v
GpuReplay                             replay: Command IR -> wgpu draw calls
    v
GPU (wgpu)
```

All three constructors are `async` — wgpu's adapter and device requests are,
so the library does not choose a blocking strategy on the caller's behalf:

- `Renderer::new(window)` — windowed; owns its surface, recovers from device loss.
- `HeadlessRenderer::new()` — rasterizes a `LayerTree` to RGBA8 without a
  window.

- `RasterExecutor` — presents every window of an application over one
  `GpuContext`; its `RasterBackend` parameter lets a GPU-free test double
  stand in for the context.
- `WgpuPainter` — the per-frame painter, for embedders driving draw calls
  directly (see `examples/painting_demo`).

A single GPU stack per renderer is the current model. ADR-0045 decision 2's
shared-per-owner-thread stack arrives with its `ReplaceServices` recovery
mechanism; see `docs/adr/ADR-0045-raster-lane.md`.

## Documentation

- `ARCHITECTURE.md` — wgpu API mapping, the record/replay boundary, mapping
  decisions, friction log.
- `CONTRIBUTING.md` — build, test, debug, and the invariant list for this
  crate.
- `AGENTS.md` — the crate's hard constraints.

## License

MIT OR Apache-2.0
