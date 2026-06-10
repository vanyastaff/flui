# flui-animation — market-ready Rust-native redesign (design spec)

- **Date:** 2026-06-09
- **Status:** APPROVED design — proceeding to implementation plan
- **Author:** orchestrated redesign (27-agent research workflow `wf_ff81c282-d46`; findings reference: `.claude/anim-findings-reference.md`)
- **Scope decision (user):** *engine-complete, no widgets* — land the full animation engine (correctness rescue → Lerp/generic-Tween → combinator re-emit → spring core → scroll physics → paint-only seam), stop before the `AnimatedFoo` widget catalog.
- **Source of truth for Flutter behavior:** `.flutter/flutter-master/packages/flutter/lib/src/{animation,physics,widgets}` (real checkout, symlinked).

---

## 1. Verdict — why this is a rescue, not a polish

flui-animation is 7492 LOC across 17 files with elaborate docs, but **the core dynamic paths are silently non-functional** and the API ~90% of real apps use is absent. Three blockers verified firsthand in source:

| # | Blocker | Evidence | Effect |
|---|---------|----------|--------|
| B1 | `animate_to` never advances value | [`controller.rs:450-452`](../../crates/flui-animation/src/controller.rs) — tick callback is `move \|_elapsed\| notifier.notify_listeners()`, discards `_elapsed`, never calls a `tick()`. Ticker runs forever. | The most-used controller method is dead; ticker leaks. |
| B1b | `animate_to` clobbers base duration | [`controller.rs:438-440`](../../crates/flui-animation/src/controller.rs) — `inner.duration = dur` permanently overwrites. | One timed `animate_to` corrupts every later run. |
| B1c | status notify under lock | [`controller.rs:455`](../../crates/flui-animation/src/controller.rs) — `notify_status_listeners(..., &inner)` with the `MutexGuard` still held. | Re-entrant status callback (idiomatic: chain another animation) → deadlock. |
| B2 | combinators never subscribe to parent | [`curved.rs:60`](../../crates/flui-animation/src/curved.rs) — `_parent_listener_id` created `None`, never written; `add_listener` (104) registers into a `self.notifier` nothing ever fires. Same in reverse/compound/proxy/switch. | `AnimatedBuilder(curved_anim)` never rebuilds on tick — the standard composed pattern is dead. |
| B3 | value-layer clamp kills overshoot | [`tween_types.rs:106`](../../crates/flui-animation/src/tween_types.rs) (+302,431,471,557…) — every tween does `t.clamp(0,1)`. Flutter's `Tween.transform` does **not** clamp. | elastic/easeOutBack/spring overshoot silently flattened — spring-as-default cannot feel right. |

**Plus** missing surface: no implicit animation (`AnimatedContainer`/`animate*AsState`), zero widget consumers (lifecycle/dispose/vsync unproven), no velocity-transfer/perceptual springs, no scroll physics (`FrictionSimulation` accepts `drag>1` → never terminates), no `Matrix4::lerp`/`TextStyleTween`/`DecorationTween`, gamma-sRGB color tweens, fabricated bench numbers, copy-paste-wrong docs.

---

## 2. Goals & non-goals

**Goals (this cycle):**
1. Correctness: all three blockers fixed, behind frame-advance/re-entrancy tests.
2. Rust-native structure: one `Lerp` trait collapses the per-type `*Tween` explosion; one `ListenerRegistry` collapses Flutter's 4-mixin listener lattice and makes the dead-listener bug structurally impossible.
3. Market differentiators: velocity-preserving interruptible springs + Apple perceptual presets; const-LUT curves; OKLab color (opt-in); paint-only repaint seam.
4. Completeness: full curve catalog, full simulation set incl. scroll physics, all animatable types, committed benches, accurate docs.

**Non-goals (explicit deferrals, separate tracks):**
- `AnimatedFoo` / `AnimatedBuilder` / `TickerProvider`-ownership widgets in flui-view (next cycle; the engine is built to feed them).
- The ADR-0002 Phase-1 `!Send` flip of flui-scheduler/bindings (engine-wide; tracked separately — see D2).
- flui-reactivity signal bridge (C1 locks signals out of the catalog; stay on the `Listenable` + `mark_needs_build/paint` path).

---

## 3. Architecture decisions

### D1 — `Animation<T>` stays an object-safe trait (reject enum-graph)
`Arc<dyn Animation<T>>` is pervasive in source ([`curved.rs:36`](../../crates/flui-animation/src/curved.rs) and every combinator) and flui-view's `AnimatedBehavior` consumes `dyn` erasure. An enum graph cannot model the open, user-extensible combinator set without sealing it, and the static-dispatch win is only partial (dyn still needed at recursive edges). **Keep the trait; cherry-pick the enum/LUT wins (const-LUT curves, zero-alloc const animations, delete `DynAnimation`) without the rewrite.**

### D2 — Thread model: scoped `Send+Sync` exception now; `!Send` flip deferred
**Decision:** keep `AnimationController` `Send+Sync` (`Arc<Mutex>`), record a scoped ADR-0002 exception, route listener bounds through `flui_foundation::WasmNotSendSync`. Design **`!Send`-ready**.

**Why not flip now (despite ADR-0002 putting the Send boundary below bindings):** the flui-scheduler Ticker contract is Send-bound at the source — `TickerCallback = Box<dyn FnMut(f64) + Send>` ([`ticker.rs:80`](../../crates/flui-scheduler/src/ticker.rs)), `Ticker::start<F: FnMut(f64)+Send+'static>` ([`ticker.rs:355`](../../crates/flui-scheduler/src/ticker.rs)), `TickerProvider: Send+Sync` ([`ticker.rs:96`](../../crates/flui-scheduler/src/ticker.rs)). A `!Send` `Rc<RefCell>` controller cannot drive it without flipping the scheduler's threading — which *is* the ADR-0002 Phase-1 work, out of this cycle's scope. The per-frame uncontended-lock cost is nanoseconds and is dwarfed by the bugs being fixed; the competitive lever is springs+paint, not lock removal.

**`!Send`-ready guarantees** so the future flip is mechanical: (a) RAII `Drop` subscriptions (work in both models); (b) all controller mutation behind one lock boundary; (c) the new `Lerp` data trait carries **no** `Send+Sync` bound (data-plane-neutral; would over-constrain `Copy` geometry primitives).

**Deliverable:** `docs/adr/ADR-0002` amendment note recording the exception + the conditions under which the flip happens.

### D3 — One `ListenerRegistry` (value + status), RAII `Drop` subscriptions
Collapse Flutter's 4-mixin lattice (`AnimationLazy/Eager/LocalListeners/LocalStatusListeners`, one shared counter) into a single composed type embedded in every animation. Backed by `ChangeNotifier`'s hardened machinery (snapshot-under-lock, registration-order firing, per-callback `catch_unwind`, remove-during-notify skip, dispose guard, **drop-lock-before-callback**). Exposes `on_first_listener`/`on_last_listener` edge hooks; lazy-vs-eager is encoded as *which* hook the owner wires. foundation grows a generic typed `Notifier<Arg>` so the status channel shares the hardened path (today it is a hand-rolled `Vec`+counter with notify-under-lock and no panic isolation). RAII `Subscription` (Drop teardown) replaces Flutter's leak-prone manual `dispose()` and **structurally eliminates B2** — you cannot forget to wire what the registry owns.

### D4 — `Lerp`/`MaybeLerp` in flui-geometry; collapse `*Tween` → generic `Tween<V>`
Define a total `Lerp` trait in flui-geometry (Layer-0, where `GeometryOps::lerp` already lives at [`traits.rs:658`](../../crates/flui-geometry/src/traits.rs)). flui-types depends on flui-geometry → orphan-legal for both geometry types and flui-types types (Color/Alignment/BorderRadius). Blanket-impl over `GeometryOps`; hand-impl composites delegating to the existing inherent `::lerp`. flui-types impls its own types. Collapse the ~9 bespoke `*Tween` structs into one `Tween<V: Lerp + Clone>` + type aliases (`FloatTween = Tween<f32>`, `ColorTween = Tween<Color>`, …). **Remove the value-layer clamp** (B3); restore the exact-endpoint short-circuit **once** in `Tween::transform`. Sibling fallible `MaybeLerp -> Option<Self>` for `Gradient`/`Decoration`/`TextStyle` (Flutter already returns `Option` here — [`decoration.rs:102`](../../crates/flui-types/src/styling/decoration.rs)).

**Naming hazard (from source):** data trait method `Lerp::lerp(self, other, t)` collides with the existing tween self-method `lerp(&self, t)` — rename the tween method to `transform`/`at`. Inherent `::lerp` signatures are inconsistent (`Offset::lerp(self, impl Into, t)` assoc vs `Color::lerp(a,b,t)` assoc vs `BorderRadius::lerp` trait-method) → the blanket forward adapts per-impl. **No shim — migrate every call site** (per no-quick-wins).

### D5 — Full per-component interruptible spring core + Apple presets
`AnimatedValue<T>` owns a `SpringSimulation` storing `(position, velocity)` **per scalar component**; `animate_to(new_target)` re-seeds from the running spring's analytic `x(t)` + `dx(t)`. FLUI's `simulation.rs` already returns closed-form `dx(t)` exactly ([`simulation.rs`](../../crates/flui-animation/src/simulation.rs)) → velocity handoff is O(1) and *strictly better* than the numerically-sampled web libs (Motion/React-Spring). Expose Apple `SpringDescription::smooth/snappy/bouncy` + `with_response_and_damping(response, damping_fraction)` as the **documented default** (designers reason in response/dampingFraction, not mass/stiffness/damping). `#[derive(Animatable)]` / `TwoWayConverter` (`T <-> [f32; N]`) in flui-macros gives per-component springs the independent "spongy" SwiftUI/Compose feel.

**Required care:** unit-aware rest epsilon via a `distance()` on the converter (0.01 right for opacity, wrong for 2000px → infinite ticking); f64-internal solution constants (f32 cancellation near critical / long springs jitters); reconcile `with_duration_and_bounce`'s `bounce<0` branch to Apple's exact `damping = 4π / (duration·(1+bounce))` before presets ship.

### D6 — Paint-only invalidation seam in flui-view
`create_mark_needs_paint_callback` mirroring the existing `create_mark_dirty_callback`, capturing the `Arc<RwLock<PipelineOwner>>` already in `ElementCore` + a `Copy` `RenderId`, calling `add_node_needing_paint(render_id)` **without** flipping the rebuild dirty bit. Partition transitions: opacity/transform/color/decoration → paint; size/position/align → layout. This is the headline beat-Flutter perf lever (Flutter's `FadeTransition` rebuilds the opacity Element every frame; FLUI drives it straight at the RenderObject). The `PipelineOwner::add_node_needing_paint` primitive already exists; only the listener→paint bridge is missing. **Validated this cycle by a direct integration test (drive a RenderObject, assert repaint-not-rebuild) — no widget required.** Care: capture `Arc`+`Copy RenderId` (can't hold `&mut Element` from a `&self` listener); a tick firing mid-frame must defer its mark (build-scope re-entrancy panics in debug).

### D7 — Curve LUTs + dead-code deletion
Const LUTs for the ~40 const `Cubic` presets where profiling justifies (the `Cubic::transform` runs a fixed 8-iter bisection every `CurvedAnimation` frame → compile-time table = O(1) lookup+lerp). Delete the redundant `DynAnimation<T>` marker ([`animation.rs:129`](../../crates/flui-animation/src/animation.rs) — re-states `Animation: Listenable`, zero capability). Fold/delete the dead `ParametricCurve<T>` trait (declared but `Curve` doesn't extend it). const-fn float bisection for compile-time LUTs needs Rust-1.95 verification; fall back to `build.rs` table generation if const-eval can't reach precision parity on steep cubics (EaseInOutExpo).

### D8 — Build-vs-buy
| Concern | Decision | Rationale |
|---------|----------|-----------|
| Perceptual color (OKLab/OKLch) | **Buy `palette`, feature-gated**; sRGB stays the Flutter-parity default | opt-in perceptual `ColorTween` is a visible quality win; gating keeps the default dep-light |
| Matrix4 interpolation | **Reuse workspace `glam`** (decompose → slerp) | already in the tree; correct quaternion path |
| Curve catalog / easing | **First-party** | one math source of truth; const-LUT wants our own table |
| `#[derive(Animatable)]` | **flui-macros** (existing crate) | no new crate |
| Spline / path motion (`kurbo`) | **Not now** | only if motion-path animation lands on the roadmap |

---

## 4. Core trait shapes (load-bearing signatures)

```rust
// ---- flui-geometry (Layer-0): the interpolation substrate ----

/// Total linear interpolation. Data-plane-neutral: NO Send+Sync bound.
/// Implementations MUST extrapolate (no clamp) — overshoot is a feature.
pub trait Lerp: Clone {
    fn lerp(&self, other: &Self, t: f32) -> Self;
}

/// Fallible interpolation for types that don't always interpolate
/// (Decoration of different kinds, Gradient of different stop counts, TextStyle).
pub trait MaybeLerp: Clone {
    fn maybe_lerp(a: &Self, b: &Self, t: f32) -> Option<Self>;
}

// blanket over numeric geometry units; hand impls for Offset/Size/Rect/RRect/Matrix4…
impl<U: GeometryOps> Lerp for U { fn lerp(&self, o: &Self, t: f32) -> Self { GeometryOps::lerp(*self, *o, t) } }

// ---- flui-animation: animatable + tween ----

/// Maps animation progress t (NOT clamped) to a value. Flutter's Animatable<T>.
pub trait Animatable<T> {
    fn transform(&self, t: f32) -> T;     // was `lerp(&self,t)` — renamed to avoid Lerp collision
    fn chain<U>(self, parent: impl Animatable<f32>) -> /* ChainedAnimatable */;
}

/// One generic tween replaces FloatTween/ColorTween/SizeTween/RectTween/...
pub struct Tween<V: Lerp> { pub begin: V, pub end: V }
pub type FloatTween  = Tween<f32>;
pub type ColorTween  = Tween<Color>;
pub type OffsetTween = Tween<Offset<Pixels>>;
// IntTween/StepTween stay distinct (rounding/flooring output, not pure Lerp)

impl<V: Lerp> Animatable<V> for Tween<V> {
    fn transform(&self, t: f32) -> V {
        if t == 0.0 { return self.begin.clone(); }   // exact endpoints, once
        if t == 1.0 { return self.end.clone(); }
        self.begin.lerp(&self.end, t)                 // NO clamp — extrapolates
    }
}

// ---- flui-animation: the animation trait (object-safe, Send+Sync exception) ----

pub trait Animation<T>: Listenable + Send + Sync + fmt::Debug
where T: Clone + Send + Sync + 'static {
    fn value(&self) -> T;
    fn status(&self) -> AnimationStatus;
    // status listeners now delegate to the embedded ListenerRegistry
}

// ---- flui-foundation: the unified listener machinery ----

pub struct ListenerRegistry { /* value Vec, status Vec, single shared counter */ }
impl ListenerRegistry {
    pub fn on_first_listener(&self, f: impl FnMut() + Send + 'static);  // wire parent subscription here
    pub fn on_last_listener(&self, f: impl FnMut() + Send + 'static);   // tear down here
    pub fn add_value(&self, cb: ListenerCallback) -> Subscription;       // RAII Drop
    pub fn notify_value(&self);                                          // snapshot, drop-lock, catch_unwind
}
pub struct Subscription { /* removes itself on Drop — fixes B2 structurally */ }
pub struct Notifier<Arg> { /* typed status channel, same hardened discipline */ }

// ---- flui-animation: the spring differentiator ----

/// T <-> fixed-width scalar vector, for per-component springs. Derivable.
/// Associated-type `Vector` (concrete `[f32; N]` per impl) keeps this on STABLE
/// Rust — `const N` + `[f32; Self::N]` would require unstable generic_const_exprs.
pub trait TwoWayConverter {
    type Vector: Copy + AsRef<[f32]> + AsMut<[f32]>;   // e.g. [f32; 1], [f32; 4]
    fn to_vector(&self) -> Self::Vector;
    fn from_vector(v: Self::Vector) -> Self;
    fn distance(a: &Self::Vector, b: &Self::Vector) -> f32;  // unit-aware rest epsilon
}
// impl TwoWayConverter for f32 { type Vector = [f32;1]; … }
// impl TwoWayConverter for Color { type Vector = [f32;4]; … }  // derive generates this

/// Interruptible spring-animated value. animate_to re-seeds from running (x, dx).
pub struct AnimatedValue<T: TwoWayConverter> { /* per-component SpringSimulation[(pos,vel)] */ }
impl<T: TwoWayConverter + Clone> AnimatedValue<T> {
    pub fn animate_to(&self, target: T);            // velocity-preserving retarget
}

impl SpringDescription {
    pub fn smooth() -> Self;                          // Apple iOS17 presets
    pub fn snappy() -> Self;
    pub fn bouncy() -> Self;
    pub fn with_response_and_damping(response: f32, damping_fraction: f32) -> Self;  // documented default
}
```

---

## 5. Module structure (post-redesign)

```
flui-geometry/src/
  lerp.rs           (NEW)  Lerp + MaybeLerp traits, blanket over GeometryOps, hand impls
  matrix.rs/…       (EDIT) Matrix4::lerp via glam decompose→slerp
flui-types/src/
  …                 (EDIT) impl Lerp/MaybeLerp for Color/Alignment/BorderRadius/TextStyle/Decoration/Gradient
flui-macros/src/
  animatable.rs     (NEW)  #[derive(Animatable)] / TwoWayConverter
flui-foundation/src/
  listener_registry.rs (NEW) ListenerRegistry + Subscription + Notifier<Arg>
flui-animation/src/
  animation.rs      (EDIT) Animation<T> trait; delete DynAnimation
  controller.rs     (EDIT) rescue: real tick, per-run duration, drop-lock-notify, repeat/animateBack/fling, dt accumulator, lifecycle gating
  listener.rs       (NEW)  re-export/compose ListenerRegistry into animation types
  tween.rs          (EDIT) generic Tween<V: Lerp> + aliases; remove clamp; partition_point TweenSequence
  tween_types.rs    (DELETE most) collapsed into tween.rs; keep IntTween/StepTween
  curve.rs          (EDIT) const-LUT presets; delete dead ParametricCurve; Curve2D/CatmullRom kept
  combinator/       (EDIT) curved/reverse/compound/proxy/switch/train_hopping — re-emit via registry
  simulation.rs     (EDIT) friction.through, drag∈(0,1) clamp, f64 spring constants
  scroll_physics.rs (NEW)  ScrollSpring/Clamped/BoundedFriction
  spring.rs         (NEW)  AnimatedValue<T>, presets, retarget
  constant.rs       (EDIT) zero-alloc const animations as associated constants
flui-view/src/
  …                 (EDIT) create_mark_needs_paint_callback (paint-only seam) + integration test
```

---

## 6. PR sequence (8 PRs — engine-complete, no widgets)

Each PR lands green, fully complete, test-first where the path is dynamic (current dynamic-path coverage is ~0).

1. **PR-1 Foundation — ListenerRegistry.** `ListenerRegistry` (value+status, shared counter, hardened, on_first/last hooks, RAII `Subscription`) + `Notifier<Arg>` in flui-foundation. Test-first. *Exit:* registry unit tests (order, panic-isolation, remove-during-notify, drop teardown, first/last edge) green.
2. **PR-2 Controller rescue.** Fix B1/B1b/B1c in discrete commits behind frame-advance + re-entrancy tests; `repeat(count, period, min, max)` + `animateBack` + `fling`; frame-coherent `dt` accumulator seeded from vsync timestamp × `time_dilation()`, clamped to `k·frame_duration`; lifecycle gating (mute Hidden/Paused, reset epoch on Resume); delete dead scheduler field; route diagnostics through `flui_foundation::log`; embed PR-1 registry; ADR-0002 exception note. *Exit:* `animate_to` advances value across N ticks; base duration intact after timed run; re-entrant status callback no deadlock.
3. **PR-3 Lerp + generic Tween.** `Lerp`/`MaybeLerp` in flui-geometry (blanket + hand impls); flui-types impls; collapse `*Tween` → `Tween<V>` + aliases; remove clamp + once-only endpoint short-circuit; keep `IntTween`/`StepTween` distinct; `Color::lerp` u8 round (not truncate); `TweenSequence` `partition_point`. *Exit:* overshoot curve passes through a tween un-flattened; all old `*Tween` call sites migrated; no `Lerp::lerp`/tween-method collision.
4. **PR-4 Combinator re-emit.** Wire curved/reverse/compound/proxy/switch to subscribe-and-re-emit via `on_first_listener` + Drop; `CompoundAnimation` listens to both children + combined status; delete `DynAnimation`; const-LUT `Cubic`; delete dead `ParametricCurve`. *Exit:* `AnimatedBuilder`-equivalent listener on a `CurvedAnimation(controller)` fires on every tick; no leaked tickers after combinator drop.
5. **PR-5 MaybeLerp + missing tweens.** `Matrix4::lerp` (glam decompose→slerp) in flui-geometry; `RelativeRect`; `TextStyleTween`; `DecorationTween`/`GradientTween` (MaybeLerp). *Exit:* a `Matrix4` rotate+scale interpolates without shear artifacts; mismatched decorations return `None` cleanly.
6. **PR-6 Spring differentiator.** Reconcile `with_duration_and_bounce` `bounce<0` to Apple's exact damping; `AnimatedValue<T>` per-component springs + velocity-preserving retarget; `#[derive(Animatable)]`/`TwoWayConverter` in flui-macros; Apple `smooth/snappy/bouncy` + `with_response_and_damping` as documented default; f64-internal constants; unit-aware rest epsilon. *Exit:* mid-flight `animate_to` preserves velocity (no visible snap); spring settles (no infinite tick) for both opacity-scale and px-scale targets.
7. **PR-7 Scroll physics.** `ScrollSpringSimulation`, `ClampedSimulation`, `BoundedFrictionSimulation`, `FrictionSimulation::through`, `drag∈(0,1)` assert/clamp (fix the never-terminate hang). *Exit:* friction with `drag≥1` rejected; bounded friction reproduces `BouncingScrollPhysics` boundary behavior in a golden test.
8. **PR-8 Paint-only seam + credibility.** `create_mark_needs_paint_callback` in flui-view + listener→`add_node_needing_paint` bridge + paint-vs-layout partition + direct integration test (repaint-not-rebuild); committed Criterion benches replacing the fabricated numbers; doc-accuracy sweep (`TweenAnimation::new` arg order, Mutex-not-RwLock, mark the fictional widget-layer claim). *Exit:* a transform/opacity transition triggers `add_node_needing_paint` and **not** a rebuild, proven by counter assertions; benches reproduce committed numbers.

**Deferred (own tracks, not in this cycle):** ADR-0002 Phase-1 `!Send` flip; `AnimatedFoo`/`AnimatedBuilder`/`TickerProvider` widgets in flui-view; flui-reactivity signal bridge.

---

## 7. Test strategy

- **Dynamic-path coverage is the #1 gap** (currently ~0). PR-1 and PR-2 are test-first.
- Frame-advance tests: a fake/manual `Ticker` driver to step N frames and assert value trajectory (not just listener firing).
- Re-entrancy tests: status callback that calls back into the controller — must not deadlock (B1c).
- Combinator tests: assert the *value listener* re-emits through curved/reverse/compound (B2), not just `status()`.
- Overshoot tests: `elasticOut`/`easeOutBack` through a `Tween` must exceed [begin,end] (B3 regression).
- Spring tests: velocity continuity across retarget; settling within tolerance for unit-divergent targets (epsilon correctness).
- Golden tests: scroll-physics boundary behavior; curve LUT vs bisection parity on steep cubics.
- Benches (Criterion, committed): per-frame `value()/status()`, combinator read, curve eval (LUT vs bisection), spring step, paint-seam vs rebuild.

## 8. Risks & mitigations

| Risk | Mitigation |
|------|-----------|
| ListenerRegistry shared-counter semantics wrong → lazy combinators leak/stall tickers | Test-first (PR-1); explicit first/last-edge tests; RAII Drop teardown |
| Lerp method-name collision + inconsistent inherent signatures | Rename tween method to `transform`; per-impl blanket adapter; migrate all call sites (no shim) |
| f32 spring cancellation near critical / long springs | f64-internal solution constants; reconcile bounce<0 to Apple exact damping |
| Unit-blind rest epsilon → infinite tick or visible snap | `TwoWayConverter::distance` per type |
| const-fn LUT precision on steep cubics under Rust 1.95 | Fall back to `build.rs` table generation; LUT-vs-bisection parity test |
| Paint seam interior-mutability / mid-frame re-entrancy | Capture `Arc`+`Copy RenderId`; defer mark if mid build-scope |
| Scope creep into widgets | Hard stop at the engine; paint seam validated by integration test, not a widget |

## 9. ADR-0002 interaction

ADR-0002 puts the Send boundary below bindings and flags the Phase-1 `!Send`-flip as high-ROI. This redesign **agrees with that intent** but defers the flip: the flui-scheduler Ticker is Send-bound at the source, so flipping the controller alone is impossible without the engine-wide scheduler change that *is* Phase-1. We ship a **recorded scoped exception** (controller stays `Send+Sync` `Arc<Mutex>`, bounds via `WasmNotSendSync`) and keep the design `!Send`-ready (RAII Drop, single lock boundary, no `Send+Sync` on the `Lerp` data trait) so the eventual flip is mechanical. Amendment note to be added to `docs/adr/ADR-0002-engine-wide-threading-architecture.md` in PR-2.
