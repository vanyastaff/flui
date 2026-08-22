# AGENTS.md — flui-engine

GPU rendering engine via wgpu. Converts Layer trees into GPU draw calls.

## What lives here

- **SceneRenderer** — top-level renderer that walks LayerTree and dispatches to layer renderers
- **CommandRenderer trait** — abstract interface for rendering draw commands
- **WgpuPainter** — concrete GPU painter implementing CommandRenderer via wgpu
- **Backend** — wgpu device/queue management, surface handling
- **TextRenderer** — glyphon-based text rendering
- **TexturePool / TextureCache** — GPU resource management
- **Layer rendering** — `wgpu/layer_render.rs` dispatches per-layer-type rendering

## Key constraints

- **Per-platform wgpu features** — target-scoped deps in Cargo.toml: Windows→dx12, macOS/iOS→metal, Linux/Android→vulkan, wasm32→webgpu+gles. Without these, `Renderer::select_backend()` finds no adapters.
- **`wgpu-backend` feature** (default) — gates all wgpu + glyphon deps. Named features: `vulkan`, `metal`, `dx12`, `webgpu`, `gles` for explicit API selection.
- **`images` feature** (default) — gates `dep:image` for texture loading.
- **`assets` feature** — gates `dep:flui-assets` for asset pipeline integration.
- **`enable-wgpu-tests` feature** — gates GPU-dependent integration tests (not run in CI).
- **`#![allow(missing_debug_implementations)]`** — wgpu handles (Device, Queue, Texture, Buffer) don't impl Debug.
- **Outstanding refactors** (tracked in ARCHITECTURE.md): the headline list is fully landed —
  the `Arc<Mutex<OffscreenRenderer>>` removal (`Renderer` owns its `OffscreenRenderer`,
  `Backend<'frame>` borrows one), the painter take/reassign cleanup (`render_scene_content`
  borrows in place), the per-frame `Arc::clone` entry (resolved when `RenderContext` lost its
  device/queue fields), and the `Arc<Mutex<TexturePoolInner>>` removal (`TexturePool` owns its
  inventory directly, `Send`-only, with an mpsc return channel for drop — see ARCHITECTURE.md
  for the deliberate divergence from the old explicit-release prescription). Port-check
  trigger #7 now watches `texture_pool.rs` with no exclusions; keep it that way — stale
  whitelist globs are how `renderer.rs`/`backend.rs` once went unwatched.
- **No `async fn` in render hot paths** — enforced by port-check trigger #3. `new`/`new_offscreen` are async (setup-phase, acceptable).

## Architecture doc

- `crates/flui-engine/ARCHITECTURE.md` — Flutter source mapping, outstanding refactors, friction log
