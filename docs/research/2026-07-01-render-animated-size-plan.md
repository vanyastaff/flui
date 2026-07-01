# `RenderAnimatedSize` + `AnimatedSize` — plan (oracle-verified)

Core.2 Slice C of ADR-0013 (`docs/adr/ADR-0013-render-object-attach-self-dirty-handle.md`). Slices A/B are implemented: `RepaintHandle::mark_needs_layout` (`crates/flui-rendering/src/pipeline/handle.rs:261-264`), and defaulted `attach(&mut self, RepaintHandle)`/`detach(&mut self)` on `RenderObject<P>`/`RenderBox`/`RenderSliver` (`crates/flui-rendering/src/traits/render_object.rs:568-585`, forwarded at `render_box.rs:469-478,733-737`), wired by the pipeline's insert/remove paths and proven by `crates/flui-rendering/tests/attach_detach_lifecycle.rs`. This plan is the deferred DEV task the ADR named: `RenderAnimatedSize`'s own design, DoD-checked against `.flutter/flutter-master/packages/flutter/lib/src/rendering/animated_size.dart`.

## Headline: the mid-flight-retarget state machine is confirmed as the trickiest part — but not for the reason a first read suggests

The ADR frames the open question as "does `begin` become the animation's *current interpolated* value, and does the controller restart from `t=0`?" — **confirmed true, but only for the very first `stable → changed` transition.** Every *subsequent* retarget while already `changed`/`unstable` does something different and easy to get wrong: it collapses `begin = end = child's raw current size` (a **degenerate, zero-span tween** — no interpolation at all, just direct tracking), while *still* restarting the controller from `t=0` for bookkeeping. A naive reading of the ADR's one-sentence framing would lead an implementer to apply "begin = current interpolated value" uniformly to all four state-transition branches, which is wrong for three of the four retarget call sites in the oracle. This plan pins down the exact per-transition formula (§2) — that is the actual headline risk, not the coarser "does it restart from 0" question, which is simply yes.

The second-highest risk is structural, not algorithmic: `RenderAnimatedSize` cannot be built the way FLUI's *only* existing precedent for "animate a render property" (`AnimatedOpacity`/`AnimatedAlign`/`AnimatedPadding`, all in `crates/flui-widgets/src/animated/`) is built. Those all rebuild the tree every tick via `AnimatedBuilder` over a plain, stateless render object (`Opacity`, `Align`). `RenderAnimatedSize` is the *first* FLUI render object that must persist across rebuilds and drive its own layout — copying the sibling pattern doesn't just under-perform, it's architecturally incompatible (confirmed independently by the ADR's own §"What `RenderAnimatedSize` needs" and reconfirmed by reading `implicitly_animated.rs`/`animated_opacity.rs` directly, §5).

## 1. Oracle — `rendering/animated_size.dart` (line-cited)

- **Four-state enum** (`:15-51`): `start` — initial, don't yet know begin/end (`:16-20`); `stable` — assumed settled, either animating or waiting (`:22-27`); `changed` — child changed once since being `stable` (`:29-39`); `unstable` — child changing every frame; render object tightly tracks it without interpolating until it repeats a frame (`:41-50`).
- **Fields** (`:132-137,239-243`): `_controller: AnimationController` (late final, built in the constructor), `_animation: CurvedAnimation` (wraps `_controller`), `_sizeTween = SizeTween()`, `_hasVisualOverflow: bool`, `_lastValue: double?`, `_currentSize: Size` (mirrors the render object's own committed `size` for later dry-layout use), `_state`.
- **Constructor** (`:76-97`): takes `vsync`, `duration` (required), `reverseDuration` (optional), `curve` (default `Curves.linear`), `alignment`/`textDirection`/`child` (forwarded to `RenderAligningShiftedBox`), `clipBehavior` (default `Clip.hardEdge`), `onEnd`. Builds `_controller = AnimationController(vsync:, duration:, reverseDuration:)..addListener(() { if (_controller.value != _lastValue) markNeedsLayout(); })` (`:88-94`) — **listens on the controller directly, not on `_animation`.** `_animation = CurvedAnimation(parent: _controller, curve: curve)` is built *after*, and is never itself subscribed to for the mark-dirty hookup.
- **`attach`/`detach`** (`:216-237`): `attach` resumes an interrupted animation — if `state` is `changed`/`unstable`, calls `markNeedsLayout()` (comment: "in case the RenderObject isn't marked dirty already, to resume interrupted resizing animation") — then `_controller.addStatusListener(_animationStatusListener)`. `detach` calls `_controller.stop()`, removes the status listener, `super.detach()`.
- **`performLayout`** (`:245-277`): `_lastValue = _controller.value;` `_hasVisualOverflow = false;` — **fast path**: no child or `constraints.isTight` → `_controller.stop()`; `size = _currentSize = _sizeTween.begin = _sizeTween.end = constraints.smallest`; `_state = start`; `child?.layout(constraints)` (child still laid out, size just unused, **no `alignChild()` call in this branch** — the child keeps whatever offset it had, possibly stale). Otherwise: `child!.layout(constraints, parentUsesSize: true)` — **full, un-loosened constraints**, matching the parent's own; dispatch on `_state` to one of four private methods (below); then `size = _currentSize = constraints.constrain(_animatedSize!)`; `alignChild()`; `_hasVisualOverflow = size.width < _sizeTween.end!.width || size.height < _sizeTween.end!.height`.
- **`_restartAnimation`** (`:309-312`): `_lastValue = 0.0; _controller.forward(from: 0.0);` — **always forward, never reverse.**
- **`_layoutStart`** (`:314-321`, `start → stable`): `_sizeTween.begin = _sizeTween.end = child!.size;` — both ends collapse to the child's size; no animation on the very first layout.
- **`_layoutStable`** (`:323-340`, only reachable when `_state == stable`):
  - If `_sizeTween.end != child!.size` (child changed): `_sizeTween.begin = size` (the **class's own `size` field — last frame's already-`constrain`-ed committed size**, i.e. the *current visual* value, NOT the tween's prior `begin`); `_sizeTween.end = child!.size`; `_restartAnimation()`; `_state = changed`. **This is the one genuine interpolation-span retarget** — begin is the live visual value, end is the new target, animated over the full duration from `t=0`.
  - Else if `_controller.value == _controller.upperBound`: reset `_sizeTween.begin = _sizeTween.end = child!.size` (both already equal; a no-op snap, just clearing any float drift).
  - Else if `!_controller.isAnimating`: `_controller.forward()` (resume after a detach, from the *current* value — not `forward(from: 0.0)`).
- **`_layoutChanged`** (`:342-362`, only reachable when `_state == changed`):
  - If `_sizeTween.end != child!.size` (child changed *again*): `_sizeTween.begin = _sizeTween.end = child!.size` — **collapses to a degenerate zero-span tween** (NOT "begin = current interpolated value"); `_restartAnimation()`; `_state = unstable`.
  - Else (child's size repeated → stabilized): `_state = stable`; if not animating, `_controller.forward()` (resume, no restart) — the *existing* genuine interpolation span from `_layoutStable` is left untouched and keeps running to completion.
- **`_layoutUnstable`** (`:364-377`, only reachable when `_state == unstable`):
  - If `_sizeTween.end != child!.size` (still changing): same degenerate collapse (`begin = end = child.size`) + `_restartAnimation()`; state stays `unstable`.
  - Else (finally repeated): `_controller.stop()`; `_state = stable`. `_sizeTween` is left untouched (already `begin == end == this size` from the last unstable iteration) — visually already locked to the child, no glitch.
- **Reported size formula** (`:239-241,271`): `_animatedSize = _sizeTween.evaluate(_animation)`; `size = constraints.constrain(_animatedSize!)`.
- **`computeDryLayout`** (`:279-307`): no child or tight → `constraints.smallest`. Else `childSize = child!.getDryLayout(constraints)` (constraints NOT loosened), then mirrors the same state dispatch as a set of early returns (`start`→`constrain(childSize)`; `stable`→ if `end != childSize` return `constrain(_currentSize)`, else if `value == upperBound` return `constrain(childSize)`; `changed`/`unstable` → if `end != childSize` return `constrain(childSize)`); falling through in every branch to `constrain(_animatedSize!)` — **note dry-layout intentionally does NOT touch `_currentSize`/`_sizeTween`/`_state` (no side effects)**, it only *reads* them, so per the ADR's own framing this is meaningful and implementable (state exists as of the last *real* layout, dry-layout is a pure query against it).
- **`computeDryBaseline`** (`:403-419`): `child.getDryBaseline(constraints, baseline)`; if non-null, `childSize = child.getDryLayout(constraints)`, `mySize = getDryLayout(constraints)` (i.e. re-derives the SAME formula as `computeDryLayout` recursively), `offset = resolvedAlignment.alongOffset(mySize - childSize)`, return `result + offset.dy`.
- **Clip + paint** (`:385-401,421,423-429`): `if (child != null && _hasVisualOverflow && clipBehavior != Clip.none)` → `context.pushClipRect(..., super.paint, clipBehavior: clipBehavior, oldLayer: _clipRectLayer.layer)`; else paint child directly, clearing `_clipRectLayer.layer = null`. Default `clipBehavior = Clip.hardEdge`.
- **Alignment** comes from the inherited `RenderAligningShiftedBox` (`shifted_box.dart:294-377`): `resolvedAlignment` (`:315`, resolves `AlignmentGeometry` against `textDirection`, memoized, invalidated by `_markNeedResolution` which ALSO calls `markNeedsLayout` — alignment changes trigger relayout, not just repaint, `:339-345`); `alignChild()` (`:370-377`) sets `childParentData.offset = resolvedAlignment.alongOffset(size - child!.size)` using the box's *own, just-assigned* `size`. `RenderShiftedBox` (`:32-91`) provides unscaled forward-to-child intrinsics (`child?.getMinIntrinsicWidth(height) ?? 0.0`, `:39-56`) and an *uncached* `computeDistanceToActualBaseline` reading `child.getDistanceToActualBaseline(baseline) + childParentData.offset.dy` fresh every call (`:58-74`) — `RenderAligningShiftedBox` does not override this (only `RenderPositionedBox`-family subclasses add factor scaling).
- **What triggers a NEW animation vs. continuing**: precisely `_sizeTween.end != child!.size`, compared **exactly**, no epsilon — checked once per layout pass, per current `_state`, never against `_animation`'s or `_sizeTween`'s evaluated value.
- **Constructor-vs-widget split**: `vsync`, `duration`, `reverseDuration`, `curve` are all **render-object constructor params** (`:76-84`), not separately settable render-object fields with independent defaults — but they DO have public setters (`:146-171`) the widget's `updateRenderObject` calls every rebuild. `vsync` also has a setter (`:192-201`) calling `_controller.resync(vsync)` — **out of scope for FLUI** (§7; FLUI's `AnimationController` has no `resync`, and FLUI's non-singleton `Vsync` model doesn't need it — see §3 D2).

## 2. The retarget state machine — the precise, corrected FLUI translation

Refining the ADR's framing: "begin = current interpolated value" is true **only** for the `stable → changed` transition. Every other retarget (`changed → unstable`, `unstable → unstable`) is a **degenerate zero-span collapse** (`begin = end = child_size`), and `unstable → stable` does **no retarget at all**. The FLUI state enum and transitions, 1:1 with the oracle:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimatedSizeState { Start, Stable, Changed, Unstable }
```

- `layout_start(&mut self, child_size: Size)`: `self.size_tween = SizeTween::new(child_size, child_size); self.state = Stable;`
- `layout_stable(&mut self, child_size: Size)`:
  - `if self.size_tween.end != child_size { self.size_tween = SizeTween::new(self.current_size, child_size); self.restart_animation(); self.state = Changed; }` — `self.current_size` here is **last frame's already-`constrain`-ed committed size** (see §3, why this field is legitimate to keep despite the 2B-field-dedup rule).
  - `else if self.controller.status() == AnimationStatus::Completed { self.size_tween = SizeTween::new(child_size, child_size); }` — see §3 for why `status() == Completed` correctly replaces oracle's `value == upperBound` without needing an `upper_bound()` accessor.
  - `else if !self.controller.is_animating() { let _ = self.controller.forward(); }`
- `layout_changed(&mut self, child_size: Size)`:
  - `if self.size_tween.end != child_size { self.size_tween = SizeTween::new(child_size, child_size); self.restart_animation(); self.state = Unstable; }`
  - `else { self.state = Stable; if !self.controller.is_animating() { let _ = self.controller.forward(); } }`
- `layout_unstable(&mut self, child_size: Size)`:
  - `if self.size_tween.end != child_size { self.size_tween = SizeTween::new(child_size, child_size); self.restart_animation(); }`
  - `else { let _ = self.controller.stop(); self.state = Stable; }`
- `restart_animation(&mut self)`: `let _ = self.controller.forward_from(Some(0.0));` — confirmed exact match to `forward(from: 0.0)`; FLUI's `AnimationController::forward_from` (`crates/flui-animation/src/controller.rs:331-354`) clamps, sets `start_value`/`target_value = upper_bound`, and restarts the run.

## 3. FLUI building blocks — what exists, what's a genuine gap, what's a workaround

**No gap: `SizeTween` already exists.** `flui_animation::tween_types::SizeTween = Tween<Size<Pixels>>` (`crates/flui-animation/src/tween_types.rs:277`), and the generic `Tween<V: Lerp>: Animatable<V>` (`tween_types.rs:66-76`) evaluates `begin.lerp_to(&end, t)`. No `Tween<f32>`-pair workaround needed — the ADR-flagged risk ("does `flui-animation` have a `SizeTween` equivalent?") is **resolved: yes, exactly**.

**`CurvedAnimation`/`AnimationController` already exist and compose exactly as needed.** `AnimationController` (`controller.rs:100-103`) implements both `Animation<f32>` (`value()`, `status()`, `is_animating()`, `add_status_listener`/`remove_status_listener`, `forward()`/`forward_from(from)`/`stop()`) and `Listenable` (`add_listener`/`remove_listener`, backed by its own `notifier: Arc<ChangeNotifier>` field, `:1298-1310`) — **two separate channels**, matching the oracle's own split (`_controller.addListener(...)` for value-change, `_controller.addStatusListener(...)` for completion). `CurvedAnimation<C: Curve + Clone + Send + Sync>` (`curved.rs:35-52`) wraps `parent: Arc<dyn Animation<f32>>`, forwards `add_status_listener`/`remove_status_listener` straight to `parent` (`:143-149`), and is itself `Listenable` via its own notifier that re-emits parent changes (`:152-164`) — **for `RenderAnimatedSize`, subscribe to `self.controller` directly for the mark-dirty hookup (matching oracle exactly, which also subscribes to `_controller`, never `_animation`), and to `self.controller` again for the status/`on_end` hookup.** `ArcCurve` (`curve.rs:1131`, `Arc<dyn Curve + Send + Sync>`, `Clone`+`Debug`) is the existing pattern for a runtime-swappable curve field, already used by every sibling implicit widget.

**A real, small, previously-undocumented API gap in `AnimationController`:** no public `upper_bound()`/`lower_bound()` accessor exists (grepped `controller.rs` in full — none). Oracle's `_controller.value == _controller.upperBound` check has no direct FLUI equivalent. **Workaround, not a blocker**: since `restart_animation` **always** calls `forward_from(Some(0.0))` (never `.reverse()`), a completed run is *always* `AnimationStatus::Completed` — `self.controller.status() == AnimationStatus::Completed` is bounds-agnostic and exactly equivalent for this object's usage, and is arguably cleaner than exposing bounds. **Recommendation: use `status()`, do not add an `upper_bound()` getter to `AnimationController` for this feature.**

**A second, minor, confirmed-dead-code oracle parameter:** `reverseDuration`/`reverse_duration` has **no observable effect** on `RenderAnimatedSize` — `_restartAnimation` never calls `.reverse()`, so `_controller.reverseDuration` is never read by anything this object does. Confirmed by reading every call site of `_restartAnimation`/`.reverse()` in the oracle file — there are none of the latter. Kept for constructor/widget API parity only (Flutter's own `AnimationController` needs it for symmetry; it's genuinely inert here). Document this explicitly rather than silently wiring it as if it mattered.

**A third, small, real gap:** `AnimationController::set_reverse_duration(&self, duration: Duration)` (`controller.rs:288-291`) has no way to **clear** `reverse_duration` back to `None` — Flutter's `Duration?` setter can. Given point above (it's inert for this feature), **do not fix this as part of this slice** — note it, defer it (§8).

**D2 confirmed, and here's the concrete FLUI pattern for it** — read from `crates/flui-widgets/src/animated/implicitly_animated.rs` (`ImplicitController`, `:40-120`) and `vsync_scope.rs`: a `View`'s `State` builds `AnimationController::new(duration, Arc::new(Scheduler::new()))` — **a fresh, never-pumped scheduler** (comment at `implicitly_animated.rs:50-53`: "on a real display its ticker would drive the controller off wall-clock time; under a `VsyncScope` the binding drives it deterministically via `tick_at` instead… so the two paths never double-advance") — then registers it: `if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) { let registration = vsync.register(controller.clone()); }` (`animated_opacity.rs:90-93`, `implicitly_animated.rs:67-71`). `RenderAnimatedSize`'s owning `AnimatedSizeState` follows the **identical** construction/registration recipe — the *only* structural difference from `AnimatedOpacity`'s `ImplicitController` is that `AnimatedSizeState` does **not** build a `Tween<T>`/`CurvedAnimation`/`AnimatedBuilder` at all; those live on the render object instead (§4), because the render object — not a rebuild — must detect the retarget.

## 4. `RenderAnimatedSize` (`crates/flui-objects/src/layout/animated_size.rs`, new)

`type Arity = Single; type ParentData = BoxParentData;` — modeled directly on `RenderAlign`/`AligningShiftedBox` (`crates/flui-objects/src/layout/align.rs`, `shifted_box.rs`), the closest existing precedent for "align + hit-test + baseline-cache a single child," reused **as-is**, not reimplemented (FLUI's `compute_distance_to_actual_baseline` trait hook has no `ctx` parameter, so `AligningShiftedBox`'s cache-during-layout design is not just convenient — it's the *only* mechanism the trait surface allows; a live per-call child query, like the oracle's uncached `RenderShiftedBox.computeDistanceToActualBaseline`, is not expressible in FLUI's current trait shape).

```rust
pub struct RenderAnimatedSize {
    inner: AligningShiftedBox,           // alignment + hit_test + baseline cache (reused, not reimplemented)
    controller: AnimationController,     // injected at construction (ADR D2) — never built here
    animation: CurvedAnimation<ArcCurve>,
    curve: ArcCurve,                     // kept alongside `animation` so set_curve can rebuild it
    size_tween: SizeTween,
    state: AnimatedSizeState,
    current_size: Size,                  // last frame's committed (constrain()-ed) size — legitimate
                                          // per-object animation state, NOT a re-introduction of the
                                          // removed 2B-dedup `size()` field: it's read only by the
                                          // `stable`-retarget branch and by compute_dry_layout's
                                          // `stable` branch, exactly mirroring oracle's `_currentSize`.
    has_visual_overflow: bool,
    clip_behavior: Clip,
    listener_id: Option<ListenerId>,      // value-change subscription, torn down in detach
    status_listener_id: Option<ListenerId>, // on_end subscription, torn down in detach
    on_end: Arc<parking_lot::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>,
}
```

- **Constructor**: `RenderAnimatedSize::new(controller: AnimationController, curve: ArcCurve, alignment: Alignment, clip_behavior: Clip, on_end: Option<Arc<dyn Fn() + Send + Sync>>) -> Self` — takes an **already-built** `AnimationController` per ADR D2 (never a `Vsync`/`Duration` it builds one from internally). Builds `animation = CurvedAnimation::new(Arc::new(controller.clone()) as Arc<dyn Animation<f32>>, curve.clone())` — no `.with_reverse_curve(...)` (oracle's `AnimatedSize` widget has no `reverseCurve` param at all, only `reverseDuration`, so the reverse-curve-lock machinery in `CurvedAnimation` is never exercised — a nice, confirmed non-issue since `restart_animation` only ever runs `Forward`, never `Reverse`).
- **`attach(&mut self, handle: RepaintHandle)`**: subscribe to `self.controller` (the `Listenable`, NOT `self.animation`, matching oracle's `_controller.addListener`):
  ```rust
  fn attach(&mut self, handle: RepaintHandle) {
      let mark_handle = handle.clone();
      self.listener_id = Some(self.controller.add_listener(Arc::new(move || {
          let _ = mark_handle.mark_needs_layout();
      })));
      let on_end = Arc::clone(&self.on_end);
      self.status_listener_id = Some(self.controller.add_status_listener(Arc::new(move |status| {
          if status == AnimationStatus::Completed
              && let Some(cb) = on_end.lock().as_ref() { cb(); }
      })));
      if matches!(self.state, AnimatedSizeState::Changed | AnimatedSizeState::Unstable) {
          let _ = handle.mark_needs_layout(); // oracle :225-227, resume interrupted animation
      }
  }
  ```
  The status listener is registered **unconditionally**, not gated on `on_end.is_some()` at attach time — `on_end` can be set later via a setter, and the closure reads the live `Arc<Mutex<Option<..>>>` cell each fire, so a later `set_on_end` needs no re-subscription. This `Arc<Mutex<>>` indirection is the Rust-specific translation of Dart's "closure captures `this` and reads `_onEnd` live" — `ListenerCallback`/`StatusCallback` are `'static` and cannot borrow `&self`.
- **`detach(&mut self)`**: unsubscribe both listeners; **deliberately does NOT call `self.controller.stop()`** — this is a documented FLUI divergence from the oracle (`:233-236`, which stops the controller because Flutter's `detach` fires far more often, e.g. for temporarily-offstage-but-still-mounted subtrees; FLUI's `detach` only fires on structural tree removal, per the ADR's tree-lifecycle note, and controller lifecycle/disposal is the owning `State`'s job). Stopping here would also race a fresh `attach` on a remove+insert reparent.
- **`perform_layout`**: fast path (`ctx.child_count() == 0 || constraints.is_tight()`) stops the controller, resets `size_tween`/`state = Start`, still lays out the child with the raw constraints but does **not** call `align_child` (matches oracle's stale-offset quirk exactly, §1). Otherwise: `ctx.layout_child(0, constraints)` — **full, un-loosened constraints**, per oracle `:258`, dispatch to the four transition methods (§2), then `let animated = self.size_tween.transform(self.animation.value()); let size = constraints.constrain(animated); self.current_size = size; self.inner.align_child(ctx, size, child_size); self.inner.record_child_baselines(ctx); self.has_visual_overflow = size.width < self.size_tween.end.width || size.height < self.size_tween.end.height;`
- **`compute_dry_layout`**: pure fn of `&self` + freshly-queried `child_size` — see §2/§1's exact per-state formula, factored into a private `dry_size_for(&self, constraints, child_size) -> Size` shared with `compute_dry_baseline` (mirrors the Table plan's shared-closure pattern for the identical reason: one formula, two call sites with differently-typed contexts that both expose `child_dry_layout`).
- **`compute_dry_baseline`**: `ctx.child_dry_baseline(0, constraints, baseline)?`, `ctx.child_dry_layout(0, constraints)`, `my_size = if constraints.is_tight() { constraints.smallest() } else { self.dry_size_for(constraints, child_size) }`, `offset = self.inner.dry_child_offset(my_size, child_size)`, return `child_baseline + offset.dy.get()`.
- **`compute_distance_to_actual_baseline`**: `self.inner.actual_baseline(baseline)` — direct reuse, identical to `RenderAlign`.
- **Intrinsics**: unscaled forward-to-child (`RenderShiftedBox` parity, `shifted_box.dart:38-56`, no factor): `if ctx.child_count() == 0 { 0.0 } else { ctx.child_min_intrinsic_width(0, height) }` (and the max/height variants) — simpler than `RenderAlign`'s (no `width_factor`/`height_factor` multiplication).
- **`paint`**: `if self.has_visual_overflow && self.clip_behavior != Clip::None { let bounds = Rect::from_origin_size(Point::ZERO, ctx.size()); ctx.with_clip_rect(bounds, self.clip_behavior, |ctx| ctx.paint_child()); } else { ctx.paint_child(); }` — directly mirrors `RenderStack`'s established clip idiom (`layout/stack.rs:550-564`), `PaintCx<'_, Single>::paint_child()` (no index, `context/paint_cx.rs:417-421`).
- **`hit_test`**: `self.inner.hit_test(ctx)` — direct reuse.
- **Setters** (called from the widget's `update_render_object`, changed-flag convention matching `RenderFlow`/`RenderStack`): `set_alignment(&mut self, alignment: Alignment) -> bool` (oracle's alignment setter triggers relayout, `shifted_box.dart:339-345` — `AligningShiftedBox` has no setter today; add one, or reconstruct it — cheap either way since it holds no other state); `set_clip_behavior(&mut self, clip: Clip) -> bool` (paint-only, oracle `:178-184`); `set_duration(&self, d: Duration)`/`set_reverse_duration(&self, d: Duration)` (thin forwards straight to `self.controller.set_duration`/`set_reverse_duration` — no restart, matching oracle's plain field-assignment setters `:148-162`); `set_curve(&mut self, curve: ArcCurve)` (rebuilds `self.animation = CurvedAnimation::new(Arc::new(self.controller.clone()), curve.clone()); self.curve = curve;` — safe because direction never flips for this object, so `CurvedAnimation`'s reverse-curve-lock state losing its capture on rebuild is a confirmed non-issue, §3); `set_on_end(&self, cb: Option<Arc<dyn Fn() + Send + Sync>>) { *self.on_end.lock() = cb; }` (no dirty-marking at all, matches oracle's inert setter).
- **`Diagnosticable`**: expose `alignment` (via a getter added to `AligningShiftedBox` or stored redundantly — small detail), `clip_behavior`, `state`, mirroring `RenderAlign`'s `debug_fill_properties`.

**Confirmed-safe-to-skip micro-optimization**: oracle's `_lastValue` guard (only `markNeedsLayout` if `_controller.value != _lastValue`) is **not required for correctness** in FLUI — `RepaintHandle::mark_needs_layout` is a cheap buffered-channel enqueue, and Flutter's own `markNeedsLayout()` is *also* a no-op when already dirty, so the guard is a defensive micro-optimization on both sides, not a correctness dependency. Recommend omitting it (simplifies the listener closure, no shared last-value cell needed) — see §8.

## 5. `AnimatedSize` widget (`crates/flui-widgets/src/animated/animated_size.rs`, new)

**Structural warning, the single most important divergence from every sibling in this directory**: `AnimatedOpacity`/`AnimatedAlign`/`AnimatedPadding` all *delegate* their `build()` to an **existing plain widget** (`Opacity`, `Align`, `Padding`) wrapped in `AnimatedBuilder`, and implement **no `RenderView` themselves**. There is no pre-existing "a box that positions+clips a child" plain widget shaped like `RenderAnimatedSize` to delegate to, and even if there were, delegating would be wrong here — `RenderAnimatedSize` must **persist** across rebuilds (holding animation state no rebuild-driven object could keep), so `AnimatedSize` needs its **own private `RenderView` implementation**, directly wrapping `RenderAnimatedSize`. Modeled on Flutter's own split (public `AnimatedSize` `StatefulWidget` → private `_AnimatedSize` `SingleChildRenderObjectWidget`, `widgets/animated_size.dart:27-93,111-182`):

```rust
#[derive(Clone, StatefulView)]
pub struct AnimatedSize {
    alignment: Alignment,        // default Alignment::CENTER
    duration: Duration,          // required
    reverse_duration: Option<Duration>,
    curve: ArcCurve,             // default: RECOMMEND Curves::Linear (oracle parity, widgets/animated_size.dart:33) —
                                  // note this deliberately diverges from the sibling widgets' FLUI-chosen
                                  // Curves::EaseInOut default; flag to api-design-lead if sibling-consistency
                                  // is instead preferred (one-line change either way).
    clip_behavior: Clip,         // default Clip::HardEdge
    on_end: Option<Arc<dyn Fn() + Send + Sync>>,
    child: Child,                // OPTIONAL — matches Align's Child (not AnimatedOpacity's required BoxedView),
                                  // because the tight/no-child fast path is a real, load-bearing code path here.
}

pub struct AnimatedSizeState {
    controller: AnimationController,
    vsync: Option<Vsync>,
    vsync_registration: Option<VsyncRegistration>,
    child: Child,
}
```

- `create_state`: `AnimationController::new(duration, Arc::new(Scheduler::new()))` (fresh, unpumped — exact `ImplicitController::new` recipe), then `if let Some(rd) = self.reverse_duration { controller.set_reverse_duration(rd); }`.
- `init_state`: `if let Some(vsync) = ctx.get::<VsyncScope, _>(|s| s.vsync().clone()) { self.vsync_registration = Some(vsync.register(self.controller.clone())); self.vsync = Some(vsync); }` — identical to `AnimatedOpacityState::init_state`.
- `build`: returns the private inner view **directly** — **no `AnimatedBuilder`**:
  ```rust
  fn build(&self, view: &AnimatedSize, _ctx: &dyn BuildContext) -> impl IntoView {
      AnimatedSizeRenderView {
          controller: self.controller.clone(),
          curve: view.curve.clone(),
          alignment: view.alignment,
          clip_behavior: view.clip_behavior,
          on_end: view.on_end.clone(),
          child: self.child.clone(),
      }
  }
  ```
  Because `build` reads the **live** `view: &AnimatedSize` fields on every call, `AnimatedSizeRenderView`'s field values are naturally fresh on every rebuild — the ordinary single-child-render-object reconciliation machinery calls `update_render_object` exactly as it would for `Align`, and that is where alignment/curve/clip_behavior/on_end changes reach the persistent `RenderAnimatedSize` instance, via **targeted setters**:
  ```rust
  impl RenderView for AnimatedSizeRenderView {
      type Protocol = BoxProtocol;
      type RenderObject = RenderAnimatedSize;
      fn create_render_object(&self) -> RenderAnimatedSize {
          RenderAnimatedSize::new(self.controller.clone(), self.curve.clone(), self.alignment, self.clip_behavior, self.on_end.clone())
      }
      fn update_render_object(&self, ro: &mut RenderAnimatedSize) {
          ro.set_alignment(self.alignment);
          ro.set_curve(self.curve.clone());
          ro.set_clip_behavior(self.clip_behavior);
          ro.set_on_end(self.on_end.clone());
          // controller is NOT re-passed here — it's the SAME persistent Arc-backed
          // object across every rebuild; only ITS internal duration is pushed,
          // and that happens in did_update_view below, not here.
      }
      fn has_children(&self) -> bool { self.child.is_some() }
      fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
          if let Some(c) = self.child.as_ref() { visitor(c); }
      }
  }
  impl_render_view!(AnimatedSizeRenderView);
  ```
  **The critical trap to name explicitly**: `Align::update_render_object` (`layout/align.rs:76-78`) does `*render_object = self.build_render_object();` — **replaces the whole object**. Copying that pattern here would silently wipe `RenderAnimatedSize`'s persistent animation state (`state`/`size_tween`/`current_size`/listener subscriptions) on **every single unrelated rebuild** — a severe, easy-to-introduce regression precisely because `RenderAlign`/`Align` is the closest structural precedent yet its widget-update convention must NOT be copied. Use targeted setters only, matching `RenderFlow`/`RenderStack`'s convention instead.
- `did_update_view`: `self.child = new_view.child.clone(); self.controller.set_duration(new_view.duration); if let Some(rd) = new_view.reverse_duration { self.controller.set_reverse_duration(rd); }` (no restart — matches oracle's plain-assignment setters).
- `dispose`: `if let (Some(vsync), Some(reg)) = (&self.vsync, self.vsync_registration) { vsync.unregister(reg); } self.controller.dispose();`

Registered in `crates/flui-widgets/src/animated/mod.rs`: `mod animated_size; pub use animated_size::{AnimatedSize, AnimatedSizeState};` (the private `AnimatedSizeRenderView` stays unexported).

**Manifest change required**: `crates/flui-objects/Cargo.toml:31` currently lists `flui-animation` under `[dev-dependencies]` only (line 26 opens that section) — must be promoted to `[dependencies]` (line ~11 section), since `RenderAnimatedSize` holds an `AnimationController` in production code, not just tests. This is the exact situation ADR-0013 described for `flui-rendering` (§"The four verified gaps," point 4) but the gap sits one crate over, in `flui-objects`. `flui-scheduler` can stay dev-only (nothing in this feature constructs a `Scheduler` inside `flui-objects`). `flui-widgets/Cargo.toml` already depends on both `flui-objects` and `flui-animation` as normal dependencies (`:20,24`) — no change needed there.

## 6. Catalog guard (mandatory)

`crates/flui-objects/src/layout/animated_size.rs` registered in `layout/mod.rs` (`mod animated_size;` / `pub use animated_size::*;`, alongside `align`/`shifted_box`). Add `"RenderAnimatedSize"` to `RENDER_OBJECT_TYPES` (`crates/flui-objects/tests/render_object_harness.rs:119-...`) and a doc-table coverage row near the `RenderAlign`/`RenderStack` rows, plus `harness_animated_size_*` tests (checked both by `catalog_covers_every_render_object_name` and `render_object_types_match_exports`, which cross-checks against `pub use` exports in `crates/flui-objects/src/lib.rs`).

## 7. Tests

**Unit (state machine, deterministic virtual time via `Vsync::tick_all`, the mechanism `implicitly_animated.rs`/`vsync.rs` tests already use)**:
- `start → stable` snaps with no animation (`size_tween.begin == end == child_size` immediately).
- `stable → changed`: assert `size_tween.begin` equals the **last reported/constrained** size (not the raw pre-constrain tween value — construct a case where the parent's constraints clip the animated size, to actually distinguish the two), `end` equals the new child size, controller restarts at `t=0`.
- `changed → unstable` (child changes again before settling): assert the **degenerate collapse** — `begin == end == new child size` exactly (this is the test that catches the "apply begin=current-value uniformly" bug the headline risk warns about).
- `unstable → unstable` repeatedly, then `unstable → stable`: assert no visible size jump across the transition (since `begin == end` already).
- Retarget mid-flight at several different progress fractions (`t = 0.2, 0.5, 0.8`) via `Vsync::tick_all`, asserting the resumed run's reported size continues from the constrained live value, never jumps.
- `compute_dry_layout`/`compute_dry_baseline` parity against `perform_layout`'s formula at each state, with no side effects (call twice, assert `state`/`size_tween` unchanged).
- Fast path (tight constraints / no child): asserts `state` resets to `Start`, controller stopped, no `align_child` call (offset stays at whatever it was — regression-test the deliberately-preserved oracle quirk).
- `on_end` fires exactly once per completed run, via the shared `Arc<Mutex<>>` cell, including after a `set_on_end` swap mid-run.
- `attach` on a node already in `Changed`/`Unstable` state immediately requests a layout (simulates reparent-mid-animation).

**Harness (`harness_animated_size_*`, real per-frame geometry across several `run_frame()` calls — matching Flow/Table rigor)**:
- A child that grows once: assert the reported size actually interpolates over several frames (not snap), matching hand-computed `Tween::transform` values at known `t`.
- Clip verification: an overflowing mid-animation frame produces a `Clip` layer in `run.structure()`; a settled frame (no overflow) does not.
- Alignment verification: with a non-center alignment, assert the child's laid-out offset within the (still-animating, undersized) parent bounds.
- Retarget-mid-flight harness case (the ADR's own flagged scenario): drive several frames of growth, then change the child's size again before settling, assert the visual size never jumps discontinuously frame-to-frame.
- Baseline: assert `compute_distance_to_actual_baseline` matches child baseline + recorded offset, refreshed only on frames where `align_child` actually ran.

**Widget test**: `AnimatedSize` under a `VsyncScope`, driven via `Vsync::tick_all` through a headless binding — assert the container's rendered size actually animates across frames when the child's `key`-stable subtree is swapped for a differently-sized one, and that alignment/clip_behavior/curve changes from a parent rebuild reach the persistent render object without resetting its in-flight animation (the regression test for the `update_render_object`-must-not-replace-the-object trap in §5).

## Risk ranking

- **HIGH** — the retarget state machine's exact per-transition formula (§2): confirmed as the single highest-risk item, but precisely because the *naive* reading of "begin = current interpolated value" is wrong for 3 of 4 transitions, not because the general shape is unclear.
- **HIGH** — the widget-layer structural split (§5): `AnimatedSize` needs its own private `RenderView` (unlike every sibling in `animated/`), and its `update_render_object` must use targeted setters, not `Align`'s "replace the whole object" convention — both are real traps precisely because the nearest precedents (siblings for the first, `Align`/`RenderAlign` for the second) actively point the wrong way.
- **MED** — the `Arc<Mutex<Option<..>>>` interior-mutability pattern needed for `on_end` (a `'static` `StatusCallback` cannot borrow `&self`) — mechanical once identified, easy to get subtly wrong (e.g. gating the status-listener registration on `on_end.is_some()` at attach time, which would break a later `set_on_end`).
- **MED** — `compute_dry_layout`/`compute_dry_baseline` sharing one pure `dry_size_for` helper without touching `&mut self` state, and the tight-constraints short-circuit ordering (must skip `ctx.child_dry_layout` entirely when tight, matching oracle's asymmetry with `perform_layout`'s tight path which DOES still touch the child).
- **LOW→MED** — the tight/no-child fast path's deliberately-NOT-refreshed baseline cache (a genuine, narrow, documented divergence from oracle's *uncached* baseline lookup — only observable when constraints are tight, a child is present, and baseline is queried in that exact frame).
- **LOW** — reusing `AligningShiftedBox`/`RenderStack`'s clip idiom/`PaintCx::with_clip_rect`, unscaled forward-to-child intrinsics, hit-test (all direct, unmodified reuse of existing precedent); catalog guard; `Cargo.toml` dependency promotion (single-line, no other manifest changes needed).

## 8. Deferred, documented

- **`AnimationController::resync`/render-object `vsync` setter** (oracle `:192-201`) — FLUI's `AnimationController` has no `resync`, and FLUI's `Vsync` is a registry the *State* registers into once, not a per-controller mutable reference the render object holds — out of scope, consistent with the ADR's own D2 (the render object never sees a `Vsync`/scheduler at all).
- **`reverseDuration` clearing** (`AnimationController::set_reverse_duration` takes `Duration`, not `Option<Duration>` — no way to unset) — confirmed **inert** for `RenderAnimatedSize` specifically (never reads `reverse_duration`, only ever calls `forward_from`, never `.reverse()`); defer the `flui-animation` API fix as a separate, low-priority follow-up, not blocking this feature.
- **`_lastValue`-style dedup guard on the value-listener** — confirmed safe to omit; `mark_needs_layout` and Flutter's own `markNeedsLayout` are both already cheap/idempotent when already dirty.
- **Exact curve shapes beyond linear/named curves** — `ArcCurve` already supports arbitrary `Curve` impls (elastic, bounce, custom cubic), nothing further needed; not actually deferred, just noting no gap exists.
- **`vsync` scoping edge cases** (a render object outliving its `VsyncScope`, multiple scopes, etc.) — inherits whatever behavior `ImplicitController`/`Vsync` already has for every other implicit-animation widget; not specific to this feature.
- **Semantics** (`markNeedsSemanticsUpdate` on `clipBehavior` change, oracle `:182`) — no semantics tree in FLUI yet, consistent catalog-wide.
- **`AnimationController::upper_bound()`/`lower_bound()` accessors** — deliberately NOT added; `status() == Completed` is the correct, bounds-agnostic FLUI-native substitute for this object.

### Critical Files for Implementation
- crates/flui-objects/src/layout/animated_size.rs (new — `RenderAnimatedSize`)
- crates/flui-objects/src/layout/shifted_box.rs (existing `AligningShiftedBox` — reused directly for alignment/hit-test/baseline)
- crates/flui-widgets/src/animated/animated_size.rs (new — `AnimatedSize`/`AnimatedSizeState`/private `AnimatedSizeRenderView`)
- crates/flui-animation/src/controller.rs (`AnimationController` — value/status listener API, `forward_from`, `status()`)
- crates/flui-objects/Cargo.toml (promote `flui-animation` from `[dev-dependencies]` to `[dependencies]`, line ~31)
