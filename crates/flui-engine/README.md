# FLUI Engine

GPU-accelerated compositor for FLUI. Consumes a `flui_layer::Scene` and lowers
it to wgpu draw calls (Vulkan / Metal / DX12 / WebGPU).

## Entry points

```text
Scene (flui-layer)                    built by the widget tree's Canvas
    |
    v
RasterExecutor (flui-engine)          renders every slot of one GpuContext
    |                                 (instance, adapter, device, queue); a
    |                                 window's slot holds its surface
    v
Scene walk -> LayerDispatcher -> WgpuPainter
    |                                 record: batched Command IR (record/command_ir.rs)
    v
GpuReplay                             replay: Command IR -> wgpu draw calls
    v
GPU (wgpu)
```

- `RasterExecutor` over one `GpuContext` — presents every window of an
  application, each window a slot of one device (ADR-0305).
  `GpuContext::bootstrap` makes the context and the `SurfaceFactory` the owner
  makes each window's surface with; the `RasterBackend` parameter lets a
  GPU-free test double stand in for the context. `flui-app`'s runners drive
  it; an application does not.
- `HeadlessRenderer::new()` — rasterizes a scene to RGBA8 without a window.

What either draws is a `Scene`: draw calls recorded on a `flui_painting::Canvas`,
its display list in a picture layer. There is no public painter below the
scene. An application draws through a `CustomPaint`, as `examples/painting_demo`
does, and `flui-app`'s runners present its scenes; an embedder that wants a
scene's whole path reads those runners or starts from `HeadlessRenderer`.

The context and headless constructors are `async` — wgpu's adapter and device
requests are, so the library does not choose a blocking strategy on the
caller's behalf.

## Documentation

- `ARCHITECTURE.md` — wgpu API mapping, the record/replay boundary, mapping
  decisions, friction log.
- `CONTRIBUTING.md` — build, test, debug, and the invariant list for this
  crate.
- `AGENTS.md` — the crate's hard constraints.

## License

MIT OR Apache-2.0
