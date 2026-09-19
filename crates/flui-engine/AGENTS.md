# AGENTS.md — flui-engine

GPU rendering engine via wgpu. Converts Layer trees into GPU draw calls.

## What lives here

- **Renderer** — owns one window's GPU stack and drives the layer walk (the embedder entry point)
- **WgpuPainter / LayerDispatcher / LayerRender** — the per-frame painter and its layer-type dispatch (crate-internal)
- **CommandRenderer trait** — the command dispatch surface (`crate::traits`, crate-internal)
- **GpuReplay / CommandIR** — the record/replay split: batched IR, then wgpu encoding
- **LayerDispatcher** — the per-frame command route from the layer walk to `WgpuPainter` (crate-internal)
- **GlyphAtlas** — the rasterised-glyph cache; text is a glyph batch of the segment (ADR-0067)
- **TexturePool / TextureCache** — GPU resource management

## Key constraints

- **Per-platform wgpu features** — target-scoped deps in Cargo.toml: Windows→dx12, macOS/iOS→metal, Linux/Android→vulkan, wasm32→webgpu+gles. Without these, `Renderer::select_backend()` finds no adapters.
- **wgpu is the engine, unconditionally** — there is no backend feature; `vulkan`, `metal`, `dx12`, `webgpu`, `gles` are additive pass-throughs to wgpu's own API features (the per-target dependency entries already pick the right one).
- **`testing` feature** — gates the ~440 GPU readback tests; CI's `gpu-test` job runs them on WARP with `FLUI_REQUIRE_GPU=1`, so an unavailable adapter there fails instead of skipping.
- **`#![expect(missing_debug_implementations)]`** is crate-wide; see `src/lib.rs` for which types it covers (wgpu 30's handles all derive `Debug`, so the reason is no longer "wgpu handles").
- **No `Arc<Mutex<_>>` on any engine subsystem** — `Renderer` owns its `OffscreenRenderer`
  and `WgpuPainter` outright and `LayerDispatcher<'frame>` borrows them disjointly;
  `TexturePool` owns its inventory with an mpsc return channel. Port-check trigger #7 watches
  the whole crate with no exclusions; keep it that way — stale whitelist globs are how files
  once went unwatched. Open items live under `ARCHITECTURE.md` → "Open items".
- **No `async fn` in render hot paths** — enforced by port-check trigger #3. `Renderer::new`/`HeadlessRenderer::new` are async (setup-phase, acceptable).
- **One GPU stack per renderer, today.** ADR-0045 decision 2's shared-per-owner-thread stack (and its windowed half) is deferred until `ReplaceServices` — the owner-thread re-pointing step device recovery needs once several renderers share a device — exists; the offscreen-only `GpuServices` value type was deleted rather than carried unwired. `Renderer::new` is the advertised entry point.

## Architecture doc

- `crates/flui-engine/ARCHITECTURE.md` — Flutter source mapping, outstanding refactors, friction log
