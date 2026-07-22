# RenderFlow + Flow — plan (oracle-verified)

Core.2 catalog closure #2 in the delegate family (after `CustomPaint`). `FlowDelegate` exists (`crates/flui-rendering/src/delegates/flow_delegate.rs`, gated `experimental-delegates`) but is **not wired to anything real** — its `paint_child` silently drops the transform it's handed (`_transform: Matrix4` — underscore-prefixed, unused). Fixing that wiring, not the trait signature, is the whole plan.

## Headline: paint-side has NO per-child-transform primitive; hit-test-side already does

FLUI's paint pipeline can apply a transform to a **whole node** (`RenderObject::paint_transform(size) -> Option<Matrix4>`, read once per node in `paint_subtree_impl`, `crates/flui-rendering/src/pipeline/owner/paint.rs:170,252-271`) — that's how `RenderTransform`/`RotatedBox` work (node == the one transformed subtree). `RenderFlow` needs **N different transforms for N sibling children of the same Variable-arity node**, chosen by the delegate at *paint* time. There is no such primitive: `FragmentOp::Child` only carries `offset_override: Option<Offset>` (`crates/flui-rendering/src/context/paint_cx.rs:70-77`), and `PaintCx` has no `with_transform` (only `with_clip_rect`/`with_clip_rrect`/`with_clip_path`, same file L339-377). **This plan requires a small, new, additive pipeline primitive** (§2) before RenderFlow can be written.

Hit-testing is the opposite story and is *already solved*: `BoxHitTestContext`/`HitTestContext` already expose `push_transform`/`pop_transform`/`with_transform` publicly to arity-agnostic render-object code (`crates/flui-rendering/src/context/hit_test.rs:157-194`, backed by `crates/flui-rendering/src/protocol/box_protocol.rs:1461-1478`), completely independent of the per-node `hit_test_transform()` hook. `RenderFlow::hit_test` can call `ctx.with_transform(matrix, |ctx| ctx.hit_test_child(idx, local_pos))` directly — no pipeline change needed there.

The second-order wrinkle: Flutter's `RenderFlow` **mutates** `FlowParentData._transform` and `_lastPaintOrder` *during* `paint()` and reads them back in `hitTestChildren()` — legal because Dart's `paint()` isn't const. FLUI's `RenderBox::paint(&self, …)` and `hit_test(&self, …)` are both `&self` (immutable) — there is no way to cache "what transform did paint assign to child 3" for hit-test to read back later. **Resolution: `hit_test` re-invokes `delegate.paint_children()` a second time** against a non-drawing "replay" context that only records `(index → transform)`, instead of caching state (§3). This is legitimate because `FlowDelegate::paint_children` is contractually a pure function of the delegate's own state + child sizes (the same assumption `should_repaint`/`should_relayout` already rely on) — it costs one extra O(children) delegate call per hit-test, not per frame.

## 1. Oracle (`.flutter/flutter-master/packages/flutter/lib/src/rendering/flow.dart`)

- `FlowPaintingContext` (abstract, L29-50): `size`, `childCount`, `getChildSize(i) -> Size?` (null on OOB), `paintChild(i, {Matrix4 transform, double opacity = 1.0})`.
- `FlowDelegate` (L64-142): ctor takes `Listenable? repaint` (L66-68, **deferred**, no FLUI plumbing — see §7). `getSize` defaults to `constraints.biggest` (L80); `getConstraintsForChild` defaults to identity (`constraints`, L95); `paintChildren` abstract (L113); `shouldRelayout` defaults `false` (L119); `shouldRepaint` **abstract, no default** (L134).
- `FlowParentData` (L144-153): only adds `Matrix4? _transform` (private cache; **not ported** — see §4's ParentData decision).
- `RenderFlow` class doc (L155-178): "rather than positioning children during layout, children are positioned using transformation matrices during paint… children are thus repositioned by repainting, skipping layout."
- Ctor + `setupParentData` (L184-205): resets `_transform = null` on reused parent data.
- `delegate` setter (L216-234): `if newDelegate.runtimeType != oldDelegate.runtimeType || newDelegate.shouldRelayout(oldDelegate) → markNeedsLayout(); else if newDelegate.shouldRepaint(oldDelegate) → markNeedsPaint()`. **Note the explicit `runtimeType` check** — unlike `CustomPaint`'s `_didUpdatePainter` (which has no such check), Flow's setter does NOT rely solely on the delegate's own `should*` methods to detect a type swap. Also wires `oldDelegate._repaint`/`newDelegate._repaint` listeners (L230-233, deferred).
- `clipBehavior` (L236-247, default `Clip.hardEdge`): setter marks needs-paint + needs-semantics-update.
- `isRepaintBoundary => true` unconditionally (L266-267) — doc explicitly recommends children also be repaint boundaries for best perf.
- Intrinsics (L269-307): author's own TODO calls this "dubious" — all four dimensions are `_getSize(BoxConstraints.tightForFinite(...)).<axis>`, finite-or-zero, **never touching children**. `tightForFinite` leaves the *other* axis at `double.infinity` by default.
- `_getSize` (L261-264): `constraints.constrain(delegate.getSize(constraints))` — the ONE sizing formula reused by `performLayout`, `computeDryLayout`, and all four intrinsics.
- `performLayout` (L315-331): `size = _getSize(constraints)`; for each child in order: `inner = delegate.getConstraintsForChild(i, constraints)`; `child.layout(inner, parentUsesSize: true)`; **`childParentData.offset = Offset.zero`** (L327 — children are NEVER positioned by layout, only by paint-time transform).
- `_randomAccessChildren` (L334, rebuilt every layout) / `_lastPaintOrder` (L337, rebuilt every paint) — Flutter-linked-list accommodations; **not needed in FLUI** (index-based child access is already native).
- `getChildSize` (L344-349): null on OOB (FLUI's existing stub panics instead — minor, non-blocking divergence, see §3).
- `paintChild` (L352-389): asserts no double-paint per child (**FLUI's stub currently has NO such assert** — worth adding, §3); `opacity == 0.0` still assigns `_transform` (so hit-testing still finds the child) but paints nothing; `opacity == 1.0` → `pushTransform`; else → `pushOpacity` wrapping `pushTransform`. **Opacity param scoped out of this plan** (see §7) — FLUI's redesigned context only takes `transform`, matching the *existing* FLUI trait signature (`fn paint_child(&self, index: usize, transform: Matrix4)` has no opacity param already, so this is not a new cut, just a pre-existing one worth naming).
- `_paintWithDelegate` (L391-405): clears `_lastPaintOrder` and every child's `_transform` **before** calling `delegate.paintChildren(this)` — i.e. a fresh paint pass has no memory of the last one. FLUI's replay-based redesign gets this for free (each `paint_children()` call builds fresh `paint_order`/`transforms` buffers).
- `paint` (L407-417): unconditionally wraps `_paintWithDelegate` in `context.pushClipRect(needsCompositing, offset, Offset.zero & size, …, clipBehavior: clipBehavior)`.
- `hitTestChildren` (L427-453): iterate `_lastPaintOrder` **in reverse** (top-most-painted-first); skip if `childIndex >= children.length` or `transform == null`; `result.addWithPaintTransform(transform: transform, position: position, hitTest: (result, position) => child.hitTest(result, position: position))` — internally inverts `transform`, maps `position`, pushes `transform` onto the result's transform stack, recurses.
- `applyPaintTransform` (L455-462): multiplies `childParentData._transform` into the transform when a caller asks "what's the transform from Flow to this specific descendant" (backs `RenderBox.getTransformTo`/`localToGlobal`). **FLUI has no such hook wired at all** — `RenderBox::local_to_global`/`global_to_local` are stub identity functions today (`crates/flui-rendering/src/traits/render_box.rs:192-199`). Deferred, matches existing FLUI-wide gap, not Flow-specific (§7).

## 2. NEW pipeline primitive: `PaintCx::with_transform` (build first — everything else depends on it)

`crates/flui-rendering/src/context/paint_cx.rs`:
- Add `FragmentOp::PushTransform(Box<Matrix4>)` alongside the existing `Push(Box<FragmentClip>)` (box it — `Matrix4` is `[f32;16]` = 64 bytes vs. `Push`'s 8-byte pointer; an unboxed variant would bloat `FragmentOp`'s size for every render object's every paint, not just Flow's). `Pop` is already generic/untyped and closes whichever scope kind is innermost — no change needed there.
- `FragmentRecorder`: add `fn push_transform_scope(&mut self, transform: Matrix4)` mirroring `push_scope` (`seal(); ops.push(FragmentOp::PushTransform(Box::new(transform))); open_scopes += 1;`). Reuse the existing `pop_scope()` as-is.
- `PaintCx<'a, A: Arity>` (the **generic** impl block, same one hosting `with_clip_rect` — not arity-restricted, since a per-child transform is a general capability): add
  ```rust
  pub fn with_transform(&mut self, transform: Matrix4, f: impl FnOnce(&mut Self)) {
      self.rec.push_transform_scope(transform);
      f(self);
      self.rec.pop_scope();
  }
  ```
  Add `Matrix4` to the file's `flui_types::{…}` import list.
- `crates/flui-rendering/src/pipeline/owner/paint.rs`: add a match arm in the `FragmentOp` loop (~L273-322):
  ```rust
  FragmentOp::PushTransform(matrix) => {
      let effective = conjugate(*matrix, origin); // see below
      composer.push_layer(Layer::Transform(TransformLayer::new(effective)));
  }
  ```
  **Extract the existing conjugation math** (currently inlined at L252-267 for the per-node hook: `T(origin) * matrix * T(-origin)`, "so the matrix pivots around the node's own origin instead of the layer origin") into a small private `fn conjugate(matrix: Matrix4, origin: Offset) -> Matrix4` and call it from **both** sites (the per-node hook and the new per-child op) — avoids duplicating non-trivial math, and `origin` is *already in scope* in `paint_subtree_impl` (it's the Flow node's own accumulated layer-space position — exactly Flutter's `_paintingOffset` semantics, so **no extra plumbing is needed to get "the ambient origin to conjugate around" — it's the same variable the per-node hook already uses**).
  - `Pop` handling (`composer.pop_layer()`) is unchanged and already correct for any layer kind on top of the composer's stack.

This is the single cross-cutting change in the plan (touches `flui-rendering` pipeline internals, not just `flui-objects`). Everything else is additive/local.

## 3. `FlowDelegate` + `FlowPaintingContext` redesign (`crates/flui-rendering/src/delegates/flow_delegate.rs`)

**No load-bearing signature mismatch on the trait methods themselves** (unlike CustomPaint's `hit_test: bool` vs `bool?`). The five methods (`get_size`, `get_constraints_for_child`, `paint_children`, `should_relayout`, `should_repaint`) already match the oracle's *types* exactly. Two real gaps, one cosmetic:

1. **Load-bearing bug (must fix):** `FlowPaintingContext::paint_child`'s `_transform: Matrix4` parameter is discarded (only a `painted: Vec<bool>` flag is recorded — see current file L178-182, `// In real implementation, this would paint the child with the transform`). No transform ever escapes the context today; a real `RenderFlow` cannot be built on this as-is.
2. **Cosmetic parity gaps (optional, low severity, worth doing while touching the file):** oracle gives `getSize`/`getConstraintsForChild`/`shouldRelayout` default bodies (`constraints.biggest()`, identity, `false` respectively); FLUI's trait requires every impl to write these out. Add matching `fn … { … }` defaults — purely additive, zero behavior change for existing impls (the two test delegates in this file already override all of them, so this doesn't even touch existing test code).
3. `getChildSize` returns `Size?` (null on OOB) in the oracle; FLUI's `child_size` panics on OOB. Leave as-is (documented, non-blocking — RenderFlow will only ever call it with in-range indices).

**Redesign `FlowPaintingContext`** to actually carry the transform out, using an `Option<&mut PaintCx>` rather than a new trait object (fewer moving parts):

```rust
pub struct FlowPaintingContext<'ctx, 'cx> {
    size: Size,
    child_sizes: &'ctx [Size],
    live: Option<&'ctx mut PaintCx<'cx, Variable>>,   // None during hit-test replay
    paint_order: &'ctx mut Vec<usize>,                 // Flutter's _lastPaintOrder
    transforms: &'ctx mut Vec<Option<Matrix4>>,        // always recorded, both modes
    painted: &'ctx mut Vec<bool>,                       // existing dup-paint guard, now actually asserted
}
```
- `pub fn for_paint(ctx: &'ctx mut PaintCx<'cx, Variable>, child_sizes: &'ctx [Size], paint_order: &'ctx mut Vec<usize>, transforms: &'ctx mut Vec<Option<Matrix4>>, painted: &'ctx mut Vec<bool>) -> Self` — capture `let size = ctx.size();` **before** moving `ctx` into `live: Some(ctx)` (ordering matters, `ctx.size()` borrows immutably first).
- `pub fn for_replay(size: Size, child_sizes: …, paint_order: …, transforms: …, painted: …) -> Self` — `live: None`.
- `paint_child(&mut self, index: usize, transform: Matrix4)`: `assert!(index < child_sizes.len())`; **add** `assert!(!self.painted[index], "paint_child called twice for child {index}")` (oracle L356-365, currently missing entirely — the existing code sets the flag unconditionally with no guard); `self.painted[index] = true; self.paint_order.push(index); self.transforms[index] = Some(transform);` then, only if `live` is `Some`: `ctx.with_transform(transform, |ctx| ctx.paint_child(index))`.
- `child_count`/`child_size`/`all_children_painted` unchanged.
- Trait signature becomes `fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>)` (two elided lifetimes instead of one — mechanical, one-line change in the trait + the two existing test delegates in this file's `#[cfg(test)]` module).
- Because delegate authors only ever *receive* `&mut FlowPaintingContext`, never construct one, this is **source-compatible for delegate implementors** — only `RenderFlow` (the sole production caller, being added by this same plan) needs the new constructors.

**Un-gate**: move `flow_delegate` out of `#[cfg(feature = "experimental-delegates")]` in `crates/flui-rendering/src/delegates/mod.rs` (mirror the exact diff shape of commit `6965fc36`'s `custom_painter` move), update the module doc table and `crates/flui-rendering/Cargo.toml`'s feature comment (now three remaining gated modules: `multi_child_layout_delegate.rs`, `single_child_layout_delegate.rs`, `custom_clipper.rs`), update `crates/flui-rendering/AGENTS.md`'s one-line feature description, and add an amendment to `docs/adr/ADR-0007-experimental-delegates-sunset.md` following the exact structure of the "Amendment (2026-07-01): `CustomPainter` un-gated" section already there.

## 4. `RenderFlow` (`crates/flui-objects/src/layout/flow.rs`)

Placement: `layout/`, not `proxy/` — `RenderFlow` is Variable-arity (like `RenderStack`/`RenderListBody`), not a Single-child proxy like `RenderCustomPaint`. `RenderListBody` is the closest precedent for "Variable-arity multi-child primitive living in `layout/` despite being a plain module doc that says 'position a single child'" (that doc comment is already stale for `ListBody`).

```rust
pub struct RenderFlow {
    delegate: Arc<dyn FlowDelegate>,
    clip_behavior: Clip,               // default Clip::HardEdge (oracle default)
    child_sizes: Vec<Size>,            // cached during perform_layout; read by paint() + hit_test()
}
```
- `type Arity = Variable; type ParentData = BoxParentData;` — **no custom `FlowParentData`.** Flutter's `FlowParentData` exists solely to cache `_transform` for later hit-test replay; FLUI's design replays the delegate call instead (§3), so there is nothing to cache on parent data. State it explicitly as a deliberate simplification, not an oversight.
- Private helper `fn get_size(&self, constraints: BoxConstraints) -> Size { constraints.constrain(self.delegate.get_size(constraints)) }` — the oracle's `_getSize` (L261-264), single source of truth reused by layout, dry layout, and all four intrinsics.
- `perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size`:
  ```rust
  let constraints = *ctx.constraints();
  let size = self.get_size(constraints);
  let n = ctx.child_count();
  self.child_sizes.clear();
  self.child_sizes.reserve(n);
  for i in 0..n {
      let inner = self.delegate.get_constraints_for_child(i, constraints);
      let child_size = ctx.layout_child(i, inner);
      ctx.position_child(i, Offset::ZERO);      // oracle L327 — always zero, position is paint-time only
      self.child_sizes.push(child_size);
  }
  size
  ```
- `compute_dry_layout(&self, constraints, _ctx) -> Size { self.get_size(constraints) }` — **children are never touched**, matches oracle L311-313 exactly (`return _getSize(constraints)`).
- Four intrinsics (also never touch children, per oracle's own "dubious" TODO L269-271, ported as-is):
  ```rust
  fn compute_min_intrinsic_width(&self, height: f32, _ctx) -> f32 {
      let w = self.get_size(BoxConstraints::tight_for_finite(Pixels::INFINITY, Pixels::new(height))).width;
      if w.is_finite() { w.get() } else { 0.0 }
  }
  // compute_max_intrinsic_width: identical body (oracle reuses the same formula for min AND max, L273-289)
  // compute_min/max_intrinsic_height: swap axes — tight_for_finite(Pixels::new(width), Pixels::INFINITY), .height
  ```
  Note: FLUI's `tight_for_finite(width: Pixels, height: Pixels)` takes both positionally (no named-param infinity default like Dart's), so the non-constrained axis must be passed `Pixels::INFINITY` explicitly.
- `is_repaint_boundary(&self) -> bool { true }` — oracle L266-267, unconditional. One existing FLUI precedent to copy from: `crates/flui-objects/src/proxy/repaint_boundary.rs:109`.
- `paint(&self, ctx: &mut PaintCx<'_, Variable>)`:
  ```rust
  let bounds = Rect::from_origin_size(Point::ZERO, ctx.size());
  let body = |ctx: &mut PaintCx<'_, Variable>| {
      let n = self.child_sizes.len();
      let (mut painted, mut paint_order, mut transforms) = (vec![false; n], Vec::with_capacity(n), vec![None; n]);
      let size = ctx.size();
      let mut flow_ctx = FlowPaintingContext::for_paint(ctx, &self.child_sizes, &mut paint_order, &mut transforms, &mut painted);
      let _ = size; // for_paint reads ctx.size() itself before moving ctx — see §3 ordering note
      self.delegate.paint_children(&mut flow_ctx);
  };
  if self.clip_behavior != Clip::None {
      ctx.with_clip_rect(bounds, self.clip_behavior, body);
  } else {
      body(ctx);
  }
  ```
  (Clip-gating on `!= Clip::None` mirrors `RenderStack`'s established FLUI idiom rather than Flutter's unconditional `pushClipRect` call — same visible result, fewer emitted layers when `Clip::None`.)
- `hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool`:
  ```rust
  if !ctx.is_within_own_size() { return false; }
  let n = self.child_sizes.len();
  let (mut painted, mut paint_order, mut transforms) = (vec![false; n], Vec::with_capacity(n), vec![None; n]);
  let mut flow_ctx = FlowPaintingContext::for_replay(ctx.own_size(), &self.child_sizes, &mut paint_order, &mut transforms, &mut painted);
  self.delegate.paint_children(&mut flow_ctx);   // side-effect-free replay of the SAME call paint() made

  let position = *ctx.position();
  for &index in paint_order.iter().rev() {                // oracle L430: reverse = top-most-painted-first
      let Some(transform) = transforms[index] else { continue };
      let Some(inverse) = transform.try_inverse() else { continue }; // degenerate matrix ⇒ nothing visible (RenderTransform parity)
      let (lx, ly) = inverse.transform_point(position.dx, position.dy);
      if ctx.with_transform(transform, |ctx| ctx.hit_test_child(index, Offset::new(lx, ly))) {
          return true;
      }
  }
  false
  ```
  `ctx.hit_test_child(index, position)` (not `hit_test_child_at_offset`) is the right primitive — it treats `position` as *already* the final child-local point (confirmed at `crates/flui-rendering/src/protocol/box_protocol.rs:1447-1452`, `hit_test_child` forwards straight to the child callback with `Some(position)`, no further subtraction). `with_transform` only affects the `HitTestResult` transform-stack bookkeeping (`crates/flui-rendering/src/protocol/box_protocol.rs:1461-1478`) used if the child itself calls `add_self_with_transform` — it does not alter recursion.
- `set_delegate(&mut self, delegate: Arc<dyn FlowDelegate>) -> DelegateChange` — new small enum (no existing precedent to reuse; CustomPaint's binary `bool` isn't expressive enough for Flow's 3-way oracle contract):
  ```rust
  pub enum DelegateChange { None, Repaint, Relayout }
  ```
  Body: oracle's setter (L216-234) does an explicit `runtimeType` check *in addition to* `should_relayout`/`should_repaint` (unlike CustomPaint's `_didUpdatePainter`, which has no such check) — reproduce via `old.as_any().type_id() != new_delegate.as_any().type_id()`:
  ```rust
  let type_changed = self.delegate.as_any().type_id() != delegate.as_any().type_id();
  let relayout = type_changed || delegate.should_relayout(&*self.delegate);
  let repaint = !relayout && delegate.should_repaint(&*self.delegate);
  self.delegate = delegate;
  if relayout { DelegateChange::Relayout } else if repaint { DelegateChange::Repaint } else { DelegateChange::None }
  ```
- `set_clip_behavior(&mut self, clip_behavior: Clip) -> bool` — standard changed-flag setter (matches `RenderStack::set_clip_behavior`).
- `Diagnosticable`: `builder.add_enum("clip_behavior", self.clip_behavior)` only — Flutter's own `RenderFlow` doesn't override `debugFillProperties` at all (no delegate info surfaced), so there's nothing else to add.

## 5. `Flow` widget (`crates/flui-widgets/src/layout/flow.rs`)

Placement: `layout/`, registered in `layout/mod.rs` alongside `ListBody` — not `paint/` (that module's doc explicitly scopes to "thin `RenderView` over a `flui-objects` proxy," i.e. Single-arity; `Flow` is Variable/multi-child like `Stack`). Model directly on `crates/flui-widgets/src/stack/stack.rs`'s `Stack<C = Vec<BoxedView>>` shape (generic `ViewSeq`, `generic_render_view_element!` macro), not on `CustomPaint`'s single-`Child` shape:

```rust
pub struct Flow<C = Vec<BoxedView>> {
    delegate: Arc<dyn FlowDelegate>,
    clip_behavior: Clip,     // default Clip::HardEdge
    children: C,
}
impl<C> Flow<C> {
    pub fn new(delegate: Arc<dyn FlowDelegate>, children: C) -> Self { … }
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self { … }
}
impl<C: ViewSeq + Clone + Send + Sync + 'static> RenderView for Flow<C> {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlow;
    fn create_render_object(&self) -> RenderFlow { RenderFlow::new(self.delegate.clone()).with_clip_behavior(self.clip_behavior) }
    fn update_render_object(&self, ro: &mut RenderFlow) { ro.set_delegate(self.delegate.clone()); ro.set_clip_behavior(self.clip_behavior); }
    fn has_children(&self) -> bool { !self.children.is_empty() }
    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) { self.children.for_each(|_i, c| visitor(c)); }
}
generic_render_view_element!(Flow);
```
Note: `update_render_object` discards `set_delegate`'s `DelegateChange` return today — same "framework marks paint/layout unconditionally, future-proofing only" caveat CustomPaint's plan already accepted for `set_painter`'s bool.

## 6. Catalog guard (MANDATORY or CI red)

`crates/flui-objects/tests/render_object_harness.rs`: add `"RenderFlow"` to `RENDER_OBJECT_TYPES` (~L133, after `RenderListBody`) and the module-doc coverage table row (~L36): `| RenderFlow | harness_flow_* | yes | yes | yes | yes | order |`. Add `harness_flow_*` tests (both guards checked by `catalog_covers_every_render_object_name`).

## 7. Tests

- **Unit (delegate, in `flow_delegate.rs`):** `paint_child` records the transform (not just a bool) in both `for_paint`- and `for_replay`-style harnesses; duplicate `paint_child(i, …)` panics (new assert); the two existing `LinearFlowDelegate`/`should_relayout` tests keep passing unmodified.
- **Unit (RO, in `flow.rs`):** `set_delegate` returns `Relayout` on type change, `Relayout`/`Repaint`/`None` per `should_relayout`/`should_repaint` on same-type swap (mirrors CustomPaint's `set_painter` test shape); `get_size` formula (`constrain(delegate.get_size(…))`) via dry-layout harness; intrinsics finite/infinite-guard; childless flow sizes via `get_size` alone.
- **Harness (`harness_flow_*`):** a delegate that lays out fixed-size children on a line via `Matrix4::translation` (like the existing `LinearFlowDelegate` test fixture) mounted with 3 colored children:
  - **paint order via real display-command inspection** (matching CustomPaint's rigor, not size-only): assert `run.display_commands()` shows 3 `DrawRect`s in the delegate's chosen order/colors, wrapped in a `TransformLayer`/structural marker per child (`run.structure()` should show `Transform` layers per child, not one shared layer) — proves per-child (not per-node) transform layers are actually emitted.
  - **hit-test-replays-the-real-transform**: a delegate that assigns child `i` a translation of `i * 50px`; assert `run.hit_first(x, y)` resolves to the correct child at each translated position, AND assert a position that falls between two *untranslated* bounding boxes but *inside* the translated position of child 1 hits child 1 — proves the inverse-transform hit-test, not a bounding-box approximation.
  - **paint-order-reversed-for-hit-test**: two overlapping children painted in order [0, 1] (1 on top); assert the overlapping point hits child 1 (reverse-order-first, oracle L430).
  - **degenerate transform**: a delegate returning a zero-scale (non-invertible) matrix for one child; assert that child is never hit but others still are.
  - **clip_behavior gating**: `Clip::None` vs `Clip::HardEdge` — assert presence/absence of a `Clip` layer in `run.structure()`.
- **Parity (`tests/parity/flow_test.rs`):** layout-size parity (`get_size`/`get_constraints_for_child` formulas); a two-child delegate with distinct transforms, comparing FLUI's final composited child positions against hand-computed Flutter-equivalent expectations.

## Risk ranking

- **HIGH** — the new `PaintCx::with_transform`/`FragmentOp::PushTransform` pipeline primitive (§2): the only change in this plan outside `flui-objects`/`flui-widgets`/the already-gated delegate module; touches shared paint-replay code every render object goes through. Get the conjugation-around-`origin` formula right (reuse, don't reimplement, the existing L252-267 math) and get the enum-size boxing right.
- **HIGH** — the `&self`-paint / `&self`-hit_test replay design (§3/§4): if the "delegate is a pure function of size + own state" assumption is ever violated by a real user delegate (e.g. one that consults external mutable state or randomness), paint and hit-test will silently disagree. Worth a doc-comment warning on `FlowDelegate::paint_children` mirroring Flutter's own (implicit) purity assumption.
- **MED** — `FlowPaintingContext`'s lifetime redesign (`<'ctx, 'cx>`, `Option<&mut PaintCx>`) — mechanical but easy to get the `ctx.size()`-before-move ordering wrong; low blast radius (trait signature change is source-compatible for delegate authors).
- **MED** — `DelegateChange`'s explicit `type_id()` check in `set_delegate` — easy to skip (defaulting to CustomPaint's simpler pattern) and silently under-relayout on delegate type swaps; must be checked against the oracle's L223 explicitly, not inferred from CustomPaint's precedent.
- **LOW** — `RenderFlow` layout/dry-layout/intrinsics (pure formula ports, no per-child edge cases); widget scaffolding (direct `Stack<C>` clone); catalog guard.

## Deferred, documented (out of scope, matching CustomPaint's precedent)

- `FlowDelegate`'s `Listenable? repaint` constructor arg and the `attach`/`detach` listener wiring (oracle L64-68, L230-233, L249-259) — no FLUI-side `Listenable`/`Animation` plumbing exists for render objects yet (same gap CustomPaint already documented for its own painter).
- `FlowPaintingContext.paintChild`'s `opacity` parameter (oracle L352, `pushOpacity` wrapping) — FLUI's *existing* `FlowDelegate::paint_children`/`paint_child(index, transform)` signature has no opacity parameter already (pre-existing scope cut, not new).
- `RenderObject.applyPaintTransform`/`getTransformTo`/`localToGlobal` (oracle L455-462) — `RenderBox::local_to_global`/`global_to_local` are stub identity functions everywhere in FLUI today (`render_box.rs:192-199`), not a Flow-specific gap.
- `markNeedsSemanticsUpdate` on `clipBehavior` change (oracle L245) — FLUI has no semantics tree yet (consistent with every other render object in the catalog).

### Critical Files for Implementation
- crates/flui-rendering/src/context/paint_cx.rs
- crates/flui-rendering/src/pipeline/owner/paint.rs
- crates/flui-rendering/src/delegates/flow_delegate.rs
- crates/flui-rendering/src/context/hit_test.rs
- crates/flui-objects/src/layout/stack.rs (closest Variable-arity precedent to model `RenderFlow` on)
