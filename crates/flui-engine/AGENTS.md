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
- **Outstanding refactors** (tracked in ARCHITECTURE.md):
  - `Arc<Mutex<TexturePoolInner>>` → direct ownership — the one still open
  - Landed: the `Arc<Mutex<OffscreenRenderer>>` removal (`Renderer` owns its `OffscreenRenderer`, `Backend<'frame>` borrows one), the painter take/reassign cleanup (`render_scene_content` borrows in place), and the per-frame `Arc::clone` entry (resolved when `RenderContext` lost its device/queue fields)
  - `texture_pool.rs` is whitelisted in port-check.sh trigger #7 — **remove the whitelist entry in the same change as the refactor**. Trigger 7's exclusions for `renderer.rs`/`backend.rs` were not removed when that refactor landed, and both files went unwatched until #635 caught it.
- **No `async fn` in render hot paths** — enforced by port-check trigger #3. `new`/`new_offscreen` are async (setup-phase, acceptable).

## Architecture doc

- `crates/flui-engine/ARCHITECTURE.md` — Flutter source mapping, outstanding refactors, friction log
