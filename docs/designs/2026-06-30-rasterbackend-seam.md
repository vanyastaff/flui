---
title: "RasterBackend seam — pluggable rendering backends"
status: implemented
date: 2026-06-30
roadmap: Core.0 N10
related: 2026-06-30-scene-drawcommand-contract.md
guards:
  - "scripts/port-check.sh trigger #21 — lyon confined to wgpu/tessellator.rs"
---

[← Designs index](.) · [Roadmap](../ROADMAP.md) · [Tracker (N10)](../ROADMAP-TRACKER.md) · [Scene/DrawCommand contract](2026-06-30-scene-drawcommand-contract.md)

# RasterBackend seam (Core.0 N10)

> **Goal.** Make swapping the rendering backend — lyon+wgpu today, Vello or a
> software rasterizer tomorrow — a non-breaking change. The lyon tessellator
> stays the default implementation; the seam ensures no consumer is coupled to
> it.

---

## The mistake this design avoids

The naïve reading of "RasterBackend seam" is: wrap the lyon `Tessellator`
([`wgpu/tessellator.rs`](../../crates/flui-engine/src/wgpu/tessellator.rs)) in a
trait like `trait RasterBackend { fn tessellate_fill(..) -> (Vec<Vertex>, Vec<u32>); }`.

**That is a fake seam.** Its input/output (FLUI path/shape → triangle-mesh
`(vertices, indices)`) hard-codes a *CPU-tessellation-to-triangles* strategy.
[Vello](https://github.com/linebender/vello) — the realistic future backend — is
a **compute rasterizer**: it encodes a scene and rasterizes it directly on the
GPU; it never produces triangle meshes. A trait shaped around lyon's vertex
buffers could never host it. Wrapping lyon would satisfy the words of the gate
while delivering none of its intent — exactly the "MVP-reported-as-parity"
failure mode the repo's Definition-of-Done forbids.

The swap seam must sit **above** tessellation, at the boundary where a backend
is handed *what to draw* and asked to *produce pixels*.

---

## The seam (two layers)

The engine already layers correctly; this work names, hardens, and documents the
two real seam points and removes the one violation of them.

```
Scene (flui-layer)                     ← frozen data contract (see companion doc)
  │
  ▼
RasterBackend  (driver seam)           ← NEW: render a Scene to a target
  │  default impl: wgpu::Renderer
  ▼
CommandRenderer + LayerStateStack      ← per-DrawCommand visitor seam (pre-existing)
  │  impls: WgpuPainter, DebugBackend, <future Vello/software>
  ▼
Backend internals (lyon, wgpu pipelines)   ← implementation detail, contained
```

### Layer 1 — `CommandRenderer` / `LayerStateStack` (per-command visitor)

Defined in [`traits.rs`](../../crates/flui-engine/src/traits.rs). One `render_*`
method per `DrawCommand` variant + the clip/transform/effect stack. This is the
*existing* backend-agnostic seam — its own docs already name "wgpu, skia, vello,
software" as intended implementors. A new backend implements these traits and
consumes the same `DrawCommand` stream (frozen by the
[companion contract](2026-06-30-scene-drawcommand-contract.md)). lyon is an
internal detail of the wgpu implementation, invisible at this trait.

### Layer 2 — `RasterBackend` (driver seam) — NEW

Defined in [`raster.rs`](../../crates/flui-engine/src/raster.rs). The
**frame-driver** contract that `flui-app` depends on — "own per-window GPU
state; given a `&Scene`, present a frame." The wgpu `Renderer` is the default
implementation; a Vello/software `Renderer` would be an alternative impl selected
at construction. The trait surface is exactly the per-frame methods the app loop
uses:

```rust
pub trait RasterBackend {
    fn render_scene(&mut self, scene: &flui_layer::Scene) -> Result<(), EngineError>;
    fn resize(&mut self, width: u32, height: u32);
    fn is_device_lost(&self) -> bool;
    fn mark_dirty(&mut self, rect: Rect<Pixels>);
    fn mark_full_repaint(&mut self);
    fn has_damage(&self) -> bool;
    fn size(&self) -> (u32, u32);
    fn reconfigure_surface(&mut self) -> Result<(), EngineError>;
}
```

Construction is deliberately **excluded** from the trait: it is backend- and
window-specific and asynchronous (`Renderer::new(window).await`). That single
constructor line is the backend selection point (a factory) — swapping backends
changes that one line, while the whole per-frame app loop is written against
`RasterBackend`. The trait is dyn-compatible (no generics/async in methods), so
both `<R: RasterBackend>` (zero-cost) and `Box<dyn RasterBackend>` (runtime
selection) work.

---

## What this work changed

1. **Fixed an abstract→concrete layering inversion.** `CommandRenderer::superellipse_path`'s
   default impl called `crate::wgpu::layer_render::generate_superellipse_path` —
   the *backend-agnostic* trait depending on the *concrete wgpu* module. The
   function is pure geometry (`RSuperellipse → Path`, only `flui_types`), so it
   moved to a backend-agnostic module
   [`superellipse.rs`](../../crates/flui-engine/src/superellipse.rs). The
   abstract layer no longer reaches into wgpu.
2. **Introduced the `RasterBackend` driver trait** (`raster.rs`), implemented for
   `wgpu::Renderer`, re-exported from the crate root, and adopted at the
   `flui-app` frame boundary so the swap is real, not theoretical.
3. **Feature-gated lyon** as an optional dependency behind `wgpu-backend`, so a
   non-wgpu build does not pull a tessellation library it doesn't use.
4. **Installed a containment guard** (`port-check.sh` trigger #21): `lyon` may
   appear only in `wgpu/tessellator.rs`. This turns *incidental* containment
   (lyon happens to be used in one file) into a *contractual* one — the next
   contributor who reaches for lyon elsewhere is stopped, keeping the seam swap
   non-breaking.

## How to add a backend (e.g. Vello)

1. Implement `CommandRenderer` + `LayerStateStack` for a `VelloPainter`
   (translate each `DrawCommand` into Vello scene-encoding calls; `save_layer`/
   clip-stack map to Vello's layer/clip primitives).
2. Implement `RasterBackend` for a `VelloRenderer` owning the Vello render
   context + surface.
3. Select it at the one construction site (`VelloRenderer::new(window)` instead
   of `wgpu::Renderer::new`). The app loop is unchanged.

No `DrawCommand` change is required — the data contract is frozen and shared.

---

## Honest remaining gaps

- **Backdrop-filter fast path.** `renderer.rs` routes the whole-scene
  `BackdropFilter` effect through a wgpu-specific compositor path rather than the
  `CommandRenderer` trait method. A Vello backend would need its own handling
  here; this is a known wgpu-specific shortcut, not yet abstracted. Tracked for
  the eventual second-backend work, not papered over.
- **Construction/factory.** Backend selection is a single concrete constructor
  line, not a runtime registry. A registry/factory is unnecessary speculative
  scaffolding until a second backend exists (Cross.H H7 cautions against exactly
  that); the trait makes adding one non-breaking when warranted.
