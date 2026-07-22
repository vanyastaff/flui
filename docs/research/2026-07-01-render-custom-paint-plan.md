# RenderCustomPaint + CustomPaint — plan (oracle-verified)

Business.1 catalog. Enables `CustomPaint` (custom graphics). No ADR — a leaf-ish
proxy RO with direct precedent (`RenderDecoratedBox`).

## Headline: the CustomPainter delegate EXISTS; one trait-signature fix is the risk
`crates/flui-rendering/src/delegates/custom_painter.rs` (gated `experimental-delegates`)
matches the oracle EXCEPT: `hit_test` returns `bool`, but Flutter's is `bool?`
(`Option<bool>`) — the tri-state (`None` = "use default") is load-bearing for the
RO's hit-test (`hitTestSelf` default = HIT, `hitTestChildren` foreground default =
MISS). **Fix first: `fn hit_test(&self, _p: Offset) -> Option<bool> { None }`.** No
`Listenable`/repaint wiring (acceptable scope cut — the framework marks needs-paint
unconditionally today; document as follow-up).

## 1. Oracle (`.flutter/.../rendering/custom_paint.dart`)
`RenderCustomPaint extends RenderProxyBox`. CustomPainter: `paint(Canvas,Size)`,
`shouldRepaint(old)->bool`, `hitTest(Offset)->bool?` (null default). Size: to child
if present, else `constraints.constrain(preferredSize)` (computeSizeForNoChild L579).
Paint (L639): bg painter (if any, save/restore + assert-save-count) → child (super)
→ fg painter. Repaint gate `_didUpdatePainter` L450: None→Some or Some→None → repaint;
Some→Some → `new.shouldRepaint(old)`. Hit-test order fg → child → bg (RenderBox.hitTest
= hitTestChildren||hitTestSelf; hitTestSelf = `painter != null && (painter.hitTest(p) ?? true)`;
hitTestChildren consults foreground first with `?? false`). Intrinsics: no child →
preferredSize.<axis> if finite else 0; with child → forward.

## 2. RenderCustomPaint (`crates/flui-objects/src/proxy/custom_paint.rs`)
`type Arity = Single;` (matches DecoratedBox/Opacity/Clip family — PaintCx<Single>::
paint_child self-guards child_count>0). Fields: `painter: Option<Arc<dyn CustomPainter>>`,
`foreground_painter: Option<Arc<dyn CustomPainter>>`, `preferred_size: Size`,
`is_complex`/`will_change` (carry, hint calls = TODO — no FLUI set_is_complex_hint yet).
- perform_layout (HAND-WRITTEN, NOT forward_single_child_box_queries! — its no-child
  branch returns smallest(), wrong): child_count>0 → layout_child(0,c)+position_child(0,ZERO);
  else c.constrain(preferred_size).
- compute_dry_layout: child → child_dry_layout(0,c); else c.constrain(preferred_size).
- intrinsics: no child → preferred_size.<axis>.get() if finite else 0; child → forward.
- paint: `paint_with_painter(canvas,size,bg)` → `ctx.paint_child()` → `..(fg)`. Helper:
  `save(); n=save_count(); p.paint(canvas,size); debug_assert_eq!(save_count(),n); restore();`
  (no offset translate — recorder pre-translates to local origin; cf RenderColoredBox).
- hit_test: `!is_within_own_size → false`; `fg.hit_test(pos).unwrap_or(false) → true`;
  `child hit → true`; `bg.hit_test(pos).unwrap_or(true)`; else false.
- set_painter/set_foreground_painter(new) -> bool: None↔Some → true; Some→Some →
  new.should_repaint(&**old); return change flag (framework marks paint unconditionally
  today — future-proofing, mirror set_grid_delegate discarding its flag). set_preferred_size.
Export from flui-objects/src/lib.rs + proxy/mod.rs.

## 3. CustomPainter delegate un-gate
`delegates/mod.rs`: move `custom_painter` out of the `#[cfg(feature="experimental-delegates")]`
block (leave custom_clipper/flow/multi_child/single_child gated). Re-export `CustomPainter`
from flui-widgets/src/lib.rs alongside SliverGridDelegate. Fix `hit_test → Option<bool>`.

## 4. CustomPaint widget (`crates/flui-widgets/src/paint/custom_paint.rs`)
RenderView (model on paint/decorated_box.rs + scroll/sliver_grid.rs Arc carrier):
struct { painter, foreground_painter, size (default ZERO), child: Child }. Builder:
new()/.painter(Arc<dyn CustomPainter>)/.foreground_painter(..)/.size(Size)/.child(impl IntoView).
create_render_object → RenderCustomPaint::new(...); update_render_object → set_painter/
set_foreground_painter/set_preferred_size. Register in paint/mod.rs + lib.rs re-export.

## 5. Catalog guard (MANDATORY or CI red)
Add "RenderCustomPaint" to RENDER_OBJECT_TYPES + a `harness_custom_paint_*` test in
crates/flui-objects/tests/render_object_harness.rs (both guards checked).

## 6. Tests
- Unit (RO): set_painter gating (None→Some, Some→Some same-value should_repaint=false,
  type-change) returns correct bool; layout sizes to child / constrain(preferred); dry both;
  intrinsics no-child finite-guard.
- Harness (harness_custom_paint_*): a SpyPainter capturing invocation+size via
  Arc<Mutex<Option<Size>>>; mount RenderCustomPaint(Some(spy),None,ZERO) .with_size(100×100)
  .run_frame(); assert run.painted() AND captured size == 100×100. Variants: paint-order
  (bg+fg+child spies record order), hit-test (fg Some(true) short-circuits, bg None→hit).
- Parity (tests/parity/custom_paint_test.rs, register in main.rs): layout-size parity +
  (if harness records canvas ops) a draw_rect emitted; child sizes to child.

## Risk: HIGH — the Option<bool> hit_test fix (public feature-gated trait, do first w/ un-gate);
MED — hand-written layout (no-child constrain(preferred), not the macro smallest()); MED —
save/restore assert discipline; LOW — RO scaffolding (clone DecoratedBox+SliverGrid), widget,
catalog guard. Deferred (document): Listenable repaint, semanticsBuilder, isComplex/willChange hints.
