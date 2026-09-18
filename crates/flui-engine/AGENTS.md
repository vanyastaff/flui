# AGENTS.md — flui-engine

GPU rendering engine via wgpu. Converts Layer trees into GPU draw calls.

## What lives here

- **Renderer** — owns one window's GPU stack and drives the layer walk (the embedder entry point)
- **WgpuPainter / Backend / LayerRender** — the per-frame painter and its layer-type dispatch (crate-internal)
- **CommandRenderer trait** — the command dispatch surface (`crate::traits`, crate-internal)
- **GpuReplay / CommandIR** — the record/replay split: batched IR, then wgpu encoding
- **LayerDispatcher** — the per-frame command route from the layer walk to `WgpuPainter` (crate-internal)
- **TextRenderer** — glyphon-based text rendering
- **TexturePool / TextureCache** — GPU resource management

## Key constraints

- **Per-platform wgpu features** — target-scoped deps in Cargo.toml: Windows→dx12, macOS/iOS→metal, Linux/Android→vulkan, wasm32→webgpu+gles. Without these, `Renderer::select_backend()` finds no adapters.
- **`wgpu-backend` feature** (default) — gates all wgpu + glyphon deps. Named features: `vulkan`, `metal`, `dx12`, `webgpu`, `gles` for explicit API selection.
- **`enable-wgpu-tests` feature** — gates GPU-dependent integration tests (not run in CI).
- **`#![expect(missing_debug_implementations)]`** — wgpu handles (Device, Queue, Texture, Buffer) don't impl Debug.
- **Outstanding refactors** (tracked in ARCHITECTURE.md): the headline list is fully landed —
  the `Arc<Mutex<OffscreenRenderer>>` removal (`Renderer` owns its `OffscreenRenderer`,
  `Backend<'frame>` borrows one), the painter take/reassign cleanup (`render_scene_content`
  borrows in place), the per-frame `Arc::clone` entry (resolved when `RenderContext` lost its
  device/queue fields), and the `Arc<Mutex<TexturePoolInner>>` removal (`TexturePool` owns its
  inventory directly, `Send`-only, with an mpsc return channel for drop — see ARCHITECTURE.md
  for the deliberate divergence from the old explicit-release prescription). Port-check
  trigger #7 now watches `texture_pool.rs` with no exclusions; keep it that way — stale
  whitelist globs are how `renderer.rs`/`layer_dispatcher.rs` once went unwatched.
- **No `async fn` in render hot paths** — enforced by port-check trigger #3. `new`/`new_offscreen` are async (setup-phase, acceptable).
- **One GPU stack per renderer, today.** ADR-0045 decision 2's shared-per-owner-thread stack (and its windowed half) is deferred until `ReplaceServices` — the owner-thread re-pointing step device recovery needs once several renderers share a device — exists; the offscreen-only `GpuServices` value type was deleted rather than carried unwired. `Renderer::new` is the advertised entry point.

## Architecture doc

- `crates/flui-engine/ARCHITECTURE.md` — Flutter source mapping, outstanding refactors, friction log
