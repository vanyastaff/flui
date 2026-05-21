---
title: "Mythos design verdict — flui-painting redesign"
status: design
date: 2026-05-20
author: Claude Mythos
applies-to: crates/flui-painting
---

# Mythos Design Verdict

## What `flui-painting` should be

A **single-owner Canvas recorder + a closed-enum `DrawCommand` value type + a consumed-once `DisplayList`**. Three concerns, one crate. Records paint operations from `flui-rendering`'s paint phase into a `DisplayList` that `flui-engine` matches for GPU lowering and `flui-layer` wraps in `Layer::Picture` / `Layer::Canvas`. Nothing else.

## What it must not become

A `WarmUpCanvas` trait carrier whose only purpose is to be a parameter type for a `ShaderWarmUp::warm_up_on_canvas(&self, &mut dyn WarmUpCanvas)` method whose `execute()` body literally says *"in a real implementation, we'd create an offscreen canvas here"*. A 3,305-LOC `canvas.rs` that mixes the state machine + transform stack + clip stack + 30 drawing primitives + scoped operations + multi-canvas composition + finalisation + query helpers. A `DrawCommand` enum that costs every `draw_*` caller a `paint.clone()` per command even when the same `Paint` is reused for thousands of calls. A `restore()` that silently no-ops AND a `finish()` that silently `tracing::warn`s about unrestored saves, so neither path catches the programmer error during testing. A companion `docs/MIGRATION.md` that documents migration "from 0.0.x to 0.1.x" of a crate that never had a 0.0.x release.

## Main state owner

`Canvas` owns its inner `DisplayList` during recording. `Canvas::finish(self) -> DisplayList` consumes the canvas and surrenders the immutable `DisplayList`. The `DisplayList` is then consumed exactly once: either moved into `Layer::Picture(PictureLayer)` / `Layer::Canvas(CanvasLayer)` by `flui-layer`, or iterated by `flui-engine`'s wgpu backend during GPU lowering. There is no `Arc<RwLock<Canvas>>` anywhere. The `Arc<DisplayList>` blanket impl in the sealed-trait pair (`DisplayListCore for Arc<DisplayList>`) is for retained-layer caching, not concurrent mutation -- the value is frozen by the time the `Arc` is constructed.

Two infrastructure lock sites stay (image cache + system fonts notifier; off the per-command hot path; documented in [`docs/PORT.md`](../PORT.md) lock-decision table). One feature-gated `OnceLock<Mutex<FontSystem>>` for cosmic-text font shaping stays (setup-phase + text-shape phase, not per-command). No new locks introduced.

## Main trust boundary

**`DrawCommand` is a closed concrete enum, not a `Box<dyn Drawable>` plugin trait.** The 29 variants are the entire vocabulary of paint operations the crate emits; adding a 30th is a coordinated change in `flui-painting` + `flui-engine` (whose wgpu backend pattern-matches every variant to GPU draw calls) + `flui-rendering` (whose `RenderObject::paint` impls choose which variants to emit). This is **deliberately the same shape** as `flui-layer`'s `Layer` enum (see [`docs/designs/2026-05-20-mythos-flui-layer-redesign.md`](2026-05-20-mythos-flui-layer-redesign.md) Mapping decisions #1). The reason is identical: arbitrary trait-object commands would force a `Box<dyn DrawableLower>` boundary the wgpu backend cannot translate.

The trust boundary against third-party code is `Paint`, `Path`, `Shader`, `Image`, `ImageFilter` -- all configuration types from `flui-types::painting`. Validation lives at construction time in `flui-types`. `flui-painting` accepts validated values, bakes the active transform matrix at recording time, and ships the result as an immutable `DisplayList`.

`ClipContext` is a one-prod-impl trait (`CanvasContext` in `flui-rendering`) that survives as a deliberate cross-crate seam. The three default `clip_*_and_paint` methods on the trait save real boilerplate at the caller site. The trait stays; the seal is documented.

## Main async risk

Zero. There is no `async fn` in any `Canvas::draw_*`, no `.await` in display list mutation, no `.await` in scene-canvas finalisation. `cosmic-text` font shaping is synchronous behind a `Mutex` (off the per-command path; setup + first-shape phase). The `tessellation` module is synchronous. No background tasks. No channels. No futures.

The `port-check.sh` Trigger 3 scope is **extended in U13 of this chain** to cover `crates/flui-painting/src/**` for `async fn build|layout|paint|perform_layout|composite|render|fire_composition_callbacks` -- the verb set used in `flui-rendering` and `flui-layer`. Today the crate is clean and will stay clean.

## Main simplification principle

**Every dead trait, every fake plugin seam, every fluff method in this crate must justify its presence in writing.** A non-exhaustive list of items that do not justify themselves today:

- `WarmUpCanvas` trait (4 abstract methods, 0 production impls, 0 callers outside the dead `ShaderWarmUp::warm_up_on_canvas` parameter). Pure dead code. **Delete.**
- `ShaderWarmUp` trait (1 production impl `DefaultShaderWarmUp` whose `warm_up_on_canvas` body draws into a `&mut dyn WarmUpCanvas` that nobody implements; whose `execute()` body literally says "in a real implementation, we'd create an offscreen canvas here"; whose entire purpose is to bootstrap shader compilation, but the wgpu shaders are compiled once at engine init and warm-up adds no measurable benefit). The whole shader-warm-up subsystem is decorative. **Collapse:** delete the trait, delete `DefaultShaderWarmUp`, delete the `shader_warm_up: Option<Box<dyn ShaderWarmUp>>` field on `PaintingBinding`, delete `PaintingBinding::with_shader_warm_up` constructor variant, delete `PaintingBinding::set_shader_warm_up`. File "shader warm-up backed by real offscreen canvas" in Outstanding refactors as a real future scope.
- `Canvas::restore_to_count` is barely used in production but is a clean Skia-parity method. Keep.
- `canvas.rs` is 3,305 LOC mixing: Canvas struct + CanvasState + ClipShape + transform/scale/rotate/skew methods + save/restore/save_layer + clip ops + 30 draw_* primitives + draw_path + draw_image variants + draw_text + draw_atlas + draw_shader_mask + draw_backdrop_filter + 12 `with_*` scoped operations + `extend_from`/`merge`/`append_*` composition + `reset`/`clear_commands` + `is_empty`/`len`/`bounds`/`display_list` queries + `finish`/`Drop`/`AsRef`. That's 8 distinct concerns in one file. **Split.**
- `display_list.rs` is 2,434 LOC mixing: `PointerEvent`/`PointerEventKind`/`HitRegion`/`HitRegionHandler` + sealed-trait module + `DisplayListCore` trait + `DisplayListExt` trait + 4 blanket impls (`DisplayList`, `Arc<DisplayList>`, `Box<DisplayList>`, `&DisplayList`) + `DisplayList` struct + `DisplayListStats` struct + 29-variant `DrawCommand` enum + `DrawCommand::with_opacity` (240 LOC pattern match across all variants) + `DrawCommand::bounds` (250 LOC pattern match) + `DrawCommand::transform`/`paint`/`kind`/`is_*` accessors + `DrawCommand::apply_transform` mutator + `CommandKind` enum. Same 8-concerns problem. **Split.**
- `text_layout.rs` is 1,243 LOC behind a `feature = "text"` gate (the entire file body is `#[cfg(feature = "text")] mod inner` -- which is itself an unnecessary indirection; a simpler `#[cfg(feature = "text")]` at the `text_layout` mod declaration in `lib.rs` would suffice). Mixes RTL/LTR detection helpers + `TextLayoutResult` + `LineInfo` + `TextLayout` wrapping cosmic-text Buffer + `measure_text` + `measure_inline_span` + the entire FontSystem global. **Split** and **flatten** the cfg layer.
- `text_painter.rs` is 990 LOC. Likely mixes `TextPainter` + `TextBaseline` + measurement + painting + `DEFAULT_FONT_SIZE` constant. **Split.**
- Companion `docs/MIGRATION.md` documents migration from 0.0.x to 0.1.x of a crate that never had a 0.0.x release. Obsolete. **Delete** or compress to a stub note.
- `Paint::clone()` per `Canvas::draw_*` call is an allocation hot spot. For 1000+ commands per frame with reused Paint, this is measurable. **Document** the cost; **file** a Paint-interning Outstanding refactor; do not premature-optimise without measured benefit.
- `restore()` silently no-ops on empty save stack AND `finish()` silently `tracing::warn`s about unrestored saves. Neither catches the programmer error during development. **Promote** the imbalance check to `debug_assert!` in debug builds; keep release-build no-op for Flutter parity.
- `#[forbid(unsafe_code)]` at `lib.rs:151` stays. Zero `unsafe` blocks in the crate today; net unsafe delta for this chain is **0**.

This is not architecture. It is the visible cost of porting Flutter's `Canvas` + `PaintingBinding` + `ShaderWarmUp` class hierarchy 1:1 into Rust without asking whether the abstractions earn their existence at the call sites. The Rust auto-derived `Send`, the closed enum dispatch, the `Drop` trait, the `Option<T>` enum, the `&mut self` borrow checker, and the consumed-once `finish(self)` shape subsume most of the ceremony Flutter's API recommends.

---

## 1. Problem Definition

**Responsibility.** Record canvas drawing operations (`draw_rect`, `draw_path`, `clip_rect`, `save_layer`, …) into an immutable `DisplayList` value. Provide a stack-based scoped-operation surface (`with_save`, `with_translate`, `with_clip_rect`, …) for ergonomic transform/clip lifecycles. Provide a multi-canvas composition surface (`extend_from`, `merge`, `append_display_list`) for parent-child painting workflows. Maintain a `PaintingBinding` singleton for image caching + system font notifications. Maintain a `tessellation` module for path → GPU triangle conversion (feature-gated, `lyon`-backed). Maintain a `text_layout` + `text_painter` pair for text shaping and rendering (feature-gated, `cosmic-text`-backed). Maintain a `ClipContext` cross-crate seam used by `flui-rendering::CanvasContext`.

**Non-responsibility.**
- Layer compositing, scene-building, retained layers (lives in `flui-layer`).
- GPU command-buffer construction, render-pass scheduling, draw-call submission (lives in `flui-engine`).
- Render-object layout/paint dispatch (lives in `flui-rendering`).
- Hit-test target dispatch (`HitRegion` is recorded here; resolution lives in `flui-interaction`).
- Paint/Path/Shader/Image construction (lives in `flui-types::painting`).
- Image decoding (lives in future `flui-assets`).
- Font discovery + glyph rasterisation (lives in `cosmic-text` + future `glyphon` integration).
- Async I/O, scheduling, work-stealing (lives in `flui-scheduler`, `flui-assets`).
- Process-wide singletons beyond `PaintingBinding` (which inherits `flui-foundation`'s `impl_binding_singleton!` macro).

**Callers.** Five crates consume `flui-painting`:
- **`flui-rendering`** (`context/canvas.rs`, `context/clip.rs`, `delegates/custom_painter.rs`, `objects/colored_box.rs`) -- the paint phase creates a `Canvas`, records operations, calls `finish()`, hands the `DisplayList` (as `Picture`) to `flui-layer`. Implements `ClipContext` on `CanvasContext`.
- **`flui-engine`** (`wgpu/painter.rs`, `wgpu/backend.rs`, `wgpu/layer_render.rs`, `wgpu/pipeline.rs`, `wgpu/tessellator.rs`, `wgpu/path_cache.rs`, `wgpu/debug.rs`, `traits.rs`, `commands.rs`, `lib.rs`) -- consumes `DrawCommand` via exhaustive pattern match for GPU lowering. Imports `Paint`, `PaintStyle`, `BlendMode`, `Shader`, `PointMode`, `StrokeCap`, `StrokeJoin`, `ImageFilter`, `ColorFilter`, `ImageRepeat`, `DisplayListCore`, `DrawCommand` directly.
- **`flui-layer`** (`layer/canvas.rs`, `layer/picture.rs`, `tests/scene_builder.rs`) -- wraps `Canvas` mid-recording in `Layer::Canvas(CanvasLayer)` and finished `DisplayList` (aliased as `Picture`) in `Layer::Picture(PictureLayer)`.
- **`flui-app`** (`bindings/renderer_binding.rs`, `bindings/mod.rs`) -- owns the `PaintingBinding` singleton through `flui-foundation`'s binding registry.
- **`flui-rendering`** tests, the `tessellation` example -- the only ext callers of `tessellation::tessellate_fill` / `tessellate_stroke` are `flui-engine`'s wgpu tessellator + the in-crate `examples/simple_tessellation.rs`.

The hot consumption pattern is `for cmd in display_list.commands() { match cmd { … } }` in `flui-engine`'s wgpu backend (`wgpu/backend.rs`, `wgpu/layer_render.rs`, `wgpu/debug.rs`). The `DisplayListCore::commands()` returns `impl Iterator<Item = &DrawCommand>`; no allocation, no cloning, no dispatch tax beyond the iterator's static yield.

**Lifecycle.**
- `Canvas::new()` allocates the inner `DisplayList` + empty transform stack + empty clip stack + empty save stack.
- `Canvas::reset()` clears commands + state but keeps capacity. Frame-loop reuse pattern.
- Per `draw_*` call: pushes one `DrawCommand` onto the inner `Vec<DrawCommand>`.
- Per `save()`: pushes onto `save_stack`. Per `restore()`: pops if non-empty (no-op otherwise).
- Per `save_layer()`: pushes onto `save_stack` with `is_layer: true` and emits `DrawCommand::SaveLayer`. The matching `restore()` emits `DrawCommand::RestoreLayer`.
- `Canvas::finish(self)` consumes the canvas, returns the inner `DisplayList`. The `DisplayList` is now immutable from public API (the `commands_mut` accessor is `pub` today; we lock it down).
- `DisplayList` lives until it's consumed by `flui-engine` or dropped via `flui-layer`'s `Scene` drop.

**Key invariants.**
1. **Single-writer-during-recording.** Canvas is `&mut self` for every mutating method. The borrow checker enforces single-writer.
2. **Save/restore stack depth never underflows.** `restore()` is a no-op when the stack is empty (Flutter parity); `debug_assert!` in debug builds catches imbalance at `finish()` time.
3. **Clip stack depth matches save_stack's `clip_depth` markers.** `save()` records the current clip depth; `restore()` truncates the clip stack to that depth.
4. **Bounds union is monotonic.** Adding a command unions its bounds (if any) with the existing bounds. Removing requires `recalculate_bounds()` (used by `filter`/`map`).
5. **Transform is baked at recording time.** Every `DrawCommand` variant stores its own `Matrix4` (64 bytes). The GPU backend applies the matrix without consulting any external transform state.
6. **`finish(self)` consumes the canvas.** No double-finish, no post-finish mutation. Compiler-enforced.
7. **`DisplayList::push` is `pub(crate)`** -- external mutation goes through `Canvas`. External read goes through `DisplayListCore`/`DisplayListExt`. `commands_mut` (currently `pub`) is the only loophole; we close it in U10.

**Failure modes -- normal, not exceptional.**
- `Canvas::draw_circle(.., radius = NaN, ..)` or `radius < 0.0`: today fires a `debug_assert!` in debug builds; in release, records the command with the bad value and lets GPU/lyon reject it. **Mythos:** keep `debug_assert!`; document the invariant in `## Mapping decisions`; file typed `NonNegativePixels` wrapper in Outstanding refactors.
- `Canvas::draw_shadow(.., elevation < 0.0, ..)`: same pattern.
- `Canvas::restore()` on empty save stack: today silent no-op (Flutter parity); we keep the no-op and add `debug_assert!` opt-in at finish() time.
- `Canvas::finish()` with N unrestored saves: today `tracing::warn!(unrestored_saves = N)`; release-build behaviour preserved. Add `debug_assert!(self.save_stack.is_empty(), ...)` for tests.
- Tessellation failure (`lyon` rejects a malformed path): returns `TessellationError::FillError`/`StrokeError`. Kept separate from `PaintingError` (narrow callers, narrow propagation).
- cosmic-text font lookup miss: cosmic-text falls back to the default font; no error surface today; documented as Flutter parity (Flutter's `TextPainter` does the same).
- `PaintingBinding::image_cache::put` over the configured byte limit: silent LRU eviction. Caller does not see backpressure. Documented (image cache is best-effort, not a guarantee).
- `Canvas::add_hit_region(...)` records a `HitRegion` carrying an `Arc<dyn Fn(&PointerEvent) + Send + Sync>`. The callback panics during `flui-interaction`'s hit-test pump: not this crate's concern; `flui-interaction` is responsible for `catch_unwind`-style sandboxing.

---

## 2. Architecture Overview

```text
flui-rendering (paint phase)
  │  canvas.draw_rect(...); canvas.save(); canvas.clip_rect(...); canvas.draw_path(...)
  ▼
Canvas                            ◄── single mutable owner; transform + clip + save stacks
  │  draw_* (29 primitive ops) → DrawCommand emit
  │  clip_* / save / restore / save_layer → state mutations
  │  with_save / with_translate / ... → scoped helpers (zero-cost wrappers)
  │  extend_from / merge / append_* → multi-canvas composition
  ▼
DisplayList                       ◄── inner buffer during recording; immutable after finish
  │  Vec<DrawCommand>             (closed enum; 29 variants; GPU lowering vocabulary)
  │  Rect<Pixels> bounds          (incremental union)
  │  Vec<HitRegion>               (event-handler registry)
  ▼
canvas.finish(self) → DisplayList ◄── consumed-once
  │
  ├─▶ flui-layer::Layer::Picture(PictureLayer { display_list: Arc<DisplayList> })
  │     └─▶ flui-engine::WgpuBackend::render_layer(layer)
  │           └─▶ for cmd in display_list.commands() { match cmd { … } }   ◄── GPU lowering
  │
  └─▶ flui-layer::Layer::Canvas(CanvasLayer { canvas: Canvas })
        └─▶ canvas.finish() → DisplayList → engine matches → GPU
```

```text
flui-app
  └─▶ PaintingBinding (singleton)
        ├─▶ ImageCache          (cache + live_images; RwLock<HashMap>; off hot path)
        └─▶ SystemFontsNotifier (listener vec; RwLock<Vec<Arc<dyn Fn>>>; off hot path)
        ── deleted in this chain: shader_warm_up: Option<Box<dyn ShaderWarmUp>>
```

```text
text_layout (feature = "text", cosmic-text)
  │  static FONT_SYSTEM: OnceLock<Mutex<FontSystem>>   ◄── one-time init + lock-per-shape
  │  TextLayout::new(text, style, font_size, ...)
  ▼
cosmic_text::Buffer (shaping result)
  │  metrics() / glyph_runs() / hit_test(...) / get_offset_for_caret(...)
  ▼
flui-rendering paint phase → canvas.draw_text(text, offset, size, style, paint)
```

No `Arc<RwLock<Canvas>>` on the diagram. No `Box<dyn Drawable>` plugin trait. No `Arc<Mutex<Vec<Box<dyn Fn>>>>` callback registry. No `WarmUpCanvas` plugin shape. No `Box<dyn ShaderWarmUp>` plug.

**What goes away from current code:**
- `WarmUpCanvas` trait (4 abstract methods, 0 prod impls). U1 deletion.
- `ShaderWarmUp` trait + `DefaultShaderWarmUp` struct + `Option<Box<dyn ShaderWarmUp>>` field on `PaintingBinding` + `with_shader_warm_up` constructor + `set_shader_warm_up` setter. U2 deletion (decorative subsystem; track real impl in Outstanding refactors).
- 3,305 LOC `canvas.rs` god module → `canvas/{mod,state,drawing,scoped,composition}.rs`. U4 split.
- 2,434 LOC `display_list.rs` god module → `display_list/{mod,command,command_ops,sealed,stats,hit_region}.rs`. U5 split.
- 1,243 LOC `text_layout.rs` (with unnecessary `#[cfg(feature = "text")] mod inner` indirection) → `text_layout/{mod,detect,layout,line_info,measure}.rs` (cfg moved to mod declaration). U6 split.
- 990 LOC `text_painter.rs` → `text_painter/{mod,baseline,paint,measure}.rs` or similar concern boundary. U7 split.
- 9 inline `#[cfg(test)] mod tests` blocks across the four god modules → integration tests in `crates/flui-painting/tests/`. U8 extraction.
- `tracing::warn!(unrestored_saves = N)` in `finish()` → keep tracing for release-build observability; add `debug_assert!(save_stack.is_empty(), ...)` for catch-during-tests. U10 error model.
- Companion `docs/MIGRATION.md` (obsolete migration notes between non-existent versions) → delete or stub. U12 doc-graft.

**What earns its place:**
- `Canvas` struct + state stacks + 29 `draw_*` methods + 12 `with_*` scoped helpers + 5 composition methods. Stays; split across submodules by concern.
- `DisplayList` struct + 29-variant `DrawCommand` enum + `DisplayListStats` + `HitRegion` + `PointerEvent`. Stays; split.
- `DisplayListCore` / `DisplayListExt` sealed-trait pair + 4 blanket impls (`DisplayList`, `Arc<DisplayList>`, `Box<DisplayList>`, `&DisplayList`). Stays. Documented as the sealed-extension-trait pattern (precedent: `flui-rendering`'s extension-trait split at commit `d0e53c63`).
- `ClipContext` trait + 3 default `clip_*_and_paint` methods. Stays. Single prod impl `CanvasContext` in `flui-rendering` is the legitimate cross-crate seam; sealing it would force `flui-rendering` into an awkward concrete-type position.
- `PaintingBinding` + `ImageCache` + `SystemFontsNotifier`. Stays (trimmed). The two `RwLock` sites are off-hot-path per [`docs/PORT.md`](../PORT.md) lock-decision table.
- `tessellation` module (feature-gated, `lyon`-backed). Stays as-is; clean.
- `text_layout` + `text_painter` (feature-gated, `cosmic-text`-backed). Split into submodules; flatten the `mod inner` cfg layer.
- `error.rs` with `PaintingError` + 5 variants. Stays; one variant may be added in U10 if a real surface emerges.
- `Picture = DisplayList` type alias for Flutter parity. Stays.
- `lib.rs` re-exports. Trimmed to drop `WarmUpCanvas`, `ShaderWarmUp`, `DefaultShaderWarmUp`. Other re-exports stay.

---

## 3. Core Types

```rust
// ───────────────────────────────────────────────────────────────
// Canvas — single-owner recording surface
// ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Canvas {
    display_list: DisplayList,        // inner buffer; commands grow here
    transform: Matrix4,               // current accumulated transform (baked into emitted commands)
    clip_stack: Vec<ClipShape>,       // active clips; restored via save/restore
    save_stack: Vec<CanvasState>,     // saved (transform, clip_depth, is_layer) tuples
}

#[derive(Debug, Clone)]
struct CanvasState {
    transform: Matrix4,
    clip_depth: usize,
    is_layer: bool,
}

#[derive(Debug, Clone)]
enum ClipShape {
    Rect(Rect<Pixels>),
    RRect(RRect),
    Path(Box<Path>),                  // Path is `Vec<PathCommand>` interior; Box for size-uniform variant
}

// Canvas is auto-Send + auto-!Sync.
//   Send:  Vec<DrawCommand>, Matrix4, Vec<ClipShape>, Vec<CanvasState> all auto-Send.
//   !Sync: HitRegionHandler is Arc<dyn Fn(&PointerEvent) + Send + Sync>, which IS Sync,
//          but the recording API is fundamentally single-threaded; `!Sync` is a design choice
//          enforced by the absence of any shared mutability primitive on Canvas itself.

// ───────────────────────────────────────────────────────────────
// DisplayList — consumed-once immutable command buffer
// ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DisplayList {
    commands: Vec<DrawCommand>,
    bounds: Rect<Pixels>,
    #[cfg_attr(feature = "serde", serde(skip))]
    hit_regions: Vec<HitRegion>,
}

// Public surface:
//   .commands() / .bounds() / .len() / .is_empty()           via DisplayListCore (sealed)
//   .draw_commands() / .clip_commands() / .shape_commands()
//     .image_commands() / .text_commands() / .count_by_kind()
//     .stats()                                                via DisplayListExt (blanket on DisplayListCore)
//   .iter() / .iter_mut()                                     direct std::slice::Iter / IterMut
//   .add_hit_region(_) / .hit_regions()                       HitRegion API
//   .apply_transform(matrix) / .filter(pred) / .map(f) / .to_opacity(f) / .clear()
//   pub(crate) push(DrawCommand)                              internal; Canvas-only
//   pub(crate) append(other: DisplayList)                     internal; Canvas-only

// ───────────────────────────────────────────────────────────────
// DrawCommand — closed-enum vocabulary; 29 variants
// ───────────────────────────────────────────────────────────────

/// The paint operation vocabulary that flui-engine pattern-matches for GPU lowering.
///
/// Every variant has a documented GPU translation in `flui-engine::WgpuBackend`.
/// Adding a variant is a coordinated change in `flui-painting` + `flui-engine`
/// (+ optionally `flui-rendering` if a render-object should emit it).
///
/// This is **deliberately the same shape** as `flui-layer::Layer` — a closed
/// enum, exhaustive match, no third-party `impl Drawable`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]   // future-proofing for internal additions; external callers still must match all today
pub enum DrawCommand {
    // === Clipping (3 variants) ===
    ClipRect    { rect: Rect<Pixels>, clip_op: ClipOp, clip_behavior: Clip, transform: Matrix4 },
    ClipRRect   { rrect: RRect,       clip_op: ClipOp, clip_behavior: Clip, transform: Matrix4 },
    ClipPath    { path: Path,         clip_op: ClipOp, clip_behavior: Clip, transform: Matrix4 },

    // === Primitive shapes (8 variants) ===
    DrawLine    { p1: Point<Pixels>, p2: Point<Pixels>, paint: Paint, transform: Matrix4 },
    DrawRect    { rect: Rect<Pixels>, paint: Paint, transform: Matrix4 },
    DrawRRect   { rrect: RRect, paint: Paint, transform: Matrix4 },
    DrawCircle  { center: Point<Pixels>, radius: Pixels, paint: Paint, transform: Matrix4 },
    DrawOval    { rect: Rect<Pixels>, paint: Paint, transform: Matrix4 },
    DrawPath    { path: Path, paint: Paint, transform: Matrix4 },
    DrawArc     { rect: Rect<Pixels>, start_angle: f32, sweep_angle: f32,
                  use_center: bool, paint: Paint, transform: Matrix4 },
    DrawDRRect  { outer: RRect, inner: RRect, paint: Paint, transform: Matrix4 },

    // === Text (2 variants) ===
    DrawText        { text: String, offset: Offset<Pixels>, size: Size<Pixels>,
                      style: TextStyle, paint: Paint, transform: Matrix4 },
    DrawTextSpan    { span: InlineSpan, offset: Offset<Pixels>,
                      text_scale_factor: f64, transform: Matrix4 },

    // === Image (4 variants) ===
    DrawImage           { image: Image, dst: Rect<Pixels>, paint: Option<Paint>, transform: Matrix4 },
    DrawImageRepeat     { image: Image, dst: Rect<Pixels>, repeat: ImageRepeat,
                          paint: Option<Paint>, transform: Matrix4 },
    DrawImageNineSlice  { image: Image, center_slice: Rect<Pixels>, dst: Rect<Pixels>,
                          paint: Option<Paint>, transform: Matrix4 },
    DrawImageFiltered   { image: Image, dst: Rect<Pixels>, filter: ColorFilter,
                          paint: Option<Paint>, transform: Matrix4 },

    // === Texture (1 variant) ===
    DrawTexture { texture_id: TextureId, dst: Rect<Pixels>, src: Option<Rect<Pixels>>,
                  filter_quality: FilterQuality, opacity: f32, transform: Matrix4 },

    // === Atlas + advanced (4 variants) ===
    DrawAtlas    { image: Image, sprites: Vec<Rect<Pixels>>, transforms: Vec<Matrix4>,
                   colors: Option<Vec<Color>>, blend_mode: BlendMode,
                   paint: Option<Paint>, transform: Matrix4 },
    DrawPoints   { mode: PointMode, points: Vec<Point<Pixels>>, paint: Paint, transform: Matrix4 },
    DrawVertices { vertices: Vec<Point<Pixels>>, colors: Option<Vec<Color>>,
                   tex_coords: Option<Vec<Point<Pixels>>>, indices: Vec<u16>,
                   paint: Paint, transform: Matrix4 },
    DrawShadow   { path: Path, color: Color, elevation: f32, transform: Matrix4 },

    // === Fill + gradient (3 variants) ===
    DrawColor          { color: Color, blend_mode: BlendMode, transform: Matrix4 },
    DrawPaint          { paint: Paint, transform: Matrix4 },
    DrawGradient       { rect: Rect<Pixels>, shader: Shader, transform: Matrix4 },
    DrawGradientRRect  { rrect: RRect, shader: Shader, transform: Matrix4 },

    // === Effects with child sub-display-lists (2 variants) ===
    ShaderMask     { child: Box<DisplayList>, shader: Shader, bounds: Rect<Pixels>,
                     blend_mode: BlendMode, transform: Matrix4 },
    BackdropFilter { child: Option<Box<DisplayList>>, filter: ImageFilter,
                     bounds: Rect<Pixels>, blend_mode: BlendMode, transform: Matrix4 },

    // === Layer save/restore (2 variants) ===
    SaveLayer    { bounds: Option<Rect<Pixels>>, paint: Paint, transform: Matrix4 },
    RestoreLayer { transform: Matrix4 },
}

// DrawCommand methods (defined in display_list/command_ops.rs after U5 split):
//   .with_opacity(opacity) -> DrawCommand     (allocates; full-variant pattern match)
//   .bounds() -> Option<Rect<Pixels>>         (allocates only for atlas multi-sprite union)
//   .transform() -> Matrix4                   (Copy; no allocation)
//   .transform_mut() -> &mut Matrix4
//   .paint() -> Option<&Paint>
//   .kind() -> CommandKind                    (Draw / Clip / Effect / Layer)
//   .is_draw() / .is_clip() / .is_shape() / .is_image() / .is_text() / .is_effect() / .is_layer()
//   .apply_transform(matrix)                  (mutates self.transform)

// ───────────────────────────────────────────────────────────────
// Sealed extension-trait pair
// ───────────────────────────────────────────────────────────────

#[doc(hidden)]
pub mod private {
    pub trait Sealed {}
}

pub trait DisplayListCore: private::Sealed {
    fn commands(&self) -> impl Iterator<Item = &DrawCommand>;
    fn bounds(&self) -> Rect<Pixels>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool { self.len() == 0 }
}

pub trait DisplayListExt: DisplayListCore {
    fn draw_commands(&self)   -> impl Iterator<Item = &DrawCommand> { /* filter */ }
    fn clip_commands(&self)   -> impl Iterator<Item = &DrawCommand> { /* filter */ }
    fn shape_commands(&self)  -> impl Iterator<Item = &DrawCommand> { /* filter */ }
    fn image_commands(&self)  -> impl Iterator<Item = &DrawCommand> { /* filter */ }
    fn text_commands(&self)   -> impl Iterator<Item = &DrawCommand> { /* filter */ }
    fn count_by_kind(&self)   -> (usize, usize, usize, usize) { /* iterate once */ }
    fn stats(&self)           -> DisplayListStats { /* iterate */ }
}
impl<T: DisplayListCore> DisplayListExt for T {}

// Sealed for: DisplayList, Arc<DisplayList>, Box<DisplayList>, &DisplayList.
// Each gets a DisplayListCore impl that re-routes through the inner DisplayList.

// ───────────────────────────────────────────────────────────────
// ClipContext — cross-crate seam (1 prod impl: CanvasContext in flui-rendering)
// ───────────────────────────────────────────────────────────────

pub trait ClipContext {
    fn canvas_mut(&mut self) -> &mut Canvas;   // only required method

    // Three default methods provide the boilerplate-saving surface:
    fn clip_rect_and_paint<F>(&mut self, rect: Rect<Pixels>, clip_behavior: Clip,
                              bounds: Rect<Pixels>, painter: F) where F: FnOnce(&mut Self) { /* ... */ }
    fn clip_rrect_and_paint<F>(&mut self, rrect: RRect, clip_behavior: Clip,
                               bounds: Rect<Pixels>, painter: F) where F: FnOnce(&mut Self) { /* ... */ }
    fn clip_path_and_paint<F>(&mut self, path: &Path, clip_behavior: Clip,
                              bounds: Rect<Pixels>, painter: F) where F: FnOnce(&mut Self) { /* ... */ }
}

// ───────────────────────────────────────────────────────────────
// PaintingBinding — trimmed singleton
// ───────────────────────────────────────────────────────────────

pub struct PaintingBinding {
    image_cache:  ImageCache,
    system_fonts: SystemFontsNotifier,
    // ── deleted in U2: shader_warm_up: Option<Box<dyn ShaderWarmUp>>
}

impl PaintingBinding {
    pub fn new() -> Self { /* ... */ }
    pub fn image_cache(&self)       -> &ImageCache         { &self.image_cache }
    pub fn image_cache_mut(&mut self) -> &mut ImageCache   { &mut self.image_cache }
    pub fn system_fonts(&self)      -> &SystemFontsNotifier { &self.system_fonts }
    pub fn handle_memory_pressure(&self) { self.image_cache.clear(); }
    pub fn handle_system_message(&self, message_type: &str) { /* fontsChange dispatch */ }
    // ── deleted in U3: with_shader_warm_up, set_shader_warm_up
}

// ───────────────────────────────────────────────────────────────
// Errors — narrow, structured (Cow-backed reasons; non_exhaustive)
// ───────────────────────────────────────────────────────────────

#[non_exhaustive]
#[derive(Error, Debug, Clone)]
pub enum PaintingError {
    #[error("Failed to paint decoration: {reason}")]
    PaintDecorationFailed { reason: Cow<'static, str> },

    #[error("Invalid decoration: {reason}")]
    InvalidDecoration { reason: Cow<'static, str> },

    #[error("Invalid gradient: {reason}")]
    InvalidGradient { reason: Cow<'static, str> },

    #[error("Text painting failed: {reason}")]
    PaintTextFailed { reason: Cow<'static, str> },

    #[error("Image operation failed: {reason}")]
    PaintImageFailed { reason: Cow<'static, str> },
}

pub type Result<T> = std::result::Result<T, PaintingError>;
```

The crate is `#[forbid(unsafe_code)]` at `lib.rs:151`. **Net `unsafe` delta for this chain: 0.** Zero blocks in, zero blocks out.

---

## 4. State Machine

Two state machines: **canvas recording** and **save/restore stack**.

### Canvas recording

```text
Canvas::new() / Canvas::default()
  │
  ▼
Recording                          ◄── draw_*, clip_*, save, restore, save_layer, with_*, extend_from, …
  │
  │ canvas.finish(self)
  ▼
DisplayList (immutable)            ◄── Canvas value is gone; type system forbids reuse
  │
  │ engine consumes / flui-layer wraps / dropped
  ▼
(consumed)
```

`Canvas::reset(&mut self)` is the **only** reverse transition: it returns a recorded Canvas back to the recording state with cleared internals + preserved capacity. Used by frame-loop reuse (`canvas.reset(); render(&mut canvas); let dl = canvas.finish();`). The reset clears commands, transform, clip_stack, save_stack.

`Canvas::clear_commands(&mut self)` is a softer variant: clears commands but keeps transform + clip + save state. Used when the caller wants to re-record with the same coordinate system.

No typestate parameter is introduced. The consumed-once `finish(self)` already enforces the post-recording immutability at compile time; adding a `Canvas<Recording>` / `Canvas<Finished>` typestate would force every `&mut Canvas` parameter in the workspace to monomorphise on the typestate, which is API ceremony without payback.

### Save/restore stack

```text
save_stack: Vec<CanvasState>       ◄── (transform, clip_depth, is_layer) tuples

save()                                  push current state; clip_depth = clip_stack.len()
save_layer(bounds, paint)               same as save() + is_layer = true + emit SaveLayer command
restore()                               if pop returns is_layer: emit RestoreLayer; restore transform; truncate clip_stack
restore() on empty stack                no-op (Flutter parity)
finish() with non-empty save_stack      debug_assert!(empty); tracing::warn(unrestored_saves = N); release-safe
```

The single legitimate consumer of a non-empty `save_stack` at `finish()` is `flui-rendering`'s test fixtures that intentionally exercise the warning path. We surface a `Result`-returning `try_finish()` companion for callers who want explicit handling -- **rejected** as ceremony (see Section 12, rejected design "Make `finish()` fallible").

---

## 5. Public API

The crate's public surface. Everything else lives behind `pub(crate)` or further.

```rust
// ── Canvas construction + lifecycle
Canvas::new() -> Canvas
Canvas::default() -> Canvas
Canvas::finish(self) -> DisplayList
Canvas::reset(&mut self)
Canvas::clear_commands(&mut self)

// ── State (transform + save/restore)
Canvas::transform_matrix(&self) -> Matrix4
Canvas::translate(&mut self, dx, dy)
Canvas::scale_uniform(&mut self, factor) / scale_xy(sx, sy)
Canvas::rotate(&mut self, radians) / rotate_around(radians, pivot_x, pivot_y)
Canvas::skew(&mut self, sx, sy)
Canvas::transform<T: Into<Matrix4>>(&mut self, transform: T)
Canvas::set_transform<T: Into<Matrix4>>(&mut self, transform: T)
Canvas::save(&mut self) / restore(&mut self)
Canvas::save_count(&self) -> usize
Canvas::restore_to_count(&mut self, count: usize)
Canvas::save_layer(&mut self, bounds: Option<Rect<Pixels>>, paint: &Paint)
Canvas::save_layer_alpha / save_layer_opacity / save_layer_blend (convenience)

// ── Clipping
Canvas::clip_rect / clip_rrect / clip_path (default ClipOp::Intersect, AntiAlias)
Canvas::clip_rect_ext / clip_rrect_ext / clip_path_ext (explicit ClipOp + Clip)
Canvas::local_clip_bounds(&self) -> Option<Rect<Pixels>>
Canvas::device_clip_bounds(&self) -> Option<Rect<Pixels>>
Canvas::would_be_clipped(&self, rect: &Rect<Pixels>) -> Option<bool>

// ── Drawing primitives (29 methods, one per DrawCommand variant)
Canvas::draw_line / draw_rect / draw_rrect / draw_circle / draw_oval
Canvas::draw_path / draw_arc / draw_drrect
Canvas::draw_text / draw_text_span
Canvas::draw_image / draw_image_repeat / draw_image_nine_slice / draw_image_filtered
Canvas::draw_texture / draw_shadow
Canvas::draw_gradient / draw_gradient_rrect
Canvas::draw_atlas / draw_points / draw_points_with_mode / draw_polyline / draw_point
Canvas::draw_vertices
Canvas::draw_color / draw_paint
Canvas::draw_shader_mask / draw_backdrop_filter (closure-based child capture)
Canvas::draw_picture(&mut self, picture: &DisplayList)

// ── Scoped operations (auto save/restore)
Canvas::with_save / with_translate / with_rotate / with_rotate_around
Canvas::with_scale / with_scale_xy / with_transform
Canvas::with_clip_rect / with_clip_rrect / with_clip_path
Canvas::with_opacity / with_blend_mode

// ── Multi-canvas composition
Canvas::extend_from(&mut self, other: Canvas)
Canvas::extend<I: IntoIterator<Item = Canvas>>(&mut self, others: I)
Canvas::merge(self, other: Canvas) -> Canvas
Canvas::append_display_list(&mut self, display_list: DisplayList)
Canvas::append_display_list_at_offset(&mut self, display_list: &DisplayList, offset: Offset<Pixels>)

// ── Hit testing recording
Canvas::add_hit_region(&mut self, region: HitRegion)

// ── Query
Canvas::is_empty(&self) -> bool
Canvas::len(&self) -> usize
Canvas::bounds(&self) -> Rect<Pixels>
Canvas::display_list(&self) -> &DisplayList
impl AsRef<DisplayList> for Canvas

// ── DisplayList API
DisplayList::new() / default()
DisplayList::iter / iter_mut          (std::slice::Iter)
DisplayList::commands_mut             (DEPRIVATE: pub(crate) in U10)
DisplayList::add_hit_region / hit_regions
DisplayList::apply_transform / filter / map / to_opacity / clear
impl DisplayListCore + DisplayListExt for DisplayList, Arc<DisplayList>, Box<DisplayList>, &DisplayList

// ── DrawCommand API
DrawCommand::with_opacity(opacity) -> Self
DrawCommand::bounds() -> Option<Rect<Pixels>>
DrawCommand::transform() / transform_mut() / paint() / has_paint()
DrawCommand::kind() / is_draw / is_clip / is_shape / is_image / is_text / is_effect / is_layer
DrawCommand::apply_transform(matrix: Matrix4)

// ── DisplayListStats
DisplayListStats::zero() / new(total, draw, clip, effect, layer, shapes, images, text, hit_regions)
impl Display for DisplayListStats

// ── ClipContext (1 prod impl)
trait ClipContext { fn canvas_mut(&mut self) -> &mut Canvas; clip_*_and_paint defaults; }

// ── PaintingBinding singleton (TRIMMED)
PaintingBinding::new() / default()
PaintingBinding::image_cache() / image_cache_mut() / system_fonts()
PaintingBinding::handle_memory_pressure() / handle_system_message(message_type)
PaintingBinding::evict(asset: &str)
PaintingBinding::instance() -> &'static PaintingBinding   (via impl_binding_singleton! macro)

// ── ImageCache
ImageCache::new() / with_limits(max_images, max_size_bytes)
ImageCache::get / put / evict / clear / mark_live / unmark_live / clear_live_images
ImageCache::max_images / set_max_images / max_size_bytes / set_max_size_bytes
ImageCache::count / current_size_bytes

// ── SystemFontsNotifier
SystemFontsNotifier::new() / add_listener / remove_listener / notify_listeners

// ── HitRegion
HitRegion::new(bounds, handler) / contains(point)
HitRegionHandler = Arc<dyn Fn(&PointerEvent) + Send + Sync>

// ── PointerEvent (re-exported for HitRegion users)
PointerEvent::new(kind, position, pointer)
PointerEventKind: Enter, Exit, Down, Move, Up, Cancel

// ── Text (feature = "text")
TextLayout::new(text, style, font_size, max_width, line_height, direction)
TextLayout::metrics() / get_offset_for_caret / hit_test / ...
TextPainter::*
detect_text_direction(text) / measure_text(text, style, ...) / measure_inline_span(...)

// ── Tessellation (feature = "tessellation")
tessellate_fill(path, options) -> Result<TessellatedPath, TessellationError>
tessellate_stroke(path, stroke_width, options) -> Result<TessellatedPath, TessellationError>
TessellationOptions { tolerance, anti_alias } / TessellatedPath { vertices, indices }
TessellationError: FillError(String) / StrokeError(String) / InvalidPath(String)

// ── Errors
PaintingError (5 variants, non_exhaustive, Cow-backed)
pub type Result<T> = std::result::Result<T, PaintingError>;

// ── Flutter parity alias
pub type Picture = DisplayList;

// ── Re-exports from flui_types::painting
BlendMode, Paint, PaintBuilder, PaintStyle, PointMode, Shader, StrokeCap, StrokeJoin
```

**Compared to today, the following surface disappears:**

| Today | Replacement / disposition |
|---|---|
| `pub trait WarmUpCanvas` (4 abstract methods, 0 prod impls) | Deleted in U1. Pure dead code. |
| `pub trait ShaderWarmUp` (1 impl, decorative `execute()`) | Deleted in U2. Real warm-up tracked as Outstanding refactor. |
| `pub struct DefaultShaderWarmUp` | Deleted in U2 alongside the trait. |
| `PaintingBinding::with_shader_warm_up(warm_up: Box<dyn ShaderWarmUp>)` | Deleted in U3. |
| `PaintingBinding::set_shader_warm_up(warm_up: Box<dyn ShaderWarmUp>)` | Deleted in U3. |
| `PaintingBinding::shader_warm_up: Option<Box<dyn ShaderWarmUp>>` field | Deleted in U3. |
| Inline `#[cfg(feature = "text")] mod inner` indirection in text_layout.rs | Flattened in U6 (cfg moves to mod declaration in lib.rs). |
| `DisplayList::commands_mut` public method | Demoted to `pub(crate)` in U10. The external use case (transforms applied to a finished DisplayList) goes through the existing `apply_transform` method. |

**Methods that stay but get re-shuffled across submodules in U4-U7:** the entire `Canvas` API (split by concern), the entire `DrawCommand` impl block (split by purpose), the entire `text_layout` API (split by pipeline phase), the entire `text_painter` API (split similarly).

---

## 6. Internal Modules

```text
crates/flui-painting/src/
  lib.rs                      — re-exports, prelude, #[forbid(unsafe_code)], lints
  error.rs                    — PaintingError + Result
  binding.rs                  — PaintingBinding + ImageCache + SystemFontsNotifier
                                (trimmed: no ShaderWarmUp, no WarmUpCanvas, no shader_warm_up field)
  clip_context.rs             — ClipContext trait + 3 default methods
  canvas/                     — split from 3,305-LOC canvas.rs in U4
    mod.rs                    — Canvas struct + Default/Clone/Debug + AsRef impl + finalisation
    state.rs                  — CanvasState + ClipShape + save/restore/save_layer/save_count/restore_to_count
    transform.rs              — translate / scale_* / rotate* / skew / transform / set_transform / transform_matrix
    clipping.rs               — clip_rect / clip_rrect / clip_path / *_ext + clip query helpers
    drawing.rs                — 29 draw_* primitives (line, rect, rrect, circle, oval, path, text,
                                  text_span, image*, texture, shadow, gradient*, arc, drrect, points,
                                  vertices, color, paint, atlas, shader_mask, backdrop_filter, picture)
    scoped.rs                 — 12 with_* helpers (with_save, with_translate, with_rotate*,
                                  with_scale*, with_transform, with_clip_*, with_opacity, with_blend_mode)
    composition.rs            — extend_from / extend / merge / append_display_list / append_display_list_at_offset
  display_list/               — split from 2,434-LOC display_list.rs in U5
    mod.rs                    — DisplayList struct + Default + iter/iter_mut + apply_transform/filter/map/
                                  to_opacity/clear + commands_mut (pub(crate)) + Display for stats
    command.rs                — DrawCommand enum (29 variants) + CommandKind enum
    command_ops.rs            — DrawCommand impl: with_opacity, bounds, transform, transform_mut, paint,
                                  has_paint, kind, is_*, apply_transform
    sealed.rs                 — private::Sealed module + DisplayListCore trait + DisplayListExt trait
                                  + blanket impls for DisplayList, Arc<DisplayList>, Box<DisplayList>, &DisplayList
    stats.rs                  — DisplayListStats struct + zero() + new() + Display impl
    hit_region.rs             — PointerEvent + PointerEventKind + HitRegion + HitRegionHandler
  text_layout/                — split from 1,243-LOC text_layout.rs in U6; cfg flat at mod decl
    mod.rs                    — re-exports + module-level docs
    detect.rs                 — detect_text_direction + is_rtl_char + is_ltr_char
    layout.rs                 — TextLayout struct + new + metrics + cursor + hit_test
    line_info.rs              — LineInfo struct + TextLayoutResult struct
    measure.rs                — measure_text / measure_inline_span + style_to_attrs helpers
  text_painter/               — split from 990-LOC text_painter.rs in U7
    mod.rs                    — re-exports + TextPainter struct + DEFAULT_FONT_SIZE constant
    baseline.rs               — TextBaseline enum + baseline computation
    paint.rs                  — TextPainter paint methods + Canvas integration
    measure.rs                — measurement-side helpers if separable
  tessellation.rs             — feature = "tessellation"; lyon-backed; untouched
  tests/                      — integration tests pulled out of inline #[cfg(test)] mods (U8)
    canvas_state.rs           — extracted from canvas.rs tests
    display_list_command.rs   — extracted from display_list.rs tests
    text_layout_detect.rs     — extracted from text_layout.rs tests
    text_painter.rs           — extracted from text_painter.rs tests
    (existing tests stay: rich_text_example.rs, text_layout_pipeline.rs, canvas_scoped.rs,
                          canvas_transform.rs, thread_safety.rs, canvas_composition.rs,
                          tessellation_integration.rs)
```

**What earned its place vs. what didn't.**

Earned existence:
- `binding.rs` -- PaintingBinding + ImageCache + SystemFontsNotifier. Process-wide singleton + image cache + system font listener. Without it, no Flutter-parity binding for the painting subsystem.
- `clip_context.rs` -- the cross-crate seam consumed by `flui-rendering::CanvasContext`. The three default methods save ~50 LOC of boilerplate at the caller; deleting them would push that boilerplate into flui-rendering.
- `canvas/state.rs` -- save/restore is its own state machine and deserves a focused file.
- `canvas/transform.rs` -- 7 transform methods + Matrix4 ops form a coherent concern.
- `canvas/clipping.rs` -- 6 clip methods + 4 clip query helpers form a coherent concern.
- `canvas/drawing.rs` -- 29 draw_* primitives form the largest single concern; sub-split is possible (per-variant-group) but cosmetic.
- `canvas/scoped.rs` -- 12 with_* helpers are zero-cost wrappers over save/restore; their own file.
- `canvas/composition.rs` -- multi-canvas composition is its own concern.
- `display_list/command.rs` -- the DrawCommand enum + CommandKind. The vocabulary itself.
- `display_list/command_ops.rs` -- the 7 impl blocks on DrawCommand (with_opacity, bounds, transform, paint, kind, is_*, apply_transform). One file for the operations, separate from the variant declarations.
- `display_list/sealed.rs` -- the sealed-extension-trait pair + 4 blanket impls. Standalone responsibility.
- `display_list/stats.rs` -- DisplayListStats + 12-field Display impl. Separable from the runtime.
- `display_list/hit_region.rs` -- HitRegion + PointerEvent + handler type. Separate concern from the display-list core.
- `text_layout/*` -- one file per pipeline phase: detect, layout, line_info, measure.
- `text_painter/*` -- same split, by paint subconcern.
- `tessellation.rs` -- already focused; one file, 533 LOC, clean.

Did not earn its place -- proposed deletions:
- `WarmUpCanvas` trait + its 4-method declaration block. 0 callers. Pure dead code. **Delete in U1.**
- `ShaderWarmUp` trait + `DefaultShaderWarmUp` struct + `Option<Box<dyn ShaderWarmUp>>` field + `with_shader_warm_up` constructor + `set_shader_warm_up` setter. 1 impl + 0 real consumers. The `execute()` body does nothing real. **Delete in U2.**
- `#[cfg(feature = "text")] mod inner { … }` indirection inside `text_layout.rs`. The cfg should sit on the mod declaration in `lib.rs`. **Flatten in U6.**
- `DisplayList::commands_mut` public method. External callers should go through `apply_transform`/`filter`/`map`. **Demote to `pub(crate)` in U10.**
- Companion `docs/MIGRATION.md` (migration from 0.0.x to 0.1.x of a crate that never had 0.0.x). **Delete or stub in U12.**

**What `lib.rs` re-exports.** Today 293 lines. After the chain:
- `Canvas` (from `canvas/mod.rs`)
- `DisplayList`, `DisplayListCore`, `DisplayListExt`, `DisplayListStats`, `DrawCommand`, `CommandKind`, `HitRegion`, `HitRegionHandler`, `PointerEvent`, `PointerEventKind` (from `display_list/*`)
- `Picture` type alias = `DisplayList`
- `ClipContext`
- `PaintingError`, `Result`
- `PaintingBinding`, `ImageCache`, `CachedImage`, `ImageHandle`, `SystemFontsNotifier`, `image_cache()` (from `binding`)
- `TextLayout`, `TextLayoutResult`, `LineInfo`, `detect_text_direction`, `measure_text`, `measure_inline_span` (from `text_layout`, feature-gated)
- `TextBaseline`, `TextPainter`, `DEFAULT_FONT_SIZE` (from `text_painter`, feature-gated)
- `tessellate_fill`, `tessellate_stroke`, `TessellationOptions`, `TessellatedPath`, `TessellationVertex`, `TessellationError` (from `tessellation`, feature-gated)
- Re-exports from `flui_types::painting`: `BlendMode`, `Paint`, `PaintBuilder`, `PaintStyle`, `PointMode`, `Shader`, `StrokeCap`, `StrokeJoin`
- **Deleted:** `WarmUpCanvas`, `ShaderWarmUp`, `DefaultShaderWarmUp`

Estimated final re-export count: ~40 names (down from 47 today).

---

## 7. Async & Failure Semantics

**Task ownership.** Zero. The crate runs synchronously inside the paint phase (recording) and inside the engine's render pass (consumption). There is no `tokio::spawn`, no `JoinHandle`, no `async fn` anywhere in `src/`.

**Cancellation.** Not applicable. A `Canvas` is either built, finished, and consumed -- or dropped mid-recording. Dropping mid-recording drops the inner `Vec<DrawCommand>`, `Vec<ClipShape>`, `Vec<CanvasState>`, and any `Arc<dyn Fn>` hit-region handlers. No side effects to undo.

**Retry.** Not applicable at the painting level. If a frame fails to record (would only happen if a render-object panics during `paint()`), the higher-level pipeline (`flui-rendering`'s pipeline phase, Mythos Step 12 commit `dc0fa1ad`) handles it via `catch_unwind`.

**Idempotency.** Tree mutation is **not** strictly idempotent at `draw_*` level -- calling `canvas.draw_rect(rect, &paint)` twice records two `DrawCommand::DrawRect` entries. This is intentional Flutter parity. The GPU will draw the rectangle twice (overdraw); the engine does not coalesce duplicates.

**Backpressure.** Not applicable -- no channel, no queue. The image cache silently LRU-evicts when over its byte/count limits; documented as best-effort with no caller-visible signal.

**Shutdown.** `Canvas::drop` drops the inner `Vec<DrawCommand>` + all heap-backed payloads (Path = `Vec<PathCommand>`, Image = handle, Paint = optional `Box<Shader>`). `DisplayList::drop` does the same. `PaintingBinding` is process-wide via the `impl_binding_singleton!` macro -- it lives until process exit; no explicit shutdown.

**Partial failure recovery.** No partial-failure surface inside the crate. The closest analog is `tessellate_fill` / `tessellate_stroke` returning `TessellationError` -- the caller (today, `flui-engine::wgpu::tessellator`) decides whether to skip the path or substitute a fallback. cosmic-text panics inside `Buffer::set_text` are not catch_unwind-wrapped here; if cosmic-text panics on malformed input, the panic propagates and `flui-rendering`'s pipeline catches it at the per-render-object boundary.

**Two-phase commits.** Not needed; everything is in-memory.

**FontSystem mutex contention.** `static FONT_SYSTEM: OnceLock<Mutex<FontSystem>>` in `text_layout`. The lock is held during `Buffer::set_text` + `Buffer::shape_until_scroll` calls (cosmic-text shapes synchronously, can take 1-10ms for complex text). Multiple text widgets shaping simultaneously will serialise. Documented as Outstanding refactor for cosmic-text 0.13+ per-thread `FontSystem` adoption. Not blocking this chain.

---

## 8. Security Model

`flui-painting` is a library, not a service. It does not handle credentials, secrets, or network input. Its trust boundaries are:

**Trusted inputs.**
- `DrawCommand` variants -- the closed enum is crate-defined; all 29 variants are vetted by exhaustive engine match.
- `Paint`, `Path`, `Shader`, `Image`, `ImageFilter`, `ColorFilter`, `RRect`, `Rect<Pixels>`, `Offset<Pixels>`, `Size<Pixels>`, `Point<Pixels>` -- types from `flui-types`, validated at their construction boundary.
- `Matrix4` -- 64-byte stack-allocated value type from `flui-types::geometry`.
- `TextStyle`, `InlineSpan` -- typography types from `flui-types::typography`.
- `LayerId`, `RenderId`, `ElementId` -- typed IDs from `flui-foundation` (this crate does not consume these directly; flui-rendering bridges).

**Untrusted inputs.**
- `HitRegionHandler` = `Arc<dyn Fn(&PointerEvent) + Send + Sync>` -- third-party closure. Can panic (not caught here; `flui-interaction` is responsible for sandboxing the per-event dispatch). Can run for unbounded time (no detection). Can allocate unbounded memory (OS-level OOM). Documented at the `HitRegion::new` site.
- `cosmic_text::FontSystem` font discovery -- reads system fonts directories. Untrusted at OS level (a malicious font file could trigger UB in the shaping library); cosmic-text's audit is what we rely on. Outside this crate's scope.
- `image::Image` payload bytes (via `flui-types::painting::Image`) -- raw RGBA / GPU texture handles. Trusted at the construction site (`flui-assets` is the future owner of decoding; today `Image` carries handles only).

**Capabilities.** None. The crate does not mediate authority. Hit-region callbacks run with the privileges of the host process.

**Secret handling.** Not applicable. `Paint`, `Path`, `Shader` `Debug` impls may print colour values, path commands, gradient stops -- if a third party embeds a secret in a Paint colour or Path coordinates, that's the third party's bug. Documented: "do not embed secrets in painting configuration; they will appear in Debug output and tracing spans."

**Logging rules.** `tracing` spans currently exist on `Canvas::save_layer`, `Canvas::extend_from`, `Canvas::append_display_list_at_offset`, `Canvas::finish`, `DisplayList::append`, `DisplayList::to_opacity`, `PaintingBinding::*`. Spans use `&'static str` field names and primitive values (LOC counts, sizes); no Paint/Path payloads. After the chain, more spans may be added on `Canvas::draw_*` if cheap. Documented in `## Mapping decisions`.

**Serialization.** `DisplayList` is `serde`-serializable behind the `serde` feature, with `hit_regions` skipped (the `Arc<dyn Fn>` handler is non-serializable). This is correct: serialized display lists are for devtools snapshots, not for re-execution with restored handlers.

**Plugin/user input rules.** No plugin surface -- the `DrawCommand` enum is closed. The `ClipContext` trait surface accepts user-defined `impl ClipContext` types (today: 1 prod impl + 2 test impls); the trust contract is "canvas_mut returns a `&mut Canvas` we can record into". A malicious `impl ClipContext` could spin in `canvas_mut()`, but that's a denial-of-service surface, not a confidentiality / integrity surface.

---

## 9. Data-Oriented Notes

**Hot data.** Touched per `Canvas::draw_*` call:
- `Canvas::display_list.commands` -- `Vec<DrawCommand>` push. Reallocation on capacity exhaustion. Estimated `DrawCommand` size: ~120-200 bytes due to the per-variant payload + 64-byte Matrix4. The full enum size is dominated by the largest variant (probably `DrawVertices` with multiple `Vec`s, or `DrawAtlas` with 3-4 `Vec`s).
- `Canvas::transform` -- 64-byte `Matrix4`, copied per draw call.
- `Canvas::clip_stack` -- only mutated on `clip_*` + `save`/`restore`; not per `draw_*`.
- `Canvas::save_stack` -- only mutated on `save` + `restore`; not per `draw_*`.
- `Paint` -- cloned per `draw_*` call. ~80-200 bytes depending on optional `Box<Shader>` payload.
- `Path` -- cloned per `draw_path` / `clip_path` / `draw_shadow` call. Each clone is a `Vec<PathCommand>` heap allocation.

**Cold data.**
- `DisplayList::bounds` -- written incrementally; read at frame end.
- `DisplayList::hit_regions` -- written occasionally; read during hit-test (off the per-command walk).
- `DisplayListStats` -- computed on-demand via `stats()`; not stored.
- `PaintingBinding::image_cache` HashMaps -- setup-phase; off per-command walk.
- `PaintingBinding::system_fonts` listener vec -- listener registration is rare.

**Allocation strategy.**
- `Vec<DrawCommand>` per Canvas: amortised; grows once per frame to typical size, kept across frames via `reset()`.
- `Vec<ClipShape>` per Canvas: small (typical depth 0-4).
- `Vec<CanvasState>` per Canvas: small (typical save depth 0-8).
- Per `DrawCommand`: `Paint.clone()` per `draw_*` call. Per `draw_path`/`draw_shadow`: additional `Path.clone()` (Vec<PathCommand> heap alloc). Per `clip_path`: `Box::new(Path.clone())` (heap alloc + Box indirection for variant uniformity).
- `Box<DisplayList>` inside `DrawCommand::ShaderMask` / `BackdropFilter`: child sub-list captured via closure; allocated once per `draw_shader_mask` / `draw_backdrop_filter` call.

**Forbidden allocations.**
- No `Arc::clone` inside the per-frame paint loop on a per-render-object basis (Trigger 5 of [`docs/PORT.md`](../PORT.md), forward-looking; flui-painting is not in Trigger 5's current scope but we hold the rule).
- No `HashMap<RenderId, _>` in the per-command path. (None exists today.)
- No `String::new` on the per-command path. Some commands (`DrawText`) clone `String`; this is necessary for the text payload but should be considered for `Cow<'static, str>` interning of repeated strings if a benchmark surfaces.
- No `Box<dyn Trait>` allocation per draw command on the per-frame path. (None exists today after U1/U2 deletes the `Box<dyn ShaderWarmUp>` from `PaintingBinding`.)

**Cache locality.**
- `DrawCommand` enum: discriminant + largest-variant payload. Sequential iteration over `Vec<DrawCommand>` is cache-friendly for the discriminant + first few bytes; the heap-backed payload (Path's `Vec<PathCommand>`, Image's handle, Paint's optional `Box<Shader>`) breaks locality on access.
- `Matrix4` per command: 64 bytes embedded inline. Cache-friendly when iterating commands sequentially. A future flat-bytecode representation (rejected design #10) would dedupe matrices, but that's a measured-benefit decision.

**Where `Arc`/`Mutex`/`HashMap`/`Box`/`dyn Trait` are acceptable.**
- `Arc<DisplayList>` -- for retained-layer caching (`Layer::Picture(PictureLayer { display_list: Arc<DisplayList> })`). One allocation per retained layer, long-lived. Read-only via `DisplayListCore`.
- `Arc<dyn Fn(&PointerEvent) + Send + Sync>` (HitRegionHandler) -- per hit region registered. Recording-time only.
- `HashMap<String, CachedImage>` in `ImageCache` -- setup-phase + occasional cache lookups. Off per-command path.
- `RwLock<HashMap<String, CachedImage>>` in `ImageCache` -- documented in [`docs/PORT.md`](../PORT.md) lock-decision table. Off hot path.
- `RwLock<Vec<Arc<dyn Fn() + Send + Sync>>>` in `SystemFontsNotifier` -- system font change notifications, rare.
- `OnceLock<Mutex<FontSystem>>` in `text_layout` -- cosmic-text init + per-shape lock. Off per-command path; per-text-layout-creation.
- `Box<DisplayList>` inside `DrawCommand::ShaderMask` / `BackdropFilter` -- child sub-display-list captured by closure. Cannot be inlined (variant size would balloon). Acceptable.

**Where they are forbidden.**
- `Arc<RwLock<Canvas>>` -- never. Canvas has one owner.
- `Arc<RwLock<DisplayList>>` -- never. DisplayList is consumed-once.
- `Box<dyn Drawable>` plugin trait -- never. The closed `DrawCommand` enum is the trust boundary.
- `Box<dyn ShaderWarmUp>` field on PaintingBinding -- deleted in U2.
- `Arc<Mutex<Vec<Box<dyn Fn>>>>` callback registry on Canvas -- never; hit-region handlers are `Arc<dyn Fn>` stored directly in `HitRegion` (no shared mutability).
- `Mutex<HashMap<DrawCommand, _>>` for command deduplication -- never. The painting layer does not deduplicate; that's an engine-level optimisation.

---

## 10. Error Model

```rust
#[non_exhaustive]
#[derive(Error, Debug, Clone)]
pub enum PaintingError {
    /// Decoration painting failed.
    #[error("Failed to paint decoration: {reason}")]
    PaintDecorationFailed { reason: Cow<'static, str> },

    /// Invalid decoration configuration.
    #[error("Invalid decoration: {reason}")]
    InvalidDecoration { reason: Cow<'static, str> },

    /// Gradient configuration error.
    #[error("Invalid gradient: {reason}")]
    InvalidGradient { reason: Cow<'static, str> },

    /// Text painting failed.
    #[error("Text painting failed: {reason}")]
    PaintTextFailed { reason: Cow<'static, str> },

    /// Image loading/painting failed.
    #[error("Image operation failed: {reason}")]
    PaintImageFailed { reason: Cow<'static, str> },
}

pub type Result<T> = std::result::Result<T, PaintingError>;
```

**Retryable** -- none. The crate has no retryable failures; image cache evictions are silent best-effort.

**Terminal for this frame** -- `PaintTextFailed`, `PaintImageFailed`, `PaintDecorationFailed`. The caller (typically `flui-rendering` paint context) decides whether to drop the frame, substitute a fallback, or surface to the developer.

**User-facing** -- `InvalidDecoration`, `InvalidGradient`. These signal a developer bug in widget configuration.

**Internal only** -- none today. The 5 variants are intentionally narrow.

**Security-sensitive** -- none. Variants use `Cow<'static, str>` for reasons; the reason strings are crate-internal, not user-supplied.

`anyhow::Error` is **never** returned from this crate's public API.

**Today's panic-flavoured paths:**
- `Canvas::draw_circle(.., radius < 0.0, ..)` and `Canvas::draw_shadow(.., elevation < 0.0, ..)` -- `debug_assert!` in debug builds; silent in release. **Kept** as-is for U10. Filing typed `NonNegativePixels` wrapper in Outstanding refactors (would require an `flui-types` change, out of this chain's scope).
- `Canvas::restore()` on empty save stack -- silent no-op (Flutter parity). **Kept**.
- `Canvas::finish(self)` with non-empty save stack -- `tracing::warn!(unrestored_saves = N)`. **Strengthened in U10:** add `debug_assert!(self.save_stack.is_empty(), "Canvas finished with N unrestored save() calls")` so tests catch the bug; keep `tracing::warn` for release-build observability. The `finish()` signature stays `(self) -> DisplayList`, **not** `Result<DisplayList, PaintingError>`. Rejected design ("Make finish() fallible", see Section 12) explains why.

**No new variants added in this chain** unless a real surface emerges during U10 implementation. The 5 existing variants cover the failure surface adequately. Future work may add `RecordingFinished`, `SaveRestoreImbalance`, `PathBoundsExceeded`, `InvalidGeometry` once typed wrappers (`NonNegativePixels`, `BoundedSaveDepth`) land in `flui-types`.

---

## 11. Tests Required

Each test must prove a design guarantee.

**Invariants on `Canvas`.**
- `Canvas::new()` produces an empty canvas with identity transform, empty clip stack, empty save stack, `is_empty()`, `len() == 0`.
- `draw_rect(rect, paint)` increases `len()` by 1; the resulting `DrawCommand::DrawRect` carries the active transform.
- `save()` + `draw_rect` + `restore()` produces a single `DrawCommand::DrawRect`; transform is restored.
- `save_layer(bounds, paint)` + `draw_rect` + `restore()` produces 3 commands: SaveLayer, DrawRect, RestoreLayer.
- `restore()` on empty save stack is a no-op; canvas is still recordable.
- `restore_to_count(n)` restores to exactly count `n`; `n > save_count()` is a no-op.
- `finish(self)` consumes the canvas; rebuild Canvas from a fresh `new()`.
- `reset()` clears all state; capacity is preserved.

**Invariants on `DisplayList`.**
- `DisplayList::new()` is empty; `len() == 0`; `bounds == Rect::ZERO`.
- `push(cmd)` updates bounds via union (when the command has bounds).
- `apply_transform(matrix)` re-bakes every command's transform; bounds recalculated.
- `filter(pred)` returns a new DisplayList with subset of commands; bounds recalculated; hit_regions cloned.
- `to_opacity(opacity)` returns a new DisplayList where every command's Paint has multiplied opacity; bounds preserved.
- Child sub-display-lists in `ShaderMask` / `BackdropFilter` participate in `to_opacity` recursively.
- `commands_mut` is `pub(crate)` (compile-test: external use is an error).

**Invariants on `DrawCommand`.**
- Every variant returns `Some(rect)` from `bounds()` except `ClipRect`/`ClipRRect`/`ClipPath`/`DrawColor`/`DrawPaint`/`DrawTextSpan`/`RestoreLayer` (documented).
- `kind()` partitions variants into Draw / Clip / Effect / Layer; the partition is exhaustive.
- `is_draw()` + `is_clip()` + `is_effect()` + `is_layer()` are mutually exclusive and cover every variant.
- `transform()` returns the active matrix; `transform_mut()` returns a `&mut Matrix4` for all variants.
- `apply_transform(matrix)` premultiplies the new matrix into the existing one for every variant.
- `with_opacity(0.0)` produces commands whose paint colour has alpha 0.

**Phase invariants.** None (the crate is sync; no typestate).

**Cancellation.** Not applicable.

**Retry / idempotency.**
- A `Canvas` built via the same sequence of operations produces the same `DisplayList` (deterministic).
- `to_opacity(1.0)` produces a DisplayList byte-equivalent to the original (modulo Paint Cow shape).

**Authorization.** Not applicable.

**Malformed input.**
- `draw_circle(.., NaN, ..)` in debug build: panic. In release: command is recorded with NaN; GPU rejects. Documented.
- `Rect::from_xywh(.., NaN, ..)`: rejected at `flui-types` boundary; not this crate's concern.
- Path with no commands (empty path): valid; produces an empty `DrawCommand::DrawPath`.

**Concurrency.**
- `Canvas: Send`: move across thread boundary; finalise on the other side. Compile-test via `fn assert_send<T: Send>()`.
- `Canvas: !Sync`: not promised; recording is single-threaded by design.
- `DisplayList: Send + Sync`: move + share across threads. The `Arc<DisplayList>` blanket impl confirms this is the intended pattern.
- `PaintingBinding`: process-wide singleton, `Send + Sync` via interior mutability primitives (RwLock + AtomicUsize). Compile-test confirms.
- `FontSystem` mutex: confirmed serialised access; no race possible.

**Property tests.** (Deferred class -- requires `proptest` dev-dep, filed in Outstanding refactors.)
- For any sequence of `(draw_*, save, restore, save_layer, restore)` operations, `finish()` produces a consistent DisplayList: every reachable command has bounds (if non-clip), the bounds union is monotonic, no commands lost.
- For any non-empty Canvas, `canvas.bounds()` contains the union of `cmd.bounds()` for all `cmd` in `display_list.commands()`.

**Loom tests.** Not applicable (no concurrent mutation paths in the crate after U2 deletions).

**Miri tests.** Not applicable -- `#[forbid(unsafe_code)]` is set crate-wide. Zero unsafe blocks. Miri adds no new coverage.

**Integration tests.** Existing in `tests/`:
- `canvas_composition.rs` (263 LOC) -- canvas merge/extend/append patterns. Stays.
- `canvas_scoped.rs` (306 LOC) -- with_* scoped operations. Stays.
- `canvas_transform.rs` (294 LOC) -- transform stack semantics. Stays.
- `thread_safety.rs` (270 LOC) -- Send guarantees. Stays.
- `text_layout_pipeline.rs` (394 LOC) -- cosmic-text integration. Stays.
- `rich_text_example.rs` (560 LOC) -- InlineSpan rendering. Stays.
- `tessellation_integration.rs` (100 LOC) -- lyon integration. Stays.

New integration tests landed in U8:
- `canvas_state.rs` -- extracted from canvas.rs inline tests.
- `display_list_command.rs` -- extracted from display_list.rs inline tests.
- `text_layout_detect.rs` -- extracted from text_layout.rs inline tests.
- `text_painter_basic.rs` -- extracted from text_painter.rs inline tests.

---

## 12. Rejected Designs

For each rejected design: what it was, why it was tempting, why it is wrong here.

### `Box<dyn Drawable>` plugin trait instead of closed `DrawCommand` enum

**What:** Replace the closed `DrawCommand` enum with a trait: `Box<dyn Drawable + Send + Sync>` where each "command" implements `fn lower(&self, backend: &mut WgpuBackend)`. Each command type ships with its own GPU translation.

**Why tempting:** Mirrors Flutter's "everything is an object" Dart shape. Enables third-party extension (a downstream crate could define `MyCustomDrawCommand` implementing `Drawable`).

**Why wrong:** The wgpu backend in `flui-engine` cannot lower arbitrary `dyn Drawable` to draw calls without inverting the dependency (engine depends on each plugin). The exhaustive-match contract gives compile-time coverage; the trait surface loses that. Same shape as `flui-layer::Layer` enum (see [`docs/designs/2026-05-20-mythos-flui-layer-redesign.md`](2026-05-20-mythos-flui-layer-redesign.md) Mapping decisions #1). 29 variants are the entire compositor primitive vocabulary; a 30th is a coordinated change, not a plugin extension.

### `Arc<RwLock<Canvas>>` for shared mutable recording

**What:** Wrap Canvas in `Arc<RwLock<>>` so multiple threads can record into the same canvas concurrently.

**Why tempting:** Enables async paint pipelines (a background image decoder can stream into the canvas).

**Why wrong:** Lock contention on every `draw_*` call. The single-owner `Canvas` + `Canvas: Send` value-moves give equivalent flexibility for the actual use case (background workers build their own Canvases and emit them as values; the merge happens on the main thread via `extend_from`). `Arc<RwLock<>>` is ceremony with no payback. Same shape as `flui-layer::Arc<RwLock<LayerTree>>` rejection (verdict S12 #2).

### Make `Canvas::finish()` fallible

**What:** Change `finish(self) -> DisplayList` to `finish(self) -> Result<DisplayList, PaintingError>` where unrestored saves produce `Err(PaintingError::SaveRestoreImbalance)`.

**Why tempting:** Surfaces the bug class. Honest about the structural invariant.

**Why wrong:** Massive caller-side ripple (every paint phase call site has to handle `Result`). Flutter parity is **silent finalisation** (Flutter's `PictureRecorder.endRecording()` does not return an error; it silently completes with whatever state exists). The pragmatic middle ground: keep `finish() -> DisplayList` infallible; add `debug_assert!(save_stack.is_empty())` for catch-during-tests; keep `tracing::warn!(unrestored_saves = N)` for release-build observability.

### Make every `draw_*` method fallible

**What:** Change every `draw_*(rect, &paint)` to `draw_*(rect, &paint) -> Result<&mut Self, PaintingError>` returning errors on NaN/negative geometry.

**Why tempting:** Type-encodes the validation.

**Why wrong:** Every call site has to `?` or `.unwrap()`. The honest move is **typed-non-negative wrapper at construction** (`NonNegativePixels`), which lives in `flui-types`. Filed in Outstanding refactors. The current `debug_assert!` in debug builds catches the bug; release-build silent-record-and-let-GPU-handle is acceptable.

### Paint interning at construction

**What:** Replace `Paint.clone()` per `draw_*` call with `PaintHandle(NonZeroU32)` indexing into a per-canvas interning table. Commands carry `PaintHandle` instead of `Paint`. The handle resolves to `&Paint` at GPU lowering time.

**Why tempting:** ~80-200 bytes saved per command; ~80% reduction in `DrawCommand` size. Reduces hash-walk cost on duplicate Paint detection.

**Why wrong for this chain:** Real benefit, but requires:
- `Paint: Hash + Eq` (additional trait impls; Paint contains `f32` colour which is not `Eq`).
- A per-canvas interning table (extra state on Canvas).
- Engine-side awareness of the handle resolution.
- Benchmarks showing measurable benefit on realistic workloads.

Filed in **Outstanding refactors** as "Paint-interning + flat-DrawCommand bytecode" with the caveat that measured benefit must come first. Same reasoning as `flui-layer`'s `SmallVec<[LayerId; 4]>` deferral (verdict S12 entry "SmallVec for LayerNode::children").

### Flat bytecode `Vec<u8>` instead of `Vec<DrawCommand>`

**What:** Replace `Vec<DrawCommand>` with a flat byte buffer containing opcode + payload, interpreted at GPU lowering. Like Skia's `SkRecord`.

**Why tempting:** Tighter memory layout. Per-command discriminant in 1 byte. Sequential cache access.

**Why wrong for this chain:** Requires:
- A bytecode encoder per `DrawCommand` variant.
- A bytecode decoder per variant on the engine side.
- Re-shaping `with_opacity` / `apply_transform` / `bounds` / `filter` / `map` operations to work over bytecode.
- Loss of `serde` derive ergonomics.

Filed in **Outstanding refactors** as "Flat-bytecode DisplayList representation" with the same measured-benefit gate.

### `RecordedCanvas` / `MutableCanvas` typestate distinction

**What:** Split `Canvas` into `Canvas<Recording>` and `Canvas<Finished>` via typestate; only `Canvas<Recording>` exposes `draw_*` methods; only `Canvas<Finished>` exposes `display_list()` / `bounds()` / `finish()` accessors.

**Why tempting:** Encodes the recording/finished invariant at the type level.

**Why wrong:** The consumed-once `finish(self)` shape **already enforces** the invariant. After `finish()`, the Canvas value is gone -- the type system already forbids reuse. Adding a `Canvas<Phase>` typestate would force every `&mut Canvas` parameter in the workspace (in `flui-rendering::CanvasContext`, in tests, in examples) to monomorphise on the phase, with zero compile-time benefit over today's `&mut Canvas` (which is implicitly `Canvas<Recording>` because Canvas is never `Finished` from `&mut self`). Same shape as `flui-rendering`'s `PipelineOwner<Phase>` typestate is justified there because the pipeline has 5 distinct phases with different valid operation sets; Canvas has 1 phase (Recording) + 1 terminal absorption (Finished via consume). Typestate is overkill.

### `enum_dispatch` proc-macro for the 29-variant DrawCommand operations

**What:** Use the `enum_dispatch` crate to auto-generate the per-variant pattern matches in `with_opacity`, `bounds`, `transform`, `transform_mut`, `paint`, `kind`.

**Why tempting:** Eliminates 1000+ LOC of pattern-match boilerplate. Macro generates them cleanly.

**Why wrong:** New proc-macro crate dependency for a moderate win. Hand-written `macro_rules!` `gen_command_accessors!` would give the same output with no dep. Adopted in `flui-layer`'s Mythos Step 4 (verdict S12 #5). Adopt the same pattern here in a follow-up Outstanding refactor; don't bundle with this chain.

### `async fn draw_*` for streaming display lists

**What:** Make `Canvas::draw_*` `async fn` so a paint phase can yield mid-frame.

**Why tempting:** Enables incremental paint with chunked GPU upload.

**Why wrong:** [`docs/PORT.md`](../PORT.md) Refusal trigger 3 forbids `async fn` on the render hot path. The paint phase is synchronous by design; mid-frame yields would require coordinating with `flui-scheduler`'s frame budget. Out of scope for this crate. Asset loading (where async legitimately lives) hands off via `flui-assets` and decoded images arrive as `Image` payloads through the synchronous `Canvas::draw_image` API.

### Convert `WarmUpCanvas` to a closed enum vocabulary

**What:** Instead of deleting the dead `WarmUpCanvas` trait, replace it with a closed enum `WarmUpCommand { Rect, RRect, Circle, Path }` and have shader warm-up record into a Vec.

**Why tempting:** Salvages the existing API. "Maybe the trait is dead but the concept isn't."

**Why wrong:** The shader warm-up subsystem itself is decorative (the `execute()` body literally says "in a real implementation, we'd create an offscreen canvas here"). Salvaging the API perpetuates the lie. Delete the whole subsystem in U1+U2; file "shader warm-up backed by real offscreen canvas" in Outstanding refactors so the future implementation lands on a clean slate.

### Make `DisplayListCore` `pub(crate)` and seal extensions

**What:** Demote the sealed-trait pair from public to crate-private; expose only `DisplayList` directly.

**Why tempting:** Less surface area. Caller doesn't need to import the trait.

**Why wrong:** The blanket `DisplayListCore for Arc<DisplayList>` impl is the load-bearing pattern -- `flui-layer::Layer::Picture` carries `Arc<DisplayList>` and the engine consumes it via `display_list.commands()`. Without the public trait + blanket impls, `flui-engine` would have to deref the `Arc` explicitly at every call site. The current sealed pattern is correct.

### Helper submodules `canvas/helpers.rs`, `display_list/helpers.rs`

**What:** Group utility functions in `helpers.rs` to "avoid cluttering the main module".

**Why tempting:** Quick way to extract code from a big file.

**Why wrong:** "Helper" is a naming smell. If a function is genuinely shared, it belongs on the type it manipulates or in a named submodule about its concern. Reject the name; require functional names for any extraction (`canvas/state.rs`, `canvas/transform.rs`, `display_list/command_ops.rs`). Same rejection as `flui-layer`'s S12 helpers-naming rejection.

---

## 13. Implementation Plan

Ordered. Each step lands as a reviewable commit. Each step compiles and passes tests independently. Steps are numbered to map onto the `flui-rendering` and `flui-layer` Mythos plan format for cross-reference.

### Step 1 — Delete dead surface: `WarmUpCanvas` trait

- Delete the `WarmUpCanvas` trait declaration in `crates/flui-painting/src/binding.rs` (lines ~281-293).
- Remove `WarmUpCanvas` from the `lib.rs` re-export list (line 189).
- Remove the `&mut dyn WarmUpCanvas` parameter from `ShaderWarmUp::warm_up_on_canvas` -- temporarily change the trait signature to `fn warm_up_on_canvas(&self)` (the trait body is dead anyway; U2 deletes the whole trait).
- Verify zero external callers via `grep -r "WarmUpCanvas" crates/`.

**Verifies:** `cargo build --workspace` clean; no other crate referenced `WarmUpCanvas`.

### Step 2 — Collapse `ShaderWarmUp` subsystem (decorative)

- Delete the `ShaderWarmUp` trait + `DefaultShaderWarmUp` struct + `impl ShaderWarmUp for DefaultShaderWarmUp` block in `binding.rs` (lines ~250-319).
- Delete the `shader_warm_up: Option<Box<dyn ShaderWarmUp>>` field on `PaintingBinding` (line 386).
- Delete the `PaintingBinding::with_shader_warm_up(warm_up: Box<dyn ShaderWarmUp>)` constructor variant (lines ~421-429).
- Delete the `PaintingBinding::set_shader_warm_up(warm_up: Box<dyn ShaderWarmUp>)` setter (lines ~449-451).
- Delete the warm-up execution path in `BindingBase::init_instances` (lines ~474-481).
- Remove `ShaderWarmUp` + `DefaultShaderWarmUp` from `lib.rs` re-exports (line 189).
- Update `Debug` impl to remove `has_shader_warm_up` field.
- Document the deletion in the next `## Mapping decisions` entry: "Shader warm-up subsystem deleted; track real offscreen-canvas-backed warm-up in Outstanding refactors."

**Verifies:** `cargo test -p flui-painting` green; PaintingBinding's image-cache + system-fonts paths still work; `cargo build --workspace` clean.

### Step 3 — Trim `PaintingBinding` surface

- Audit remaining `PaintingBinding` methods.
- Confirm `image_cache()`, `image_cache_mut()`, `system_fonts()`, `handle_memory_pressure()`, `handle_system_message()`, `evict()` all have real callers in `flui-app` + tests.
- No code changes here beyond the U2 deletions; this step is the audit + documentation.
- Add `tracing::instrument` spans where missing (only on `handle_memory_pressure` and `handle_system_message`).

**Verifies:** `cargo test -p flui-painting` green; `tracing` spans visible in logs when binding events fire.

### Step 4 — Split `canvas.rs` god module (3,305 LOC)

- Create `canvas/` directory.
- Move `Canvas` struct + `Default` + `Clone` + `Debug` + `AsRef<DisplayList>` + `finish` + `display_list()` + `reset` + `clear_commands` + `is_empty` / `len` / `bounds` into `canvas/mod.rs`.
- Move `CanvasState` + `ClipShape` + `save` / `restore` / `save_count` / `restore_to_count` + `save_layer` / `save_layer_alpha` / `save_layer_opacity` / `save_layer_blend` into `canvas/state.rs`.
- Move `translate` / `scale_uniform` / `scale_xy` / `rotate` / `rotate_around` / `skew` / `transform` / `set_transform` / `transform_matrix` into `canvas/transform.rs`.
- Move `clip_rect` / `clip_rrect` / `clip_path` / `*_ext` variants + `local_clip_bounds` / `device_clip_bounds` / `would_be_clipped` into `canvas/clipping.rs`.
- Move all 29 `draw_*` primitive methods into `canvas/drawing.rs`. Sub-split by variant group (shapes, text, image, gradient, effects, atlas, color, layer) is **not** done in this step; a follow-up Outstanding refactor can do that if the file is over ~2,000 LOC after the move.
- Move all 12 `with_*` scoped methods into `canvas/scoped.rs`.
- Move `extend_from` / `extend` / `merge` / `append_display_list` / `append_display_list_at_offset` into `canvas/composition.rs`.
- Move `add_hit_region` accessor onto `canvas/mod.rs` (small, ~5 LOC).
- Extract inline tests from `canvas.rs` to `tests/canvas_state.rs` (Step 8 lands them; for now keep as `#[cfg(test)] mod tests` in the new files).
- Update `lib.rs` `pub mod canvas;` to `pub mod canvas;` (re-export `canvas::Canvas`).

**Verifies:** `cargo build --workspace` clean; `cargo test -p flui-painting --lib` green; `cargo test -p flui-painting --tests` green (existing integration tests unaffected). `canvas/mod.rs` is ~200 LOC; the rest are 400-900 LOC each.

### Step 5 — Split `display_list.rs` god module (2,434 LOC)

- Create `display_list/` directory.
- Move `DisplayList` struct + `Default` + `iter` / `iter_mut` + `apply_transform` / `filter` / `map` / `to_opacity` / `clear` + `commands_mut` (**demoted to `pub(crate)`**) into `display_list/mod.rs`.
- Move `DrawCommand` enum + `CommandKind` enum into `display_list/command.rs`.
- Move the entire `impl DrawCommand` block (`with_opacity`, `bounds`, `transform`, `transform_mut`, `paint`, `has_paint`, `kind`, `is_*`, `apply_transform`) into `display_list/command_ops.rs`.
- Move `private::Sealed` module + `DisplayListCore` trait + `DisplayListExt` trait + 4 blanket impls (for `DisplayList`, `Arc<DisplayList>`, `Box<DisplayList>`, `&DisplayList`) into `display_list/sealed.rs`.
- Move `DisplayListStats` struct + `zero` / `new` + `Display` impl into `display_list/stats.rs`.
- Move `PointerEvent` + `PointerEventKind` + `HitRegion` + `HitRegionHandler` + the re-exports (`flui_types::painting::{BlendMode, Clip, ClipOp, FilterQuality, Paint, PointMode, Shader, TextureId, effects::ImageFilter, image::{ColorFilter, ImageRepeat}}`) into `display_list/hit_region.rs` + `display_list/mod.rs` re-export block.
- Extract inline tests from `display_list.rs` to `tests/display_list_command.rs` (Step 8 lands them; for now keep as `#[cfg(test)] mod tests` in the new files).
- Update `lib.rs` `pub mod display_list;` to `pub mod display_list;` (re-export the public surface).

**Verifies:** `cargo build --workspace` clean; existing 4 blanket impls still work; `cargo test -p flui-painting --tests` green. `display_list/command_ops.rs` is the largest file (~1,200 LOC after the 240-LOC `with_opacity` + 250-LOC `bounds` + accessors); the rest are 200-600 LOC each.

### Step 6 — Split `text_layout.rs` god module (1,243 LOC) + flatten `mod inner` cfg

- Create `text_layout/` directory.
- Move the `#[cfg(feature = "text")]` attribute from `mod inner { … }` inside `text_layout.rs` to the `pub mod text_layout;` declaration in `lib.rs`. Drop the `mod inner { … }` indirection; everything that was inside `inner` now lives at the module root.
- Move `detect_text_direction` + `is_rtl_char` + `is_ltr_char` into `text_layout/detect.rs`.
- Move `TextLayout` struct + `new` + `metrics` + `get_offset_for_caret` + cursor + hit_test methods into `text_layout/layout.rs`.
- Move `TextLayoutResult` + `LineInfo` + their accessor methods into `text_layout/line_info.rs`.
- Move `measure_text` + `measure_inline_span` + `style_to_attrs` helpers into `text_layout/measure.rs`.
- Keep the `static FONT_SYSTEM: OnceLock<Mutex<FontSystem>>` and `font_system()` accessor in `text_layout/mod.rs` (process-wide singleton; documented in `## Thread safety`).
- Extract inline tests from `text_layout.rs` to `tests/text_layout_detect.rs` + `tests/text_layout_layout.rs` (Step 8 lands them).
- Update `lib.rs` re-exports to point at `text_layout::detect::detect_text_direction` etc.

**Verifies:** `cargo build --workspace --features text` clean; `cargo build --workspace --no-default-features` clean (text feature genuinely off); `cargo test -p flui-painting --features text` green.

### Step 7 — Split `text_painter.rs` god module (990 LOC)

- Create `text_painter/` directory.
- Move `TextPainter` struct + `paint` + `DEFAULT_FONT_SIZE` constant into `text_painter/mod.rs`.
- Move `TextBaseline` enum + baseline math into `text_painter/baseline.rs`.
- Move paint integration (canvas drawing, glyph emission) into `text_painter/paint.rs`.
- Move measurement-side helpers into `text_painter/measure.rs` if separable; otherwise keep in `mod.rs`.
- Extract inline tests to `tests/text_painter_basic.rs`.
- Update `lib.rs` re-exports.

**Verifies:** `cargo build --workspace --features text` clean; `cargo test -p flui-painting --features text` green.

### Step 8 — Extract inline tests to `tests/` integration

- For each new submodule from U4-U7, extract the `#[cfg(test)] mod tests { … }` block to a corresponding file in `crates/flui-painting/tests/`.
- New integration test files:
  - `tests/canvas_state.rs` (extracted from `canvas/state.rs`)
  - `tests/canvas_transform_unit.rs` (extracted from `canvas/transform.rs`; distinct from existing `canvas_transform.rs` integration test which is broader)
  - `tests/canvas_drawing.rs` (extracted from `canvas/drawing.rs`)
  - `tests/canvas_scoped_unit.rs` (extracted from `canvas/scoped.rs`)
  - `tests/canvas_composition_unit.rs` (extracted from `canvas/composition.rs`)
  - `tests/display_list_command.rs` (extracted from `display_list/command_ops.rs`)
  - `tests/display_list_sealed.rs` (extracted from `display_list/sealed.rs`)
  - `tests/display_list_stats.rs` (extracted from `display_list/stats.rs`)
  - `tests/display_list_hit_region.rs` (extracted from `display_list/hit_region.rs`)
  - `tests/text_layout_detect.rs` (extracted from `text_layout/detect.rs`)
  - `tests/text_layout_layout.rs` (extracted from `text_layout/layout.rs`)
  - `tests/text_painter_basic.rs` (extracted from `text_painter/mod.rs`)
- Each new test file uses `use flui_painting::*;` or the specific imports needed.
- Update `tests/mod.rs` to be either a single-file or none (integration tests in cargo are independent files).

**Verifies:** test count stays the same or grows; `cargo test -p flui-painting --tests` green; each new file is focused on one concern.

### Step 9 — Allocation hot-path audit (document; no implementation)

- Profile a `Canvas` with 1000 `draw_rect` calls (synthetic workload).
- Identify the per-call allocation pattern: `paint.clone()` (~80-200 bytes) + `Vec::push` (occasional reallocation) + per-command `Matrix4` baking (64 bytes inline).
- Identify `draw_path` / `clip_path` / `draw_shadow` calls with additional `Path::clone()` (`Vec<PathCommand>` heap alloc).
- Identify `clip_path`'s additional `Box::new(Path::clone())` indirection.
- Document the findings in `crates/flui-painting/ARCHITECTURE.md` `## Friction log`.
- File Outstanding refactors:
  - "Paint interning at construction" (requires `Paint: Hash + Eq`, per-canvas table, engine handle resolution).
  - "Flat-bytecode DisplayList representation" (requires encoder + decoder + operation re-shape).
  - "Path-Cow on draw_path / clip_path / draw_shadow" (requires `Path: Clone-on-Write` semantics).
  - "Per-thread cosmic-text `FontSystem`" (requires cosmic-text 0.13+ adoption).
- No code changes in this step. Pure documentation + Outstanding refactor filing.

**Verifies:** `crates/flui-painting/ARCHITECTURE.md` `## Friction log` + `## Outstanding refactors` updated; no behaviour change.

### Step 10 — Error model + commands_mut demotion

- Demote `DisplayList::commands_mut` from `pub` to `pub(crate)`. Audit callers; the only existing caller is internal `apply_transform`.
- Strengthen the save/restore imbalance check in `Canvas::finish(self)`:
  - Add `debug_assert!(self.save_stack.is_empty(), "Canvas finished with {} unrestored save() calls", self.save_stack.len());`
  - Keep `tracing::warn!(unrestored_saves = self.save_stack.len(), "Canvas finished with unrestored save() calls");` for release-build observability.
- Audit `debug_assert!` sites in `Canvas::draw_circle` + `Canvas::draw_shadow` + `Canvas::draw_point`. Documented; no change.
- Audit `PaintingError` variants. Confirm 5 variants cover the failure surface. **No new variants added in this chain** -- file `RecordingFinished` / `SaveRestoreImbalance` / `InvalidGeometry` / `PathBoundsExceeded` in Outstanding refactors as the typed-wrapper companion work in `flui-types` lands.

**Verifies:** `cargo test -p flui-painting` green; `commands_mut` no longer callable from outside the crate (compile-test); existing callers in `flui-rendering`/`flui-engine`/`flui-layer` do not use `commands_mut`.

### Step 11 — Dependency + feature audit

- Run `cargo tree -p flui-painting -e features` and `cargo tree -d -p flui-painting`. Document the dependency tree in `## Mapping decisions`.
- Confirm `cosmic-text 0.12` is the current pin; verify `cosmic-text 0.13+` adoption is filed as Outstanding (per-thread FontSystem).
- Confirm `lyon 1.0` is the current pin; verify no unused lyon sub-features.
- Verify `parking_lot` is used only by `binding.rs` (ImageCache RwLocks + SystemFontsNotifier listener vec) and `text_layout` (FontSystem Mutex). Document the lock decisions.
- Verify the `text` feature genuinely turns text_layout + text_painter off; verify the `tessellation` feature genuinely turns tessellation off. Run `cargo build --no-default-features -p flui-painting` to confirm.
- Verify the `serde` feature compiles clean.
- Delete any dead feature flag or unused `#[cfg(...)]` block surfaced by the audit.

**Verifies:** `cargo build --no-default-features -p flui-painting` clean; `cargo build --all-features -p flui-painting` clean; `cargo tree` snapshot recorded.

### Step 12 — Per-crate `ARCHITECTURE.md` template + PORT.md Index flip

- Create `crates/flui-painting/ARCHITECTURE.md` at crate root per the `docs/PORT.md` template:
  - `## Flutter source mapping` (table: `painting/painting.dart` / `painting/canvas.dart` / `painting/clip.dart` / `painting/binding.dart` / `painting/image_cache.dart` / `painting/shader_warm_up.dart` / Skia `SkCanvas` etc. → `flui-painting/src/*`)
  - `## Mapping decisions` (Accepted trade-offs for: closed `DrawCommand` enum vs `Box<dyn Drawable>`, sealed `DisplayListCore`/`DisplayListExt` pair, `WarmUpCanvas`/`ShaderWarmUp` deletion, `ClipContext` retention as cross-crate seam, `finish(self)` infallibility, `Paint::clone()` per draw call deferred to Outstanding)
  - `## Thread safety` (table: ImageCache RwLocks, SystemFontsNotifier listener vec, FontSystem Mutex, Canvas single-owner, DisplayList consumed-once)
  - `## Friction log` (companion `docs/MIGRATION.md` obsolete; `text_layout::mod inner` cfg pre-flattening; allocation hot-path documented from U9)
  - `## Outstanding refactors` (Paint interning, flat bytecode, Path-Cow, per-thread FontSystem, typed `NonNegativePixels`, `enum_dispatch`-style macro for DrawCommand operations, shader warm-up real implementation, doctest fix sweep if needed)
- Link companion docs from `## Mapping decisions` / `## Outstanding refactors`:
  - `crates/flui-painting/docs/ARCHITECTURE.md` (architecture deep-dive; pre-template, kept as companion)
  - `crates/flui-painting/docs/PERFORMANCE.md` (perf guidance; kept as companion)
  - `crates/flui-painting/docs/MIGRATION.md` (obsolete migration notes; stubbed or deleted)
  - `crates/flui-painting/docs/README.md` (Q&A; kept as companion)
- Update `docs/PORT.md` `## Index` table:
  - Flip `flui-painting` row from "`crates/flui-painting/docs/ARCHITECTURE.md` (pre-template)" to "[`flui-painting`](../crates/flui-painting/ARCHITECTURE.md) | Templated 2026-05-20 (Mythos chain) | Active".

**Verifies:** `crates/flui-painting/ARCHITECTURE.md` exists at crate root; companion docs intact; `docs/PORT.md` index reflects the flip.

### Step 13 — Extend `scripts/port-check.sh`

- Add `crates/flui-painting/src/` to Trigger 1 path glob (`RwLock<Box<dyn ...>>` -- forward-looking; should match nothing).
- Add `crates/flui-painting/src/` to Trigger 2 path glob (`(RwLock|Mutex|RefCell|Cell|UnsafeCell)<Box<dyn ...>>` -- should match nothing).
- Add `crates/flui-painting/src/` to Trigger 3 path glob (`async fn build|layout|paint|perform_layout|composite|render|fire_composition_callbacks` -- should match nothing).
- Update `docs/PORT.md` Trigger 1, 2, 3 entries with "Scope extended in Mythos Step 13 of the `flui-painting` chain to cover `crates/flui-painting/src/`".
- Run `bash scripts/port-check.sh -v` and verify all 6 triggers stay clean.

**Verifies:** `bash scripts/port-check.sh -v` exits 0; the methodology now covers `flui-painting`.

### Step 14 — Final verification + plan flip

- Run `cargo test --workspace` -- all green.
- Run `cargo clippy --workspace -- -D warnings` -- clean. Address any new clippy warnings introduced by the refactor (deletions sometimes uncover dead-code lints on adjacent imports).
- Run `bash scripts/port-check.sh -v` -- all 6 triggers clean.
- Run `cargo build --no-default-features --workspace` -- text/tessellation features off; flui-painting still builds.
- Run `cargo build --all-features --workspace` -- everything on; flui-painting still builds.
- Flip the plan `docs/plans/2026-05-20-004-feat-flui-painting-mythos-redesign-plan.md` status to "completed".
- Confirm `crates/flui-painting/ARCHITECTURE.md` + `docs/PORT.md` index + `scripts/port-check.sh` are consistent.

**Verifies:** the workspace is fully green; the refactor is mergeable; the methodology is consistently extended.

---

## Self-check

- **Did I start from data, not traits?** Yes. `Canvas`/`DisplayList`/`DrawCommand` are the spine. There is no `Box<dyn Drawable>` plugin trait -- the closed enum is the trust boundary, same shape as `flui-layer::Layer`.
- **Did every module earn its existence?** Two traits (`WarmUpCanvas`, `ShaderWarmUp`) flagged for deletion. Two god modules (`canvas.rs`, `display_list.rs`) flagged for concern-based split. Two more god modules (`text_layout.rs`, `text_painter.rs`) flagged for pipeline-phase split. The `mod inner` cfg indirection in `text_layout.rs` flagged for flattening.
- **Did I identify the state owner?** Yes. `Canvas` owns inner `DisplayList` during recording; `Canvas::finish(self) -> DisplayList` consumes the canvas; the consumed DisplayList is moved into engine for GPU lowering or wrapped by flui-layer's Layer::Picture/Layer::Canvas. No `Arc<RwLock<>>` anywhere in the production path.
- **Did I define cancellation behavior?** Yes. The crate is sync; cancellation is not applicable. Dropping a `Canvas` mid-recording drops the inner state without side effects.
- **Did I define trust boundaries?** Yes. The closed `DrawCommand` enum is the trust contract with `flui-engine`'s GPU lowering. Third-party `Paint`, `Path`, `Shader`, `Image`, `ImageFilter`, `ColorFilter` configs come from `flui-types` and are validated at their construction boundary. `ClipContext` is a 1-prod-impl cross-crate seam (`CanvasContext` in flui-rendering); no third-party plugin surface.
- **Did I avoid fake extensibility?** Yes. `WarmUpCanvas` (0 impls) and `ShaderWarmUp` (1 stub impl, decorative `execute()`) are slated for deletion. The closed enum keeps the draw-command vocabulary explicit.
- **Did I avoid Quick Win architecture?** The plan executes 14 steps including dead-code deletion (`WarmUpCanvas`, `ShaderWarmUp`, decorative `with_shader_warm_up`/`set_shader_warm_up`/field on PaintingBinding), god-module splits (4 large files), inline-test extraction (cleaner crate test structure), error model strengthening (debug_assert + tracing on save/restore imbalance + commands_mut demotion), dependency audit, ARCHITECTURE.md graft, methodology extension. Quick-wins resisted:
  - **Did NOT** attempt Paint-interning without measured benefit (filed in Outstanding).
  - **Did NOT** attempt flat-bytecode DisplayList without measured benefit (filed in Outstanding).
  - **Did NOT** demote `ClipContext` to a private function despite 1 impl (legitimate cross-crate seam; documented).
  - **Did NOT** introduce typestate on `Canvas` (consumed-once finish already enforces the invariant).
  - **Did NOT** make `finish()` fallible (Flutter parity + caller-side ripple; debug_assert + tracing::warn covers the bug class).
- **Did I encode invariants in types where possible?** Yes. `Canvas::finish(self)` consumes the canvas; the type system forbids reuse. `DrawCommand` `#[non_exhaustive]` future-proofs internal additions. `DisplayList::commands_mut` demoted to `pub(crate)` in U10. `#[forbid(unsafe_code)]` stays. Net unsafe delta: 0.
- **Did I reject bad alternatives?** Eleven rejected designs documented in Section 12.
- **Could a Rust developer implement this design without guessing?** Yes, given the implementation plan in Section 13 and the type sketches in Section 3.
