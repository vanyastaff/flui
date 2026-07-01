# `RenderBackdropFilter` / `RenderShaderMask` — plan (oracle-verified)

Core.2 catalog item. Oracle: `.flutter/flutter-master/packages/flutter/lib/src/rendering/proxy_box.dart`, `RenderShaderMask` (`:1128-1195`) and `RenderBackdropFilter` (`:1201-1364`), read in full, plus their supporting types `rendering/image_filter_config.dart` (275 lines, read in full) and `rendering/layer.dart`'s `ShaderMaskLayer`/`BackdropKey`/`BackdropFilterLayer` (`:2218-2404`).

## Headline verdict, up front

1. **The `flui-rendering` paint-pipeline wiring the prior scoping pass identified as the sole gap is real, well-scoped, and buildable exactly as described** — `PaintCx` needs two new closure-scoped methods (`with_shader_mask`/`with_backdrop_filter`) that extend the *already-proven* clip-scope mechanism (`FragmentClip`/`clip_layer()`), not a new architectural capability. §2 traces this precisely against live source.
2. **A genuine, confirmed, previously-unflagged gap: the wgpu engine's `Layer::ShaderMask` rendering path never actually applies the shader.** `ShaderMaskLayer::render()`/`cleanup()` (`crates/flui-engine/src/wgpu/layer_render.rs:334-352`) push a `save_layer(Paint::default())`/clip/`restore_layer()` triple that reads **only** `self.bounds()` — `self.shader()` and `self.blend_mode()` are never called anywhere in that path, and `WgpuPainter::save_layer`/`restore_layer` (`crates/flui-engine/src/wgpu/painter/layer.rs:325-351,513-533`) have zero shader/mask vocabulary (only opacity/tint/blend-mode/color-filter/image-filter). This directly qualifies the prior pass's "pushes a real save-layer + clip" finding: true, but that save-layer+clip is functionally an inert `ClipRect` — the mask never visually applies. A **separate, fully-working** shader-mask GPU pipeline already exists (`Canvas::draw_shader_mask` → `DrawCommand::ShaderMask` → `Backend::render_shader_mask`, `crates/flui-painting/src/canvas/drawing.rs:442-461`, `crates/flui-engine/src/wgpu/backend.rs:773-870+`) but it is **architecturally incompatible** with `PaintCx`'s deferred-child model (§2.5) and cannot be reused as-is. This is reclassified below, not silently absorbed into "done."
3. **`BackdropFilter`'s engine path is real but blur-only.** `renderer.rs`'s `handle_backdrop_filter` (`:1517-1600+`) does genuine Dual Kawase blur — but only for `ImageFilter::Blur`; every other `ImageFilter` variant hits a `tracing::warn!("Backdrop filter type not supported for GPU blur, rendering children only")` fallback that silently drops the filter. Confirmed non-blocking for the dominant real-world case (Flutter's `ImageFilter.blur`) but must be documented, not assumed away.
4. **The oracle itself has grown past the task's assumed field set.** Current `.flutter/` master's `RenderBackdropFilter` takes `filterConfig: ImageFilterConfig` (a layout-aware filter *blueprint*, resolved at paint time against a `Rect`) plus a `backdropKey: BackdropKey?` for `BackdropGroup`-shared backdrop sampling — neither of which has *any* FLUI-side backing (`flui-layer`'s `BackdropFilterLayer` has no `backdrop_key` field at all, confirmed by reading the struct in full; `flui_types::painting::ImageFilter` has no bounded/tile-mode blur variant). §1.3 scopes this down explicitly to the classic, stable surface (`filter: ImageFilter`, `blendMode`, `enabled`) rather than transcribing the newer surface into dead plumbing.
5. **Struct shape: two independent, non-generic structs, not a generic pair** (§3) — the field sets and gating logic diverge too much to share a body worth deduplicating.
6. **None of this is a blocking gap for the render-object-level task itself.** The `flui-rendering` wiring is real, valuable, and harness-testable at the structural/field level today (`run.structure()` already lists `"ShaderMask"`/`"BackdropFilter"` as layer-kind names, zero new harness facility needed — §2.6). What must NOT be claimed is "ShaderMask now visually renders" — that half is a `flui-engine` follow-up, sketched but out of scope, in §6.

## 1. Oracle — `rendering/proxy_box.dart` (line-cited)

### 1.1 `RenderShaderMask` (`:1128-1195`)

- Constructor (`:1130-1136`): `RenderBox? child`, `required ShaderCallback shaderCallback`, `BlendMode blendMode = BlendMode.modulate` — **note the default is `modulate`, not `srcOver`** (contrast with BackdropFilter below; a naive port copying one default across both structs gets this wrong).
- `shaderCallback`/`blendMode` setters (`:1150-1172`): plain guard-and-set, `markNeedsPaint()` only. TODO comment at `:1148-1149` ("use the delegate pattern... to avoid spurious repaints when the ShaderCallback changes identity") is Flutter's own acknowledged wart, not something to "fix" here — `ShaderCallback` is a plain closure type with no identity-comparison story in either language.
- `alwaysNeedsCompositing => child != null` (`:1174-1175`) — **data-dependent**, not an unconditional `true`. See §2.7 (contrast with `RenderPhysicalModel`, where the same conceptual mechanism was confirmed dead).
- `paint` (`:1177-1194`):
  ```dart
  if (child != null) {
    layer ??= ShaderMaskLayer();
    layer!..shader = _shaderCallback(Offset.zero & size)
          ..maskRect = offset & size
          ..blendMode = _blendMode;
    context.pushLayer(layer!, super.paint, offset);
  } else {
    layer = null;
  }
  ```
  **The rect passed to the shader callback (`Offset.zero & size`, LOCAL/self-origin) is NOT the same rect stored as the layer's `maskRect` (`offset & size`, GLOBAL/parent-space)** — confirmed by reading `ShaderMaskLayer`'s own doc (`layer.dart:2249-2252`, "shader is only rendered inside `maskRect`, using the top-left of the rectangle as its origin"). §2.4 shows this maps onto FLUI's existing local-shape/origin-shift convention "for free."
  `super.paint` is `RenderProxyBoxMixin.paint` (`:129-135` of the same file) = `context.paintChild(child, offset)` — RenderShaderMask draws **nothing of its own**, ever.
- **No `debugFillProperties` override at all** — grepped the full class body (`:1128-1195`), zero hits. Unlike `RenderPhysicalModel`/`RenderClip`, Flutter's own `RenderShaderMask` surfaces neither `shader` nor `blendMode` in its diagnostics tree. §4.2 flags this as a deliberate FLUI-side improvement to make, not an oracle bug to reproduce.
- No `enabled` concept, no clip-behavior concept, no hit-test override (uses `RenderProxyBoxMixin.hitTestChildren`, §1.4).

### 1.2 `RenderBackdropFilter` (`:1201-1364`)

- Constructor (`:1208-1227`): `RenderBox? child`, `ui.ImageFilter? filter`, `ImageFilterConfig? filterConfig`, `BlendMode blendMode = BlendMode.srcOver` (default **differs from ShaderMask's `modulate`**), `bool enabled = true`, `BackdropKey? backdropKey`. Asserts exactly one of `filter`/`filterConfig` is non-null (`:1215-1222`); internally normalizes to `_filterConfig = filterConfig ?? ImageFilterConfig(filter!)`.
- `filter` getter/setter (`:1259-1273`): **`@Deprecated('Use filterConfig instead. This feature was deprecated after v3.40.0-1.0.pre.')`** — confirms `filterConfig`/`ImageFilterConfig` is the *current* stable surface, not a bleeding-edge experiment; `filter` is the legacy shim, kept for source compat only.
- `filterConfig`/`blendMode`/`enabled`/`backdropKey` setters (`:1289-1323`): all plain guard-and-set + `markNeedsPaint()`.
- `alwaysNeedsCompositing => child != null` (`:1325-1326`) — same data-dependent pattern as ShaderMask.
- `paint` (`:1328-1353`), the two-gate recipe (read precisely — these are **independent** gates, not one combined condition):
  ```dart
  if (!_enabled) { super.paint(context, offset); return; }   // gate 1: bypasses the filter ENTIRELY
  final effectiveFilter = _filterConfig.resolve(ImageFilterContext(bounds: offset & size));
  if (child != null) {                                        // gate 2: only reachable when enabled
    layer ??= BackdropFilterLayer();
    layer!.filter = effectiveFilter; layer!.blendMode = _blendMode; layer!.backdropKey = _backdropKey;
    context.pushLayer(layer!, super.paint, offset);
  } else {
    layer = null;                                             // nothing at all drawn
  }
  ```
  `enabled = false` → plain `RenderProxyBoxMixin.paint` (child painted directly if present, nothing if absent) — the filter machinery (including `_filterConfig.resolve`, which never runs) is fully bypassed. `enabled = true` + no child → **nothing at all is painted**, matching the `RenderPhysicalModel` "nothing at all" convention this repo already ported once (physical-model plan §4.9-ish precedent).
- `debugFillProperties` (`:1355-1363`): adds `filterConfig`, `blendMode`, `enabled` (a `FlagProperty`). **Asymmetric with ShaderMask's total absence of diagnostics** (§1.1) — a real oracle inconsistency, not a copy-paste error to "fix" into symmetry by removing BackdropFilter's diagnostics; if anything, the FLUI-side improvement runs the other direction (§4.2).

### 1.3 The oracle has grown past the task's assumed shape — scope this down explicitly

`ImageFilterConfig` (`rendering/image_filter_config.dart`, 275 lines, read in full) is a **framework-level filter blueprint** resolved at paint time against an `ImageFilterContext(bounds)` — its `ImageFilterConfig.blur({sigmaX, sigmaY, tileMode, bounded})` constructor's `bounded: true` mode ("bounded blur," the iOS frosted-glass look: samples only within the object's own bounds, normalizing at the edges) is genuinely bounds-dependent and has **no FLUI equivalent** — `flui_types::painting::ImageFilter::Blur{sigma_x, sigma_y}` (`crates/flui-types/src/painting/effects.rs:564-573`) has no `bounded`/`tile_mode` fields at all. `ImageFilterConfig.compose` similarly has no FLUI type to land on.

`BackdropKey`/`BackdropGroup` (`rendering/layer.dart:2315-2404`, `widgets/basic.dart:471`) let multiple `BackdropFilter.grouped` widgets under one `BackdropGroup` **share** one backdrop sample (an engine-level batching optimization, `backdropId` passed to `builder.pushBackdropFilter`) — confirmed **zero FLUI backing**: `crates/flui-layer/src/layer/backdrop_filter.rs`'s `BackdropFilterLayer` struct (read in full, `:45-92`) has exactly three fields (`filter: ImageFilter`, `blend_mode: BlendMode`, `bounds: Rect<Pixels>`) — no `backdrop_key`. Adding a `backdrop_key` field at the render-object layer with no engine consumer at all would be dead plumbing, the exact anti-pattern AGENTS.md's Definition of Done warns against.

**Decision: target the classic, still-stable surface** — `RenderBackdropFilter{filter: ImageFilter, blend_mode: BlendMode, enabled: bool}`, `RenderShaderMask{shader_callback, blend_mode: BlendMode}` — matching what the prior scoping pass assumed, and explicitly deferring `ImageFilterConfig`'s bounded-blur/compose modes and `BackdropKey`/`BackdropGroup` (§6) rather than silently building either a partial or a dead-code version of the newer surface.

### 1.4 `RenderProxyBoxMixin` defaults (`:65-133`) — both classes' inherited contract

`performLayout` (`:110-114`): `size = child?.layout(constraints, parentUsesSize: true)?.size ?? computeSizeForNoChild(constraints)`, where `computeSizeForNoChild = constraints.smallest` (`:117-119`) — plain proxy sizing, matching `RenderClip<S>`/`RenderPhysicalModelBase<C>`'s already-ported `Single`-arity pattern exactly. Intrinsics (`:73-96`) forward to child or `0.0`. `hitTestChildren` (`:127`): `child?.hitTest(result, position: position) ?? false` — plain forward, **no clip/shape gate at all** (contrast with `RenderClip`/`RenderPhysicalModel`, which DO gate hit-test on their shape). `applyPaintTransform` is a no-op (`:129`).

## 2. FLUI building blocks — verified against live source, not memory

### 2.1 `flui-layer` layer types (read in full)

`BackdropFilterLayer` (`crates/flui-layer/src/layer/backdrop_filter.rs:45-92`): `filter: ImageFilter`, `blend_mode: BlendMode`, `bounds: Rect<Pixels>`; constructor `new(filter, blend_mode, bounds)`; getters + `set_bounds`. `ShaderMaskLayer` (`crates/flui-layer/src/layer/shader_mask.rs:49-96`): `shader: Shader`, `blend_mode: BlendMode`, `bounds: Rect<Pixels>`; same shape. Both wrapped by `Layer::ShaderMask(ShaderMaskLayer)` / `Layer::BackdropFilter(BackdropFilterLayer)` (`crates/flui-layer/src/layer/mod.rs:248,251`), with `bounds()` accessors already wired at `:284-285`, a `kind_name()` match arm already present at `:387-388` (`"ShaderMask"`/`"BackdropFilter"` — the exact strings `run.structure()` will surface, §2.6), and typed downcast accessors already generated at `:457-458` (`as_shader_mask`/`as_shader_mask_mut`, `as_backdrop_filter`/`as_backdrop_filter_mut`) via whatever macro backs that block.

### 2.2 `flui_types::painting` — confirmed sufficient for the classic surface

`ImageFilter` (`crates/flui-types/src/painting/effects.rs:564-626`): `Blur{sigma_x, sigma_y}`, `Dilate{radius}`, `Erode{radius}`, `Matrix(ColorMatrix)`, `ColorAdjust(ColorAdjustment)`, `Compose(Vec<ImageFilter>)`, plus a debug-only `OverflowIndicator`. `ImageFilter::blur(sigma)` convenience constructor exists (`:632-637`). `Shader` (`crates/flui-types/src/painting/shader.rs:30-87`): `LinearGradient`/`RadialGradient`/`SweepGradient`/`Solid`/`Image` variants, `#[non_exhaustive]`, derives `Clone, PartialEq`. `BlendMode` (`blend_mode.rs:3`): `#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]` — confirms `BlendMode::Modulate` and `BlendMode::SrcOver` both exist (`:22,75`). All three types are already re-exported at `flui_types::painting::{ImageFilter, BlendMode, Shader}` (`crates/flui-types/src/painting/mod.rs:20,28-29,33`) — no new type needed, no Cargo dependency changes needed anywhere in this plan (`flui-rendering` already imports `flui_types::painting::Clip`; `flui-layer` is already a full dependency of `flui-rendering`, confirmed by paint.rs's existing `use flui_layer::{...}` import).

### 2.3 `PaintCx` / `FragmentClip` — the exact shape to extend (read in full)

`crates/flui-rendering/src/context/paint_cx.rs` (653 lines, read in full). `FragmentClip` (`:101-124`) currently has `Rect{rect, behavior}`, `RRect{rrect, behavior}`, `Path{path: Box<Path>, behavior}` — always boxed at the `FragmentOp::Push(Box<FragmentClip>)` call site (`:82`), so a larger enum body (adding `Shader`/`ImageFilter` payloads) does not bloat `FragmentOp` itself. `with_clip_rect`/`with_clip_rrect`/`with_clip_path` (`:353-391`) are the exact template: push a scope via `self.rec.push_scope(FragmentClip::Variant{..})`, run the caller's closure, `self.rec.pop_scope()` — `push_scope`/`pop_scope` (`:220-242`) seal any open canvas run and emit balanced `FragmentOp::Push`/`Pop`, with a `debug_assert` scope-counter that makes an unbalanced closure a loud debug failure rather than a silently malformed tree (`:236-241,255-259`). This is a closure-scoped API by construction — there is no way to misuse it from safe code.

**`FragmentClip` is `pub(crate)`, zero cross-crate consumers, confirmed by its own doc comment** (`crates/flui-rendering/src/context/mod.rs:83-86`: *"FragmentClip has ZERO cross-crate consumers... not re-exported as `pub`"*) and a grep across the whole workspace turning up exactly 3 files (`paint_cx.rs`, `context/mod.rs`, `pipeline/owner/paint.rs`).

**Naming call, made explicitly**: once this enum carries `ShaderMask`/`BackdropFilter` variants, the name `FragmentClip` becomes semantically wrong — neither variant is a clip (backing this up, oracle's `RenderShaderMask`/`RenderBackdropFilter` have no clip-behavior concept at all, §1). Given the confirmed 3-file, `pub(crate)`-only fan-out, **recommend a mechanical rename to `FragmentScope`** (and `clip_layer()` → `scope_layer()` in paint.rs, §2.4) as part of this change — low-risk, semantically honest, and consistent with this repo's own established practice of making this kind of naming call explicit rather than silently overloading a word (mirroring the physical-model plan's `PhysicalClipSource`-not-`ClipGeometry` call). This is a cosmetic, not load-bearing, recommendation — a reviewer preferring to keep `FragmentClip` with an expanded doc comment loses nothing functionally.

### 2.4 `pipeline/owner/paint.rs` — how a scope becomes a `Layer` (read in full, 485 lines)

`clip_layer(clip: FragmentClip, origin: Offset) -> Layer` (`:466-484`) is the mapping function the `FragmentOp::Push(clip)` arm calls (`:270`, `composer.push_layer(clip_layer(*clip, origin))`). Its documented convention (`:461-465`): clip shapes are recorded in the node's LOCAL coordinates; `clip_layer` shifts them by the accumulated `origin` before constructing the `Layer` (`rect.translate_offset(origin)`, `rrect.translate_offset(origin)`, `path.translate(origin)`) — "Always a real clip layer today... correctness is identical either way, so the recording API does not expose the choice" (i.e., no `needs_compositing`-gated elision exists yet anywhere in this composer; extending it with two more always-real-layer variants is fully consistent with the existing convention, not a new one).

**This mechanism reproduces oracle's local-callback/global-storage split "for free":** `RenderShaderMask`'s paint body computes the shader by calling `shader_callback` with the LOCAL rect (`Rect::from_origin_size(Point::ZERO, ctx.size())`, matching oracle's `Offset.zero & size`), and stores that SAME local rect in the new `FragmentClip::ShaderMask{shader, blend_mode, bounds}` variant; `clip_layer`'s existing `.translate_offset(origin)` step then shifts it to global space when building `ShaderMaskLayer::new(shader, blend_mode, bounds_shifted)` — reproducing oracle's `maskRect = offset & size` exactly, via the identical mechanism already proven for `FragmentClip::Rect`. No new coordinate-handling code is needed beyond adding the two match arms.

New `PaintCx` methods (generic over `A: Arity`, matching where `with_clip_rect` etc. already live, `:414`):
```rust
pub fn with_shader_mask(&mut self, shader: Shader, blend_mode: BlendMode, f: impl FnOnce(&mut Self)) {
    let bounds = Rect::from_origin_size(Point::ZERO, self.size); // self.size already exists (`:286,334`)
    self.rec.push_scope(FragmentClip::ShaderMask { shader, blend_mode, bounds });
    f(self);
    self.rec.pop_scope();
}
pub fn with_backdrop_filter(&mut self, filter: ImageFilter, blend_mode: BlendMode, f: impl FnOnce(&mut Self)) {
    let bounds = Rect::from_origin_size(Point::ZERO, self.size);
    self.rec.push_scope(FragmentClip::BackdropFilter { filter, blend_mode, bounds });
    f(self);
    self.rec.pop_scope();
}
```
(Needs `Point` added to `paint_cx.rs`'s `use flui_types::{...}` import list — currently imports `Matrix4, Offset, Pixels, Rect, Size, painting::Clip` but not `Point`, `:48`.) `clip_layer`/`scope_layer` in `paint.rs` gains two matching arms producing `Layer::ShaderMask(ShaderMaskLayer::new(shader, blend_mode, bounds.translate_offset(origin)))` and `Layer::BackdropFilter(BackdropFilterLayer::new(filter, blend_mode, bounds.translate_offset(origin)))`, plus `ShaderMaskLayer, BackdropFilterLayer` added to the existing `use flui_layer::{...}` import at the top of `paint.rs` (`:4-7`; both types are already exported from `flui_layer`'s crate root, `crates/flui-layer/src/layer/mod.rs:104,125`).

### 2.5 The confirmed engine gap — `Layer::ShaderMask` never applies the shader (read in full)

`LayerRender<ShaderMaskLayer>` (`crates/flui-engine/src/wgpu/layer_render.rs:334-352`):
```rust
fn render(&self, renderer: &mut R) {
    let paint = flui_painting::Paint::default();               // no shader, no blend attached
    renderer.save_layer(Some(self.bounds()), &paint, &Matrix4::IDENTITY);
    renderer.push_clip_rect(&self.bounds(), Clip::AntiAlias);
}
fn cleanup(&self, renderer: &mut R) {
    renderer.pop_clip();
    renderer.restore_layer(&Matrix4::IDENTITY);
}
```
`self.shader()`/`self.blend_mode()` (the `ShaderMaskLayer`'s own fields) are **never read** by this impl. Tracing the call chain confirms there is nowhere else they could be consumed: `Backend::save_layer`/`restore_layer` (`crates/flui-engine/src/wgpu/backend.rs:1319-1325`) forward straight to `WgpuPainter::save_layer(bounds, paint)`/`restore_layer()` (`crates/flui-engine/src/wgpu/painter/layer.rs:325-351,513-533`, read in full) — `save_layer_impl` only ever threads `layer_opacity` (from `paint.color.a`) and `layer_blend` (from `paint.blend_mode`) into the compositor; there is no shader/mask parameter anywhere in this call graph. Since `layer_render.rs` passes `Paint::default()` (alpha=255→opacity 1.0, `blend_mode: SrcOver` default), the net effect of `Layer::ShaderMask` today is **an inert clip to `bounds` at full opacity** — visually indistinguishable from `Layer::ClipRect`, regardless of what shader/blend_mode the `ShaderMaskLayer` carries.

A **separate, fully-working** shader-mask pipeline already exists one level down, at the **Canvas/Picture** level: `Canvas::draw_shader_mask<F>(bounds, shader, blend_mode, draw_child: F) where F: FnOnce(&mut Canvas)` (`crates/flui-painting/src/canvas/drawing.rs:442-461`) synchronously records the child into its own `DisplayList` and emits `DrawCommand::ShaderMask{child, shader, bounds, blend_mode, transform}`, dispatched to `Backend::render_shader_mask(child: &DisplayList, shader, bounds, blend_mode, transform)` (`crates/flui-engine/src/wgpu/backend.rs:773-870+`, read through its offscreen-render setup) — which **does** render the child to a device-resolution offscreen texture and genuinely apply the shader as a GPU mask. **This path cannot be reused for `Layer::ShaderMask` as currently architected**: it requires the child content as an immediately-available `DisplayList`, but `PaintCx::paint_child()` deliberately does **not** provide that — it records a deferred `FragmentOp::Child` marker instead (module doc, `paint_cx.rs:17-20`: *"No live recursion... paint_child records a marker instead of re-entering the pipeline"*), because a `RenderShaderMask`'s child is an arbitrary render-object subtree (with its own children, possibly its own repaint boundaries), not a flat sequence of Canvas draw calls. This confirms the `Layer::ShaderMask` (closure-scoped, Layer-tree) design in §2.4 is the *only* option compatible with `PaintCx`'s sans-IO architecture — it is not a design oversight that a Canvas-level route wasn't chosen.

**This is a real, confirmed, pre-existing gap this task does not close** — flagged prominently rather than silently absorbed, per the task's explicit instruction to reclassify genuine blocking findings. It does **not** block the `flui-rendering`-level work in this plan (the LayerTree structurally gets the right node with the right fields, harness-verifiable, §2.6) — it blocks the *visual* correctness of `ShaderMask` on screen until a follow-up `flui-engine` task teaches `render_layer_recursive` to special-case `Layer::ShaderMask` the same way it already special-cases `Layer::BackdropFilter` (§6 sketches the shape).

### 2.6 Test-harness facilities — already sufficient, zero new plumbing needed

`FrameRun::structure(&self) -> Vec<&'static str>` (`crates/flui-rendering/src/testing/harness.rs:321-326`) walks the composed `LayerTree` via `inspect::layer_structure`, which is ultimately backed by `Layer::kind_name()` (`crates/flui-layer/src/layer/mod.rs:370-391`) — a `const fn` match already listing `"ShaderMask"`/`"BackdropFilter"` (`:387-388`) alongside `"ClipRect"`/`"Opacity"`/etc., already exercised by existing tests (`crates/flui-objects/tests/render_object_harness.rs:1530,1936,1970,3709` for `"Opacity"`/`"ClipRRect"`). `run.structure().contains(&"ShaderMask")` / `&"BackdropFilter"` will work the moment §2.4 lands, with **no new harness facility**.

Field round-trip (does `blend_mode` actually reach the pushed layer?) is answerable via `FrameRun::layer_tree(&self) -> Option<&LayerTree>` (`harness.rs:315-317`) + `LayerTree::iter(&self) -> impl Iterator<Item = (LayerId, &LayerNode)>` (`crates/flui-layer/src/tree/layer_tree.rs:857`) + the already-generated `Layer::as_shader_mask()`/`as_backdrop_filter()` downcasts (§2.1):
```rust
let (_, node) = run.layer_tree().unwrap().iter()
    .find(|(_, n)| n.layer().is_shader_mask()).expect("ShaderMask layer present");
assert_eq!(node.layer().as_shader_mask().unwrap().blend_mode(), BlendMode::Multiply);
```
Again, zero new harness code — every piece already exists and is exercised elsewhere in the same test file.

### 2.7 `always_needs_compositing()` — confirmed LIVE infra here (unlike `RenderPhysicalModel`)

The task asked to check this explicitly since it changes the physical-model plan's conclusion. `RenderBox::always_needs_compositing()` defaults to `false` (`crates/flui-rendering/src/traits/render_box.rs:382-384`) but **is actively consumed**: `PipelineOwner<Compositing>::update_subtree_compositing_bits_impl` (`crates/flui-rendering/src/pipeline/owner/compositing.rs:159`) does `if node.is_repaint_boundary_flag() || node.always_needs_compositing() { node.mark_needs_compositing(); }`, and a subsequent `old_needs_compositing != new_needs_compositing` check (`:174-179`) marks the node dirty-for-paint on a transition — this is the mechanism that would correctly re-trigger paint when a `RenderShaderMask`/`RenderBackdropFilter`'s child goes from absent to present (or vice versa) purely via the compositing-bits walk. This is **materially different from the physical-model plan's finding** for the analogous `elevation`-setter compositing-bits toggle, which that plan confirmed was dead code in the oracle itself (never read, since `RenderPhysicalModel` never overrides `alwaysNeedsCompositing`). Here, both oracle classes **do** override it (`child != null`, §1.1/1.2), and FLUI's consumer of the resulting bit is live, wired, and tested (`crates/flui-rendering/src/traits/render_sliver.rs:1006-1028`'s `sliver_always_needs_compositing_forward_through_dyn` proves the override channel itself works end-to-end for at least one existing type). **Recommendation: override `always_needs_compositing()` to return `self.has_child` on both new structs** — oracle-faithful, and plugs into infrastructure confirmed live rather than confirmed dead.

No `is_repaint_boundary()` override is needed for either (oracle never sets this true for either class) — the "PR-A2 U33 bootstrap" mechanism the physical-model plan found irrelevant remains irrelevant here too (it bootstraps `is_repaint_boundary`, a separate, static flag from the dynamic `always_needs_compositing` this section discusses).

## 3. FLUI struct/trait shape — explicit generic-vs-two-structs call

**Decision: two independent, non-generic structs — not a shared generic body.** Contrast with this repo's own two precedents for the opposite calls: the physical-model plan found `RenderPhysicalModel`/`RenderPhysicalShape` share a genuinely large, ~25-line, verbatim-identical paint/hit-test recipe differing only in shape-source type, which justified one generic body (`RenderPhysicalModelBase<C: PhysicalClipSource>`). The persistent-header plan found `Scrolling`/`Pinned`'s divergence was so small a generic was "arguably overkill" and recommended two plain structs for that pair specifically (while still generic-ing the `Floating`/`FloatingPinned` pair, whose shared re-reveal state machine WAS large enough to earn a generic).

`RenderShaderMask` and `RenderBackdropFilter` land firmly in the "two plain structs" bucket: their config types are categorically different (`Arc<dyn Fn(Rect<Pixels>) -> Shader + Send + Sync>` vs. a plain `ImageFilter` value), their gating logic differs (`BackdropFilter` has an `enabled` bool with its own independent early-return, §1.2; `ShaderMask` has none), their default `blend_mode` differs (`Modulate` vs. `SrcOver`, §1.1/1.2), their diagnostics differ (oracle gives BackdropFilter three properties and ShaderMask zero, §4.2), and they push different composer targets (`Layer::ShaderMask` vs. `Layer::BackdropFilter`). The only shared shape is "single-child proxy, draws nothing of its own, wraps `paint_child()` in one closure-scoped effect, and paints literally nothing when there is no child" — about four lines, not worth a generic parameter.

```rust
// crates/flui-objects/src/proxy/shader_mask.rs (new)
use std::sync::Arc;
use flui_types::{Point, Rect, Offset, Pixels, Size, painting::{BlendMode, Shader}};

/// `ShaderCallback` analog — same `Arc<dyn Fn(..) -> T + Send + Sync>` shape
/// already established by `proxy::clip::CustomClipper<S>`
/// (`clip.rs:345`, `Arc<dyn Fn(Size) -> S + Send + Sync + 'static>`), the
/// closest existing precedent for a stored Flutter-callback field in this crate.
pub type ShaderCallback = Arc<dyn Fn(Rect<Pixels>) -> Shader + Send + Sync + 'static>;

pub struct RenderShaderMask {
    shader_callback: ShaderCallback,
    blend_mode: BlendMode,   // default BlendMode::Modulate — oracle `:1133`, NOT SrcOver
    has_child: bool,
}

impl RenderBox for RenderShaderMask {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            size
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }
    flui_rendering::forward_single_child_box_queries!();

    fn always_needs_compositing(&self) -> bool { self.has_child }  // oracle `:1174-1175`, §2.7

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        if ctx.child_count() == 0 { return; } // oracle `:1191-1193` — nothing at all
        let bounds = Rect::from_origin_size(Point::ZERO, ctx.size()); // LOCAL rect, oracle `Offset.zero & size`
        let shader = (self.shader_callback)(bounds);
        ctx.with_shader_mask(shader, self.blend_mode, |ctx| ctx.paint_child());
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() { return false; }   // RenderBox default is LEAF-shaped (§4.6) — must override
        if self.has_child { ctx.hit_test_child_at_offset(0, Offset::ZERO) } else { false }
    }
}
```

```rust
// crates/flui-objects/src/proxy/backdrop_filter.rs (new)
use flui_types::{Point, Rect, Offset, Size, painting::{BlendMode, ImageFilter}};

pub struct RenderBackdropFilter {
    filter: ImageFilter,
    blend_mode: BlendMode,  // default BlendMode::SrcOver — oracle `:1212`
    enabled: bool,          // default true — oracle `:1213`
    has_child: bool,
}

impl RenderBox for RenderBackdropFilter {
    // perform_layout / forward_single_child_box_queries!() — identical shape to RenderShaderMask above.

    fn always_needs_compositing(&self) -> bool { self.has_child } // oracle `:1325-1326`

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        if !self.enabled {              // gate 1, oracle `:1330-1333` — bypasses the filter ENTIRELY
            ctx.paint_child();          // still nothing if no child (paint_child() is itself a no-op then)
            return;
        }
        if ctx.child_count() == 0 { return; } // gate 2, oracle `:1350-1352` — nothing at all
        ctx.with_backdrop_filter(self.filter.clone(), self.blend_mode, |ctx| ctx.paint_child());
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // identical shape to RenderShaderMask::hit_test above.
    }
}
```

Setters (`set_shader_callback`/`set_blend_mode` on `RenderShaderMask`; `set_filter`/`set_blend_mode`/`set_enabled` on `RenderBackdropFilter`) follow `RenderClip::set_clipper`'s established convention exactly (`clip.rs:430-438`): unconditional overwrite + return a changed-`bool` (for `shader_callback`, this can only be presence/identity-based since `Arc<dyn Fn>` has no `PartialEq` — mirrors `set_clipper`'s own `new_some != old_some` return, since closures can't be deep-compared either).

Diagnostics: `RenderBackdropFilter` surfaces `filter`/`blend_mode`/`enabled` (matching oracle `:1355-1363`, modulo the `filterConfig`→`filter` renaming from §1.3's scope decision). `RenderShaderMask` — **recommend surfacing `blend_mode`** even though the oracle has zero diagnostics for this class (§1.1) — a deliberate, documented FLUI-side improvement for catalog-wide consistency (every other proxy render object in this crate, `RenderClip`/`RenderOpacity`/`RenderPhysicalModel`, surfaces all its fields), not a silent divergence.

## 4. Traps a naive port would fall into

1. **Transcribing the current oracle's `ImageFilterConfig`/`BackdropKey` surface verbatim** builds dead plumbing — no FLUI-side consumer exists for either (§1.3, §2.1). Scope to the classic `filter`/`blendMode`/`enabled` surface; document the newer surface as deferred (§6), don't silently half-build it.
2. **`RenderShaderMask` has zero oracle diagnostics; `RenderBackdropFilter` has three.** A port that mechanically mirrors "no diagnostics" for ShaderMask (byte-faithful) misses a real, low-cost opportunity for FLUI-wide catalog consistency; a port that "fixes" BackdropFilter's diagnostics to match ShaderMask's absence loses real information. Recommend adding `blend_mode` diagnostics to `RenderShaderMask` as documented FLUI-side improvement (§3).
3. **The shader-callback rect is LOCAL for the callback invocation but GLOBAL for the stored `maskRect`** (`Offset.zero & size` vs. `offset & size`, §1.1, §2.4). A port that resolves the shader with the origin-shifted (global) rect, or that stores the LOCAL rect directly on the layer without going through the composer's existing `translate_offset(origin)` step, breaks gradient positioning the moment the `ShaderMask` isn't at the tree root.
4. **`BackdropFilter`'s `enabled` gate and no-child gate are independent, not combined** (§1.2). `enabled = false` bypasses everything (filter resolution, layer push) and falls through to plain child-paint; `enabled = true` + no child paints literally nothing. Collapsing these into one `if enabled && has_child` condition changes behavior: `enabled=false` with a child must still paint that child (unfiltered), not skip painting entirely.
5. **Default `blend_mode` differs between the two classes** (`Modulate` for ShaderMask, `SrcOver` for BackdropFilter, §1.1/1.2) — copying one default's constructor across both structs is an easy, silent behavioral regression.
6. **FLUI's `RenderBox::always_needs_compositing()` trait default is `false`, and this is confirmed LIVE, consumed infra here** (§2.7) — unlike the physical-model plan's finding for the analogous mechanism (confirmed dead there). Skipping the override here isn't "harmless like last time"; it silently disables the compositing-bits transition-detection path for these two objects specifically.
7. **FLUI's `RenderBox::hit_test()` trait default is LEAF-shaped** (`ctx.is_within_own_size()` alone, returning `true`/`false` without ever recursing into children — `crates/flui-rendering/src/traits/render_box.rs:171-173`), **not proxy-shaped**. Both new structs MUST explicitly override `hit_test` to forward to the child (mirroring `RenderProxyBoxMixin::hitTestChildren`, §1.4) — relying on the trait default (as one might expect from "this is just a pass-through proxy, surely the default forwards") silently makes these objects hit-test-opaque leaves that swallow all child interaction. Unlike `RenderClip`/`RenderPhysicalModel`'s hit-test, **no shape-containment gate is needed** here — the mask/filter is purely visual, oracle imposes no hit-test restriction beyond the child's own bounds (§1.4).
8. **The engine cannot currently render `ShaderMask`'s visual effect at all** (§2.5) — once §2.4's wiring lands, do not report "ShaderMask now works" without qualifying that the LayerTree is structurally correct (harness-verifiable) while the on-screen pixels remain an unmasked clip until the `flui-engine` follow-up (§6) lands. This is precisely the "MVP reported as parity" trap AGENTS.md names.
9. **`BackdropFilter`'s GPU blur only covers `ImageFilter::Blur`** (§ headline #3) — the render-object-level `filter` field accepts any `ImageFilter` variant (`Dilate`/`Erode`/`Matrix`/`ColorAdjust`/`Compose`), but only `Blur` visually blurs the backdrop today; the rest silently degrade to "children only, no backdrop effect" with a `tracing::warn!`. Don't claim full `ImageFilter` coverage for backdrop use without this caveat.
10. **`FragmentClip`'s naming** (§2.3) — if the rename to `FragmentScope` is skipped, at minimum update its doc comment; leaving it named `FragmentClip` while it carries `ShaderMask`/`BackdropFilter` variants is a silent, confusing divergence for the next contributor who greps for "clip" expecting only clip-shaped things.

## 5. Test plan

Pattern precedent: `crates/flui-objects/tests/render_object_harness.rs`'s `harness_clip_*` (`:1746-1815`) and `harness_opacity_*` blocks are the structural template (`box_node(...).child(...)`, `run_layout()`/`run_frame()`, `assert_descendant_properties`), extended with `run.structure()` (already used for `"Opacity"`/`"ClipRRect"`, `:1530,1936`) and direct `LayerTree::iter()` walks (§2.6) for field round-trips — no new harness facility required anywhere in this test plan.

- **Layout pass-through**: `box_geometry(root) == box_geometry(child)` for both — Single-arity proxy, size = child's size (mirrors `harness_clip_rect_self_describes`/`harness_opacity_*`).
- **No child**: mount with zero children for both variants; assert `run.structure()` does **not** contain `"ShaderMask"`/`"BackdropFilter"` and `run.painted()`/`display_commands()` shows nothing drawn — the regression test for oracle's `layer = null` early return (§1.1 `:1191-1193`, §1.2 `:1350-1352`).
- **With child, structural push**: mount with a real child; assert `run.structure().contains(&"ShaderMask")` / `&"BackdropFilter"` — the direct proof the task requires, using the confirmed-ready `kind_name()`/`structure()` facility (§2.6).
- **Field round-trip**: construct with a non-default `blend_mode` (e.g. `BlendMode::Multiply`); walk `run.layer_tree().unwrap().iter()` to the `ShaderMaskLayer`/`BackdropFilterLayer` node via `is_shader_mask()`/`as_shader_mask()` (or the `BackdropFilter` equivalents) and assert `.blend_mode()` matches (§2.6) — catches a composer wiring bug that drops or hardcodes the field.
- **`enabled = false` bypass** (`RenderBackdropFilter` only): mount with a child and `enabled = false`; assert `run.structure()` does **not** contain `"BackdropFilter"` AND the child's own draw commands still appear in `display_commands()` — the regression test for trap §4.4's independent-gates finding (a naive combined-gate port would either still push the layer, or wrongly skip painting the child).
- **`shader_callback` rect argument**: construct with a `shader_callback` closure that captures the `Rect` it was called with (e.g. into a shared `Cell`/`RefCell`); after paint, assert the captured rect equals `Rect::from_origin_size(Point::ZERO, child_size)` — i.e., LOCAL, not offset-shifted — the regression test for trap §4.3.
- **Default `blend_mode` values**: construct each struct via its plain constructor with no explicit `blend_mode`; assert `RenderShaderMask`'s is `BlendMode::Modulate` and `RenderBackdropFilter`'s is `BlendMode::SrcOver` — the regression test for trap §4.5.
- **`always_needs_compositing`**: unit-test (no harness needed) that `always_needs_compositing()` tracks `has_child` on both structs, exercising the same "trait-object-forwarded correctly" concern `render_sliver.rs:1023-1028`'s existing test covers for the sliver side (§2.7).
- **Hit-test forwarding**: a child positioned to receive a hit at a given point; assert the hit reaches the child through both `RenderShaderMask` and `RenderBackdropFilter` unmodified by any shape gate (regression test for trap §4.7 — confirms neither struct accidentally inherits or reintroduces a clip-style shape-containment check).
- **Diagnostics**: `assert_descendant_properties` for `RenderBackdropFilter` checking `filter`/`blend_mode`/`enabled`; for `RenderShaderMask` checking `blend_mode` (the FLUI-side improvement from §3/trap §4.2 — a test that would fail under a byte-faithful "zero diagnostics" port, documenting the deliberate divergence).
- **Dry layout/baseline**: not applicable — oracle's `RenderProxyBoxMixin` computes dry layout/baseline as plain child-forwards (§1.4); confirm `forward_single_child_box_queries!()`'s default forwarding suffices (no new work, matching every other proxy object's precedent).
- **Catalog guard**: add `"RenderShaderMask"` and `"RenderBackdropFilter"` to `RENDER_OBJECT_TYPES` (`crates/flui-objects/tests/render_object_harness.rs:131-...`, alongside the `RenderOpacity`/`RenderClip*` rows at `:148,154-157`), register `mod shader_mask; mod backdrop_filter;` + `pub use shader_mask::*; pub use backdrop_filter::*;` in `crates/flui-objects/src/proxy/mod.rs` (alongside the existing `mod opacity;`/`mod physical_model;` entries, `:1-15`), and add both names to the flat re-export list in `crates/flui-objects/src/lib.rs:58-63`.

## 6. Deferred, documented (not silently dropped)

- **`ImageFilterConfig` (bounded blur, `.compose`)** — needs a new FLUI type modeling "resolve against a paint-time `Rect`" plus a `bounded`/`tile_mode` extension to `ImageFilter::Blur`; confirmed zero existing FLUI backing (§1.3). A future pass, not blocking the classic `ImageFilter`-value surface this plan targets.
- **`BackdropKey`/`BackdropGroup` shared-backdrop sampling** — needs a `backdrop_key` field on `flui-layer`'s `BackdropFilterLayer` (currently absent, §2.1) AND an engine-side batching optimization in the Dual Kawase blur path (currently single-layer-at-a-time, no sharing concept at all); a widget-layer `BackdropGroup` `InheritedWidget` besides. Confirmed zero consumers anywhere in FLUI today — do not add the field speculatively.
- **The deprecated `filter` getter/setter** — oracle marks it `@Deprecated` itself (`:1259-1273`); skip entirely, expose only `filter`/`set_filter` at the `ImageFilter`-value level this plan targets (§1.3's scoped-down surface already IS the modern `filterConfig`-equivalent for the variants FLUI supports).
- **`flui-engine`'s `Layer::ShaderMask` visual gap (§2.5)** — the concrete, confirmed, real gap this plan surfaces but does not close. Recommended shape for the follow-up: mirror `handle_backdrop_filter`'s existing special-case pattern in `render_layer_recursive` (`renderer.rs:1517-1600+`) — special-case `Layer::ShaderMask` the same way `Layer::BackdropFilter` is special-cased today (checked via `if let flui_layer::Layer::ShaderMask(sm_layer) = layer`), capturing the layer's child subtree to an offscreen texture (the composer's existing "inline children merge into one `PictureLayer`" behavior, §2.4's paint.rs citation, means the common non-repaint-boundary-child case already produces exactly one `PictureLayer` child — the tractable starting case), then routing through a texture-based sibling of `Backend::render_shader_mask` instead of the current inert `save_layer`/`push_clip_rect` pair. Falling back to "children only, no mask" for a `ShaderMask` wrapping a repaint-boundary child (mirroring `handle_backdrop_filter`'s own `ImageFilter`-not-`Blur` fallback, §2.5) is a reasonable first-cut scope boundary for that follow-up.
- **`flui-engine`'s non-`Blur` `ImageFilter` support for `BackdropFilterLayer`** (§ headline #3) — `Dilate`/`Erode`/`Matrix`/`ColorAdjust`/`Compose` currently degrade silently; a follow-up engine task, not blocking this plan's classic-blur-dominant scope.
- **Semantics** (no semantics tree in FLUI yet) — consistent with every other catalog entry.

### Critical Files for Implementation
- `crates/flui-rendering/src/context/paint_cx.rs` (new `with_shader_mask`/`with_backdrop_filter` methods; `FragmentClip` extension/rename)
- `crates/flui-rendering/src/pipeline/owner/paint.rs` (`clip_layer`/`scope_layer` new match arms producing `Layer::ShaderMask`/`Layer::BackdropFilter`)
- `crates/flui-objects/src/proxy/shader_mask.rs` (new — `RenderShaderMask`, `ShaderCallback` type alias)
- `crates/flui-objects/src/proxy/backdrop_filter.rs` (new — `RenderBackdropFilter`)
- `crates/flui-objects/src/proxy/clip.rs` (existing — `CustomClipper<S> = Arc<dyn Fn(Size) -> S + Send + Sync + 'static>` precedent for `ShaderCallback`'s shape, and the closure-scoped `with_clip_scope` pattern §2.3/§2.4 extend)
- `crates/flui-engine/src/wgpu/layer_render.rs` (documents the confirmed `Layer::ShaderMask` visual-application gap, §2.5 — read, not modified, by this plan)
- `crates/flui-objects/tests/render_object_harness.rs` (catalog registration; `harness_clip_*`/`harness_opacity_*` structural template; `run.structure()`/`LayerTree::iter()` facilities already sufficient)
