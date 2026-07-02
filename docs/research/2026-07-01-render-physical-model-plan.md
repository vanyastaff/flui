# `RenderPhysicalModel` / `RenderPhysicalShape` — plan (oracle-verified)

Core.2 catalog item, gated on `flui-engine`/`flui-layer` shadow+clip infra — that gate is closed per the prior scoping pass. Oracle: `.flutter/flutter-master/packages/flutter/lib/src/rendering/proxy_box.dart`, `_RenderPhysicalModelBase<T>` (`:2062-2126`), `RenderPhysicalModel` (`:2132-2269`), `RenderPhysicalShape` (`:2280-2373`), plus their shared private base `_RenderCustomClip<T>` (`:1484-1608`), read in full.

## Headline verdict, up front

1. **This is a straight port, not an architecture task — confirmed a second time, more specifically.** Every primitive the oracle's `paint()` calls (`drawShadow`, `drawRRect`/`drawPath`, `pushClipRRect`/`pushClipPath`, `drawPaint`) has a direct, already-shipped FLUI equivalent (`Canvas::draw_shadow`, `draw_rrect`/`draw_path`, `PaintCx::with_clip_rrect`/`with_clip_path`, `draw_paint`). The one real gap found (`transparentOccluder` on `drawShadow`) turns out to be **inapplicable, not missing** — see §2.4.
2. **The right FLUI shape is a generic struct over a *new*, small, local trait — not a reuse of `proxy::clip::ClipGeometry`, and not two independent structs.** The prior scoping pass named `RenderClip<S: ClipGeometry>` (`crates/flui-objects/src/proxy/clip.rs`) as the precedent to check. It is the right *pattern* (one generic body, sealed shape parameter, zero `Box<dyn>` in the hot path) but the wrong *trait to reuse directly* — `ClipGeometry` is scoped to "given only a `Size`, produce a shape" (no room for `RenderPhysicalModel`'s extra `shape: BoxShape` + `border_radius` config, which `default_for_size(size: Size) -> Self` can't carry) and has no shadow/fill vocabulary (plain clips never draw a shape, only clip). §3 defines a new, physical-model-scoped `PhysicalClipSource` trait instead, reusing `RRect`/`Path`/`CustomClipper<S>` as plain data types without touching `clip.rs` at all.
3. **Three confirmed, worth-flagging divergences from a literal transcription of the oracle**, all backed by direct citations: an actual bug in Flutter's own `debugFillProperties` (§4.1), a hit-test asymmetry between the two classes that a naive "always test the clip shape" port would erase (§4.2), and `RenderPhysicalModel`'s default `clipBehavior` being `Clip.none` — not `Clip.antiAlias` like every other class in the same file (§4.5).

## 1. Oracle — `rendering/proxy_box.dart` (line-cited)

### 1.1 `_RenderCustomClip<T>` (`:1484-1608`) — the shared grandparent, already ported once

`RenderPhysicalModel`/`RenderPhysicalShape` both extend `_RenderPhysicalModelBase<T> extends _RenderCustomClip<T>`, so their inherited contract is identical to what `RenderClip<S>` (`crates/flui-objects/src/proxy/clip.rs`) already ported for `RenderClipRect`/`RenderClipRRect`/`RenderClipOval`/`RenderClipPath`:
- Constructor default `clipBehavior = Clip.antiAlias` (`:1488`) — **overridden down to `Clip.none`** one level down, in `_RenderPhysicalModelBase`'s own constructor (`:2071`, `super.clipBehavior = Clip.none`). `RenderPhysicalModel`/`RenderPhysicalShape` never re-override this, so the *effective* default for both is `Clip.none` — physical-model surfaces don't clip by default, unlike every other member of the clip family. See trap §4.5.
- `_updateClip()` (`:1555-1557`): `_clip ??= _clipper?.getClip(size) ?? _defaultClip;` — memoized, invalidated by `_markNeedsClip()` (clipper/shape/borderRadius setters) and by `performLayout` on a size change (`:1546-1553`). **`RenderClip<S>`'s own port already dropped this memoization** (`clip.rs:440-446`'s doc comment: "one closure call... per paint/hit-test, which is negligible") — carry that exact same simplification forward here; recomputing `RRect::from_rect_and_corners(...)` from three plain fields every paint/hit-test is cheaper than the closure indirection `RenderClip<S>` already accepted.
- `hitTest` is **not** defined on `_RenderCustomClip` itself — each of the four public clip subclasses, and both `RenderPhysicalModel`/`RenderPhysicalShape`, redefine the identical pattern individually (`if (_clipper != null) { ...test shape...} return super.hitTest(...)`). See §4.2 — this gate matters more here than it does for the plain clip family.

### 1.2 `_RenderPhysicalModelBase<T>` (`:2062-2126`) — shared fields/setters

- Constructor (`:2064-2073`): `required child, required elevation, required color, required shadowColor, clipBehavior = Clip.none, clipper` — `assert(elevation >= 0.0)` (`:2071`, and repeated on both concrete subclasses at `:2146`/`:2293` — **triple-asserted invariant**, port as a debug assertion at all three levels or once centrally; either is oracle-faithful since Dart re-asserts defensively at every constructor in the chain).
- `elevation` setter (`:2085-2093`): guards on `elevation == value` (no-op skip), then — **this is dead code, confirmed**: it computes `didNeedCompositing = alwaysNeedsCompositing` before mutating and calls `markNeedsCompositingBitsUpdate()` if that value changed after. `alwaysNeedsCompositing` is **never overridden** anywhere in this class or either subclass (grepped `proxy_box.dart:2062-2373` in full — zero hits) — it stays at `RenderBox`'s own default (`box.dart:3091`, `=> false`) unconditionally, so `didNeedCompositing != alwaysNeedsCompositing` is always `false != false`, i.e. this branch **never fires**. `markNeedsPaint()` always fires, unconditionally, after the guard. FLUI's `RenderPhysicalModelBase<C>` does not need a compositing-bits-update concept for this feature at all — see §4.6.
- `color`/`shadowColor` setters (`:2103-2119`): plain guard-and-set, `markNeedsPaint()` only.
- `debugFillProperties` (`:2119-2125`): adds `elevation` (Double), `color` (Color), and — **confirmed oracle bug** — `description.add(ColorProperty('shadowColor', color));` at `:2124` passes `color` a second time instead of `shadowColor`. Do not reproduce this; FLUI's diagnostics should surface the real `shadow_color` field. Flag as a documented, deliberate correction, not a silent divergence.

### 1.3 `RenderPhysicalModel` (`:2132-2269`) — `BoxShape` + `BorderRadius`

- Constructor (`:2132-2147`): `shape: BoxShape = BoxShape.rectangle`, `borderRadius: BorderRadius?` (nullable, doc at `:2169-2174`: "ignored if shape is not rectangle... null treated like `BorderRadius.zero`"), `elevation = 0.0`, `required color`, `shadowColor = Color(0xFF000000)` (opaque black). **No `clipper` parameter exposed publicly** — `_RenderPhysicalModelBase`'s `clipper` field/setter exists only at the base-class level; `RenderPhysicalModel`'s own constructor never forwards one. This has a real hit-test consequence — §4.2.
- `shape`/`borderRadius` setters (`:2154-2185`): both call `_markNeedsClip()` (paint + semantics dirty, no relayout), not a plain `markNeedsPaint()`.
- `_defaultClip` (`:2182-2189`):
  ```dart
  RRect get _defaultClip {
    final Rect rect = Offset.zero & size;
    return switch (_shape) {
      BoxShape.rectangle => (borderRadius ?? BorderRadius.zero).toRRect(rect),
      BoxShape.circle => RRect.fromRectXY(rect, rect.width / 2, rect.height / 2),
    };
  }
  ```
  The circle branch is **`width/2, height/2` as two independent radii** — an ellipse inscribed in a non-square box, not a true circle. See trap §4.4.
- `hitTest` (`:2192-2200`): gated on `_clipper != null` — see §4.2.
- `paint` (`:2204-2262`), the core recipe:
  ```dart
  if (child == null) { layer = null; return; }              // :2206-2209, nothing at all is drawn
  _updateClip();
  final offsetRRect = _clip!.shift(offset);
  final offsetRRectAsPath = Path()..addRRect(offsetRRect);
  var paintShadows = true;
  assert(() {                                                 // debug-only, §4.7
    if (debugDisableShadows) {
      if (elevation > 0.0) canvas.drawRRect(offsetRRect, strokePaint(shadowColor, elevation*2));
      paintShadows = false;
    }
    return true;
  }());
  if (elevation != 0.0 && paintShadows) {
    canvas.drawShadow(offsetRRectAsPath, shadowColor, elevation, color.alpha != 0xFF);  // :2233
  }
  final usesSaveLayer = clipBehavior == Clip.antiAliasWithSaveLayer;                     // :2235
  if (!usesSaveLayer) canvas.drawRRect(offsetRRect, fillPaint(color));                   // :2236-2238, OUTSIDE the clip
  layer = context.pushClipRRect(needsCompositing, offset, Offset.zero & size, _clip!,
    (context, offset) {
      if (usesSaveLayer) context.canvas.drawPaint(fillPaint(color));                    // :2245-2249, INSIDE the clip
      super.paint(context, offset);                                                     // paints the child
    }, oldLayer: layer, clipBehavior: clipBehavior);
  ```
  Read precisely: **shadow → [fill outside the clip, only if not save-layer] → clip-push { [fill inside the clip, only if save-layer] → child }**. Confirms the task's stated ordering exactly, with the `usesSaveLayer` fork as the one real subtlety (§4.3).
- `debugFillProperties` (`:2264-2269`): adds `shape` and `borderRadius` on top of the base three.

### 1.4 `RenderPhysicalShape` (`:2280-2373`) — mandatory `CustomClipper<Path>`

- Constructor (`:2280-2293`): `required CustomClipper<Path> super.clipper` — **mandatory**, unlike `RenderPhysicalModel`'s total absence of a public clipper. No `shape`/`borderRadius` concept at all.
- `_defaultClip` (`:2296`): `Path()..addRect(Offset.zero & size)` — only reachable if `clipper` is later nulled via the inherited setter (rare; the constructor always supplies one).
- `hitTest`/`paint` (`:2298-2365`): **byte-for-byte the same recipe as `RenderPhysicalModel`**, with `Path` substituted for `RRect` throughout (`_clip!.shift(offset)` on a `Path`, `canvas.drawPath` instead of `drawRRect`, `context.pushClipPath` instead of `pushClipRRect`). Confirms the two classes are the same algorithm parametrized by shape type — the generic-worthy insight driving §3.
- `debugFillProperties` (`:2369-2373`): adds `clipper` only.

### 1.5 `debugDisableShadows` — confirmed debug/inspector-only, safe to defer

`bool debugDisableShadows = false;` (`painting/debug.dart:40`), read only inside `assert(() { ... return true; }())` blocks (`proxy_box.dart:2216,2321`, and reused identically by `box_shadow.dart`, `box_decoration.dart`, `shape_decoration.dart`). It compiles out of release builds entirely and exists so golden-image tests get deterministic, non-blurred output. **Confirmed: purely a debug/testing feature, zero runtime behavioral weight in a release build** — defer per §6, do not build it now.

### 1.6 `transparentOccluder` / `color.alpha != 0xFF` — confirmed a Skia-specific parameter that does not map onto FLUI's shadow model

`canvas.drawShadow(path, color, elevation, transparentOccluder)` (`lib/ui/painting.dart:7695-7699`, doc: *"The `transparentOccluder` argument should be true if the occluding object is not opaque"*). Traced to the engine dispatcher (`display_list/skia/dl_sk_dispatcher.cc:341-343`): it sets `SkShadowFlags::kTransparentOccluder_ShadowFlag`, which changes Skia's `SkShadowUtils::DrawShadow` ambient/spot tonal-color computation to avoid a self-occlusion artifact specific to Skia's two-light (ambient+spot) shadow-casting algorithm.

**FLUI's `Canvas::draw_shadow` signature is `draw_shadow(&mut self, path: &Path, color: Color, elevation: f32)`** (`crates/flui-painting/src/canvas/drawing.rs:254-263`, checked against the live source, not memory) — **no fourth parameter, anywhere in the pipeline**: `DrawCommand::DrawShadow { path, color, elevation, transform }` (`flui-painting/src/display_list/command.rs`), `WgpuPainter::draw_shadow(path, color, elevation)` (`flui-engine/src/wgpu/painter/draw.rs:441`), and the shader itself (`flui-engine/src/wgpu/shaders/effects/shadow.wgsl`) implement a **from-scratch single-color analytic Gaussian rounded-rect shadow** (Evan Wallace's fast-shadow technique), not Skia's dual-tone ambient/spot model — there is no tonal-color computation, no self-occlusion test, and no separate ambient/spot terms for `transparentOccluder` to gate. **Confirmed: this is a real, but inapplicable, gap** — the parameter has nothing to attach to in FLUI's chosen shadow algorithm; do not add it to `Canvas::draw_shadow`'s signature for this feature, and do not silently drop the `color.alpha != 0xFF` computation into a no-op parameter either. Document as a **confirmed non-issue**, not a deferred TODO.

## 2. FLUI building blocks — what exists, verified against live source

- **`Path::from_rrect(rrect: RRect) -> Self`** already exists (`crates/flui-types/src/painting/path.rs:154-171`) and builds a proper bezier-cornered rounded-rect outline — this is exactly `RenderPhysicalModel`'s `Path()..addRRect(offsetRRect)` (oracle `:2211`ish) for the shadow path. `Path::add_rrect` (a mutating method) does **not** exist — only the associated-function form does; call `Path::from_rrect(rrect)`, not a hypothetical `path.add_rrect(rrect)`.
- **`RRect::from_rect_and_corners(rect, top_left, top_right, bottom_right, bottom_left)`** exists (`crates/flui-geometry/src/rrect.rs:210-218`), fields named identically to `Corners<Radius<Pixels>>` = `BorderRadius`'s own fields (`crates/flui-geometry/src/corners.rs:12-20`, `top_left`/`top_right`/`bottom_right`/`bottom_left`) — the conversion is a direct field-for-field pass-through, no adapter needed.
- **A near-identical `BorderRadius → RRect` conversion already ships**, one crate over: `flui-painting/src/decoration.rs:94-102`'s private `decoration_rrect` helper does exactly `RRect::from_rect_and_corners(rect, radius.top_left, radius.top_right, radius.bottom_right, radius.bottom_left)` for `BoxDecoration`. **This is the pattern to model the new code on**, not a from-scratch design — and, tellingly, it does **not** call `.clamp_radii()` either (see trap §4.4 for why that's the right call here too, not an oversight to fix).
- **`RRect::from_rect_xy(rect, rx, ry)`** exists (`rrect.rs:200-205`, an alias for `from_rect_elliptical`) — the exact building block for `BoxShape::Circle`'s `RRect.fromRectXY(rect, width/2, height/2)`.
- **`PaintCx<'_, Single>`'s clip API** (`crates/flui-rendering/src/context/paint_cx.rs:353-391`): `with_clip_rect(rect, behavior, f)`, `with_clip_rrect(rrect: flui_types::RRect, behavior, f)`, `with_clip_path(path: flui_types::painting::Path, behavior, f)` — all closure-scoped exactly like `RenderClip<S>::paint` already uses them (`clip.rs:521-528`). `ctx.canvas()` returns a canvas **already translated to local coordinates** — unlike the oracle's `paint(context, offset)`, no manual `.shift(offset)` step is needed anywhere in the FLUI port; every oracle `_clip!.shift(offset)` collapses to just using the untranslated local shape.
- **`Clip::saves_layer()`** exists (`crates/flui-types/src/painting/clipping.rs`, `matches!(self, Clip::AntiAliasWithSaveLayer)`) — the exact boolean the `usesSaveLayer` fork needs. The paint pipeline's own comment (`flui-rendering/src/pipeline/owner/paint.rs:461-465`) confirms clips are "always a real clip layer today," with save-layer-vs-not lowering deferred to a composer-side optimization — meaning correctness of the `usesSaveLayer` fill-placement fork (§4.3) does not depend on speculating about the wgpu backend's internals; replicate the oracle's exact branch regardless.
- **`Canvas::draw_paint(&mut self, paint: &Paint)`** exists (`drawing.rs`, "Fills entire canvas with a paint (respects clipping)") — the exact analog of the oracle's `context.canvas.drawPaint(Paint()..color=color)` used inside the save-layer branch.
- **`Paint::fill(color: Color) -> Self`** (`crates/flui-types/src/painting/paint.rs:110-125`) is the one-line fill-paint constructor needed everywhere in `paint()`.
- **Test-harness draw-order inspection already exists and is exactly what this feature's paint-order tests need**: `FrameRun::display_commands(&self) -> Vec<DrawCommandSummary>` (`crates/flui-rendering/src/testing/harness.rs:564`), backed by `collect_commands` (`testing/snapshot.rs:920-938`), which walks the **whole `LayerTree` in pre-order across Picture layers**, recursing through every layer boundary (clip layers included, since it walks `node.children()` unconditionally regardless of layer variant). `DrawKind::Shadow`/`RRect`/`Path`/`Clip` are already distinct summary categories (`snapshot.rs:26-57`). This means a single harness assertion over `run.display_commands()` can directly verify `Shadow` appears before the fill's `RRect`/`Path` command, which appears before the child's own commands — no new harness facility is required.
- **`is_repaint_boundary()`/`always_needs_compositing()`** are defaulted trait methods on `RenderBox`/`RenderSliver`/`RenderObject` (`crates/flui-rendering/src/traits/render_box.rs:372,383`, etc.) that `RenderClip<S>` and every other proxy render object leave at their defaults. Confirmed: `RenderPhysicalModelBase<C>` should do the same — the oracle **never overrides `alwaysNeedsCompositing`** for this class (§1.2), and `Material`'s own widget source (`material/material.dart:475-528`) wraps its `AnimatedPhysicalModel`/`PhysicalShape` output in no explicit `RepaintBoundary` either. **The "PR-A2 U33 bootstrap IS_REPAINT_BOUNDARY flag" mechanism is confirmed automatic/unrelated to this render object** — it needs zero special wiring here.

## 3. FLUI struct/trait shape

**The generic-vs-two-structs call, made explicitly**: unlike the persistent-header plan's *static* pair (Scrolling/Pinned), where the two variants' divergence was small enough that a generic wasn't worth it, `RenderPhysicalModel`/`RenderPhysicalShape` share a genuinely large body — the entire ~25-line paint recipe (shadow → conditional fill → clip-scope → conditional inner fill → child), the hit-test recipe, four field-level setters (elevation/color/shadow_color/clip_behavior), and the diagnostics base — verbatim identical per §1.3/§1.4's side-by-side reading of the oracle. That is worth deduplicating with one generic body, matching the `RenderClip<S>` *pattern*. But `ClipGeometry` itself (`clip.rs:139-171`) is the wrong trait to parametrize over: its `default_for_size(size: Size) -> Self` is a pure function of size alone, with no room for `RenderPhysicalModel`'s extra `shape: BoxShape` + `border_radius: Option<BorderRadius>` per-instance config, and it has no shadow/fill vocabulary at all (plain `RenderClip<S>` never draws a shape, only clips). Retrofitting those onto the sealed, four-shape `ClipGeometry` trait (which also serves `Rect`/`Oval`, neither of which physical-model needs) is a worse fit than a small trait scoped to exactly this family. **Decision: a new, local `PhysicalClipSource` trait, generic body, zero changes to `clip.rs`.**

```rust
// crates/flui-objects/src/proxy/physical_model.rs (new)

/// Shape-level operations shared by the two clip carriers physical-model
/// surfaces use (`RRect`, `Path`). Deliberately NOT `ClipGeometry` — see
/// module doc for why (extra per-instance config on RRect's source,
/// shadow/fill vocabulary ClipGeometry has no need for).
pub trait PhysicalClipShape: Clone + fmt::Debug + Send + Sync + 'static {
    fn contains(&self, position: Point<Pixels>) -> bool;
    /// The path `Canvas::draw_shadow` casts against.
    fn shadow_path(&self) -> Path;
    /// Fills the shape directly on the CURRENT canvas (used for the
    /// non-save-layer branch, drawn before the clip is pushed).
    fn fill(&self, canvas: &mut Canvas, paint: &Paint);
    fn with_clip_scope(
        &self,
        ctx: &mut PaintCx<'_, Single>,
        behavior: Clip,
        f: impl FnOnce(&mut PaintCx<'_, Single>),
    );
}

impl PhysicalClipShape for RRect { /* contains via existing RRect corner-ellipse test (mirrors
    ClipGeometry's RRect impl, clip.rs:198-275, but this is a fresh, non-trait-coupled impl);
    shadow_path -> Path::from_rrect(*self); fill -> canvas.draw_rrect(*self, paint);
    with_clip_scope -> ctx.with_clip_rrect(*self, behavior, f) */ }

impl PhysicalClipShape for Path { /* contains -> self.contains(position) (fill-type aware);
    shadow_path -> self.clone(); fill -> canvas.draw_path(self, paint);
    with_clip_scope -> ctx.with_clip_path(self.clone(), behavior, f) */ }

/// Per-variant "how do I derive the clip shape from `size`" source.
pub trait PhysicalClipSource: Clone + fmt::Debug + Send + Sync + 'static {
    type Shape: PhysicalClipShape;
    const DIAGNOSTIC_NAME: &'static str;
    fn compute_clip(&self, size: Size) -> Self::Shape;
    fn debug_fill_extra(&self, builder: &mut DiagnosticsBuilder);
}

#[derive(Debug, Clone)]
pub struct RectangleClip {
    pub shape: BoxShape,
    pub border_radius: Option<BorderRadius>,
}
impl PhysicalClipSource for RectangleClip {
    type Shape = RRect;
    const DIAGNOSTIC_NAME: &'static str = "RenderPhysicalModel";
    fn compute_clip(&self, size: Size) -> RRect {
        let rect = Rect::from_origin_size(Point::ZERO, size);
        match self.shape {
            BoxShape::Rectangle => {
                let br = self.border_radius.unwrap_or(BorderRadius::ZERO);
                // Mirrors flui-painting/src/decoration.rs:94-102's decoration_rrect
                // exactly — same field destructure, same lack of clamp_radii (§4.4).
                RRect::from_rect_and_corners(rect, br.top_left, br.top_right, br.bottom_right, br.bottom_left)
            }
            // Oracle proxy_box.dart:2188 — width/2, height/2 as TWO INDEPENDENT
            // radii (an inscribed ellipse for non-square boxes), NOT a true
            // circle. See trap §4.4 before "fixing" this to look more circular.
            BoxShape::Circle => RRect::from_rect_xy(rect, rect.width() * 0.5, rect.height() * 0.5),
        }
    }
    // debug_fill_extra: add_enum("shape", ...), add_optional("border_radius", ...)
}

#[derive(Clone)]
pub struct PathClip {
    pub clipper: Option<CustomClipper<Path>>, // reuses proxy::clip's existing type alias
}
impl PhysicalClipSource for PathClip {
    type Shape = Path;
    const DIAGNOSTIC_NAME: &'static str = "RenderPhysicalShape";
    fn compute_clip(&self, size: Size) -> Path {
        match &self.clipper {
            Some(c) => (c)(size),
            None => { // oracle `:2296`'s _defaultClip fallback
                let mut p = Path::new();
                p.add_rect(Rect::from_origin_size(Point::ZERO, size));
                p
            }
        }
    }
    // debug_fill_extra: add_flag("custom_clipper", self.clipper.is_some(), ...)
}

pub struct RenderPhysicalModelBase<C: PhysicalClipSource> {
    clip_source: C,
    elevation: f32,
    color: Color,
    shadow_color: Color,
    clip_behavior: Clip,   // default Clip::None — §4.5
    has_child: bool,       // mirrors RenderClip<S>'s own field, same reason (no
                           // child_count() on BoxHitTestContext)
}

pub type RenderPhysicalModel = RenderPhysicalModelBase<RectangleClip>;
pub type RenderPhysicalShape = RenderPhysicalModelBase<PathClip>;
```

Shared `RenderBox` impl (one body, monomorphized twice):

```rust
impl<C: PhysicalClipSource> RenderBox for RenderPhysicalModelBase<C> {
    type Arity = Single;              // matches RenderClip<S>'s own choice — Flutter's
    type ParentData = BoxParentData;  // nullable `RenderBox? child` maps to Single + has_child,
                                       // NOT Optional arity; consistent with existing precedent.

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

    flui_rendering::forward_single_child_box_queries!(); // unscaled forward-to-child intrinsics —
                                                          // oracle never overrides intrinsics either.

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        if ctx.child_count() == 0 { return; } // oracle :2206-2209 — nothing at all is drawn, no shadow/fill either
        let size = ctx.size();
        let shape = self.clip_source.compute_clip(size);

        if self.elevation != 0.0 {
            ctx.canvas().draw_shadow(&shape.shadow_path(), self.shadow_color, self.elevation);
        }

        let uses_save_layer = self.clip_behavior == Clip::AntiAliasWithSaveLayer;
        let fill_paint = Paint::fill(self.color);
        if !uses_save_layer {
            shape.fill(ctx.canvas(), &fill_paint);
        }

        shape.with_clip_scope(ctx, self.clip_behavior, |ctx| {
            if uses_save_layer {
                ctx.canvas().draw_paint(&fill_paint);
            }
            ctx.paint_child();
        });
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() { return false; }
        // FLUI-wide convention (RenderClip<S>, clip.rs:530-545): always test the
        // shape. Deliberate divergence from oracle for RenderPhysicalModel
        // specifically — see trap §4.2.
        let shape = self.clip_source.compute_clip(ctx.own_size());
        if !shape.contains(Point::new(ctx.x(), ctx.y())) { return false; }
        if self.has_child { ctx.hit_test_child_at_offset(0, Offset::ZERO) } else { false }
    }
}
```

Setters live in two places: shared ones (`set_elevation`, `set_color`, `set_shadow_color`, `set_clip_behavior`, each returning a changed-`bool` matching `RenderClip::set_clip_behavior`'s convention) on `impl<C: PhysicalClipSource> RenderPhysicalModelBase<C>`; variant-specific ones (`set_shape`, `set_border_radius` vs. `set_clipper`) on two separate `impl RenderPhysicalModelBase<RectangleClip> { ... }` / `impl RenderPhysicalModelBase<PathClip> { ... }` blocks — Rust permits inherent impls on a specific monomorphization of a generic struct, so this needs no trait-level plumbing.

`Diagnosticable`: one shared impl reading `C::DIAGNOSTIC_NAME`, `elevation`/`color`/`shadow_color`/`clip_behavior` (correctly, unlike oracle's `shadowColor` bug), then `self.clip_source.debug_fill_extra(builder)`.

## 4. Traps a naive port would fall into

1. **Oracle's own `debugFillProperties` bug** (`proxy_box.dart:2124`, `ColorProperty('shadowColor', color)` — passes `color` twice, never surfaces the real `shadowColor`). A byte-faithful transcription would reproduce this. Don't — surface `self.shadow_color` correctly; this is a confirmed, citeable correction, not an invented improvement.

2. **The hit-test gate is asymmetric between the two classes in the oracle, and both differ from what "always test the shape" gives you.** `RenderPhysicalModel`/`RenderPhysicalShape` both write `if (_clipper != null) { ...test shape... } return super.hitTest(...)` (`:2192-2200`, `:2298-2306`) — inherited from `_RenderCustomClip`'s pattern, not a base-class method (`_RenderCustomClip` itself never defines `hitTest`). Because `RenderPhysicalModel` **never receives a public `clipper`** (§1.3), `_clipper` is always `null` for it in practice, so the gate **never engages** — a circular or rounded-corner `RenderPhysicalModel` (a circular FAB, a rounded Card) hit-tests as its **full rectangular bounding box** in real Flutter, not as the visible shape. `RenderPhysicalShape`, by contrast, always has a non-null clipper (mandatory constructor param), so its gate **always** engages and it genuinely hit-tests against the path. A naive port that "always tests the shape for both, since that's obviously more correct" silently changes `RenderPhysicalModel`'s observable tap behavior relative to the oracle. **Recommendation** (§3's `hit_test` body above): apply the already-shipped `RenderClip<S>` convention (clip.rs:530-545, which also always tests the shape regardless of custom-clipper presence) to *both* variants for FLUI-wide consistency, and document this explicitly as an intentional divergence for `RenderPhysicalModel` specifically (backed by the existing precedent, and arguably fixing a well-known Flutter UX quirk) — not as an accidental "more correct so it doesn't need flagging" choice. `RenderPhysicalShape` needs no such flag since the oracle and the convention agree there.

3. **The `usesSaveLayer` fork controls WHERE the fill is drawn, not just whether it's drawn** (`:2235-2249`, `:2340-2354`). `!usesSaveLayer` draws the fill **before** the clip is pushed, on the parent-layer canvas (`canvas.drawRRect`/`drawPath`); `usesSaveLayer` draws it **inside** the clip scope via `drawPaint` (fills whatever the current clip already restricts to — cheaper and avoids double anti-aliasing the same edge, per the oracle's own comment referencing flutter/flutter#18057). A naive port that always fills outside (or always inside, or always via the shape-specific `fill()` regardless of the fork) either double-paints the fill or produces the exact bleeding-edge artifact this fork exists to avoid. Both branches must be present, each gated on `uses_save_layer`, exactly as sketched in §3.

4. **`BoxShape::Circle`'s formula is an ellipse (`width/2, height/2` as two independent radii), not a true circle** (`proxy_box.dart:2188`, `RRect.fromRectXY(rect, rect.width/2, rect.height/2)`). This is a real, easy-to-miss risk *specifically because FLUI's own `BoxShape` doc comment* (`crates/flui-types/src/layout/box.rs`, on `BoxShape::Circle`) claims *"If the box is not square, the circle will be inscribed in the shorter dimension"* — a **true-circle** description that contradicts the oracle's actual elliptical formula for `RenderPhysicalModel` specifically. That doc comment currently has zero conflicting implementation anywhere in the codebase (grepped `flui-painting`/`flui-objects` — no `BoxShape::Circle` consumer exists yet), so it isn't a live bug to fix, but it is a trap: an implementer who trusts FLUI's own doc comment over the oracle citation will build a `min(width, height)/2` true circle, which is wrong for `RenderPhysicalModel` per `:2188`. Follow the oracle formula exactly; flag the doc-comment mismatch separately (not this task's file to fix) rather than silently "reconciling" the two.

5. **`clip_behavior` defaults to `Clip::None` for both variants, not `Clip::AntiAlias`** — `_RenderCustomClip`'s own default is `Clip.antiAlias` (`:1488`), but `_RenderPhysicalModelBase` overrides it down to `Clip.none` in its own constructor (`:2071`), and neither `RenderPhysicalModel` nor `RenderPhysicalShape` re-overrides it. This is the opposite of `RenderClip<S>`'s default (`clip.rs:476-480`, `Default for RenderClip<S> { fn default() -> Self { Self::anti_alias() } }`) — do not give `RenderPhysicalModelBase<C>` a `Default` impl that reuses that convention; its constructors must take an explicit `Clip::None` as the baseline (matching the oracle), with callers opting into `AntiAlias`/`HardEdge`/`AntiAliasWithSaveLayer` explicitly.

6. **The `elevation` setter's `alwaysNeedsCompositing`/`markNeedsCompositingBitsUpdate` dance is confirmed dead code in the oracle itself** (§1.2) — do not port a "toggle compositing bits on elevation change" mechanism into FLUI; there is nothing for it to toggle (the class never overrides `always_needs_compositing`), and building one anyway would be inventing behavior the oracle itself doesn't have.

7. **`RenderPhysicalModel`'s `shape`/`borderRadius` setters call the equivalent of `_markNeedsClip()` (paint dirty only), not a relayout** — a naive port that routes these through `mark_needs_layout()` (plausible, since "shape changed" sounds layout-adjacent) would force unnecessary re-layouts; the clip shape is purely a paint/hit-test concern here since `size` never depends on `shape`/`border_radius`.

8. **The `BorderRadius → RRect` conversion should NOT call `RRect::clamp_radii()`**, even though that method exists and looks like the "safe" choice. `flui-painting/src/decoration.rs:94-102`'s `decoration_rrect` — the closest existing sibling doing the exact same conversion for `BoxDecoration` — does not call it either, and oracle's own `BorderRadius.toRRect` (`painting/border_radius.dart:441-452`) only clamps individual radii to be non-negative (`clamp(minimum: Radius.zero)`), never the sum-vs-edge-length normalization `RRect::clamp_radii()` performs (a per-corner half-extent cap, which is a *different, more conservative* algorithm than Skia's native proportional corner-overlap scaling, and not equivalent to it for asymmetric radii). Introducing `.clamp_radii()` here — alone, not also applied to `decoration_rrect` — would make `RenderPhysicalModel` inconsistent with its nearest existing sibling for no oracle-backed reason. Match the existing convention (no clamp), and flag the pre-existing `clamp_radii()`/native-Skia-normalization gap as a separate, out-of-scope concern if it ever needs fixing.

## 5. Test plan

Pattern precedent: `crates/flui-objects/tests/render_object_harness.rs:1746-1815`'s `harness_clip_*` block is the structural template (mount via `box_node(...)`, `.child(...)`, `run_layout()`/`run_frame()`, `assert_descendant_properties`), extended with `run.display_commands()` (`harness.rs:564`) for paint-order assertions, which the existing `harness_clip_*` tests don't yet exercise but the facility already supports.

- **Layout pass-through**: `box_geometry(root) == box_geometry(child)` for both variants (single-child proxy, size = child's size) — mirrors `harness_clip_rect_self_describes`.
- **No child**: mount with zero children; assert `run.display_commands()` is empty and `painted()` still reports true-but-blank (or whatever the harness's no-op-paint convention is) — regression test for oracle's `:2206-2209` early return (nothing drawn at all, not even a background fill).
- **Elevation gating**: `elevation == 0.0` → assert no `DrawKind::Shadow` in `display_commands()`; `elevation > 0.0` → assert exactly one `Shadow` entry, with the shadow appearing **before** the fill (`RRect`/`Path`) command and before the child's own commands in `display_commands()`'s ordered output — the direct test for the ordering the task called out, now mechanically checkable via the pre-order `collect_commands` walk (§2).
- **`usesSaveLayer` fork**: with `clip_behavior = Clip::AntiAlias` (not save-layer), assert the fill (`DrawKind::RRect`/`Path`) appears *before* the `DrawKind::Clip` entry; with `clip_behavior = Clip::AntiAliasWithSaveLayer`, assert the fill instead appears *after* the clip entry (i.e. inside the clipped region) and that there is exactly one fill command total in both cases (catches "always draw both" double-fill regressions — trap §4.3).
- **Hit-test**: `RenderPhysicalModel` with `shape = Circle` — a point inside the bounding box but outside the inscribed ellipse misses (per the FLUI-wide "always test shape" convention, §4.2), a point inside the ellipse recurses to the child; a corresponding `RenderPhysicalShape` test with a triangular clipper closure, reusing the `RenderClipPath`-family triangle-path idiom already in `clip.rs`'s own test module. A comment on both tests must cite §4.2's divergence-from-oracle explicitly (this is exactly the kind of behavior a future contributor might "fix" back toward literal oracle parity without realizing it was a deliberate, precedent-backed choice).
- **`BoxShape::Circle` vs `BoxShape::Rectangle`+`BorderRadius`**: assert the computed `RRect` for a non-square box under `Circle` has **two different** corner radii (`rect.width()/2 != rect.height()/2` when width≠height) — the regression test for trap §4.4 (catches a "true circle" mis-port immediately, since a true-circle implementation would produce equal radii).
- **`RenderPhysicalShape` custom clipper**: mirrors `harness_clip_rect_custom_clipper_flag` — assert `custom_clipper` diagnostic flag is true/false correctly, and that the fallback (`Path::new().add_rect(...)`) is used when clipper is absent.
- **Diagnostics**: `assert_descendant_properties` for both, checking `elevation`/`color`/`shadow_color`/`clip_behavior` are all present with correct values (specifically `shadow_color` reads back the actual shadow color, not `color` — the regression test for trap §4.1's oracle-bug correction) plus `shape`/`border_radius` (Model) or `custom_clipper` (Shape).
- **Dry layout/baseline**: not applicable — oracle never overrides `computeDryLayout`/baseline for this family; confirm the `forward_single_child_box_queries!()` macro's default forwarding is sufficient (no new work, matching `RenderClip<S>`'s own "not applicable" note).
- **Catalog guard**: add `"RenderPhysicalModel"` and `"RenderPhysicalShape"` to `RENDER_OBJECT_TYPES` (`crates/flui-objects/tests/render_object_harness.rs:127-...`, alongside the `RenderClipRect`-family rows at `:150-153` for logical grouping), a coverage-table row (`harness_physical_model_*`/`harness_physical_shape_*`), and register the new module in `crates/flui-objects/src/proxy/mod.rs` (`mod physical_model; pub use physical_model::*;`) plus the flat re-export list in `crates/flui-objects/src/lib.rs:58-62`.

## 6. Deferred, documented (not silently dropped)

- **The `PhysicalModel`/`PhysicalShape` plain widgets, and `Material`'s own `AnimatedPhysicalModel`/`_MaterialInterior` wrapper** (`material/material.dart:502-528,927`) — a separate, later widget-layer pass. Note for whoever picks it up: `Material`'s default (rectangular, `MaterialType.canvas`) path animates elevation/color/shape via `AnimatedPhysicalModel` (an implicit-animation widget, architecturally similar to the already-closed `RenderAnimatedSize`'s sibling pattern — a `TweenAnimationBuilder`-style rebuild-driven wrapper, not a persistent render object needing its own `AnimationController` injection), while the custom-shape path builds a plain, non-animated `PhysicalShape` directly and animates it externally. Neither needs new render-tree infrastructure beyond what this plan delivers.
- **`debugDisableShadows`** — confirmed debug/inspector-only (§1.5), zero behavioral weight in a release build; skip entirely unless/until FLUI grows an equivalent golden-test determinism story.
- **`transparentOccluder`** — confirmed inapplicable to FLUI's chosen shadow algorithm (§1.6), not a gap to fill; do not extend `Canvas::draw_shadow`'s signature for this feature.
- **Semantics** (`markNeedsSemanticsUpdate` inside `_markNeedsClip`) — no semantics tree in FLUI yet, consistent with every other catalog entry.
- **`RRect::clamp_radii()` vs. Skia's native proportional corner-overlap normalization** — a pre-existing, unaddressed divergence shared with `flui-painting`'s `decoration_rrect` (§4.8); out of scope for this feature specifically, not introduced by it.

### Critical Files for Implementation
- `crates/flui-objects/src/proxy/physical_model.rs` (new — `PhysicalClipShape`, `PhysicalClipSource`, `RectangleClip`, `PathClip`, `RenderPhysicalModelBase<C>`, `RenderPhysicalModel`, `RenderPhysicalShape`)
- `crates/flui-objects/src/proxy/clip.rs` (existing — the `RenderClip<S: ClipGeometry>` generic-collapse *pattern* this design follows, and the already-shipped "always test shape" hit-test convention this design deliberately reuses for `RenderPhysicalModel`, §4.2)
- `crates/flui-painting/src/decoration.rs` (existing `decoration_rrect` helper, `:94-102` — the exact `BorderRadius → RRect` conversion precedent to model `RectangleClip::compute_clip` on, including its no-`clamp_radii` convention)
- `crates/flui-painting/src/canvas/drawing.rs` (`draw_shadow`, `draw_rrect`, `draw_path`, `draw_paint` — confirms `draw_shadow`'s real 3-arg signature, no `transparent_occluder` parameter)
- `crates/flui-rendering/src/context/paint_cx.rs` (`PaintCx::with_clip_rrect`/`with_clip_path`, `paint_child`, `canvas()`)
- `crates/flui-objects/tests/render_object_harness.rs` (catalog registration; `harness_clip_*` at `:1746-1815` is the structural template; `FrameRun::display_commands()`/`DrawKind` for the paint-order assertions)
