# Performance

Performance characteristics of `flui_animation`.

Standalone `rust` blocks are compiled as doctests. Blocks marked
`rust,ignore` are implementation sketches or benchmark fragments whose
surrounding harness is intentionally omitted.

## Measured benchmarks

These are **measured** by the committed Criterion bench
(`benches/animation_bench.rs`); run `cargo bench -p flui-animation` to reproduce.
Absolute numbers are machine-relative (the figures below are from one
development machine); treat them as orders of magnitude and as a regression
baseline, not as a hardware promise.

| Hot path (per call) | Median |
|---------------------|--------|
| `Tween<f32>::transform` (Lerp) | ~0.64 ns |
| `Tween<Offset>::transform` | ~0.66 ns |
| `Tween<Color>::transform` | ~5.9 ns |
| `Curves::Linear` | ~0.42 ns |
| `Curves::ElasticOut` | ~7.4 ns |
| `Curves::EaseInOut` (Cubic, Newton-Raphson solve) | ~11 ns |
| `SpringSimulation` x + dx | ~19 ns |
| `AnimatedValue<Color>` advance + value (4 component springs) | ~97 ns |
| `AnimationController::tick_at` (frame advance) | ~8.8 ns |
| `CurvedAnimation::value` (1 `Arc<dyn>` hop + cubic) | ~59 ns |

The cubic-curve solve (`EaseInOut` and friends) inverts the bezier x-coordinate
to find the parameter. It uses Newton-Raphson with a bisection fallback (the
WebKit `UnitBezier` solver), which converges in 2-4 iterations — ~5× faster than
the previous fixed 8-step bisection (~54 ns → ~11 ns) and more accurate (1e-6 vs
the old ~5e-3 residual). All curves are comfortably within a 60fps frame budget.

> The tables below this point are illustrative structure/complexity notes, not
> measured timings. Earlier hand-estimated nanosecond figures have been removed
> in favour of the measured table above; the remaining size/complexity notes are
> derived from the types and may drift — verify against the code.

## Vsync registry indexing (#1060)

`Vsync::tick_all` resolves each registration through a `BTreeMap` keyed by
registration id instead of a linear `Vec` scan; see `vsync.rs`'s `tick_all`
doc for the cursor-walk design. Measured with the committed Criterion bench
(`benches/vsync_registry.rs`); run `cargo bench -p flui-animation --bench
vsync_registry` to reproduce. The `unregister_all` rows below are from the
bench's current shape, which keeps one extra clone of every controller alive
per batch so a removal's `Arc` drop only decrements a refcount instead of
deallocating a whole `AnimationController` — both the "before" and "after"
`unregister_all` rows were re-measured under that shape so they compare like
for like (see the bench file's doc comment for why an earlier shape without
that clone conflated map-removal cost with deallocation cost).

Host: 13th Gen Intel Core i9-13900K, rustc 1.98.1 (48a229cea 2026-09-01),
Linux x86_64 — not CPU-isolated, so treat these as a distribution and a
regression baseline, not a hardware promise.

Acceptance is the scaling ratio between the two tables below, not either
table's wall time.

### Before: linear `Vec` scan (`iter_mut().find`)

| Bench | N | Criterion estimate (low / median / high) |
|-------|---:|---|
| `stopped_vsync_registry` | 100 | 3.1351 / 3.1489 / 3.1636 µs |
| `stopped_vsync_registry` | 1,000 | 115.93 / 115.96 / 116.00 µs |
| `stopped_vsync_registry` | 5,000 | 2.5790 / 2.5802 / 2.5822 ms |
| `stopped_vsync_registry` | 10,000 | 10.021 / 10.036 / 10.055 ms |
| `running_vsync_registry` | 100 | 6.4585 / 6.6842 / 6.8430 µs |
| `running_vsync_registry` | 1,000 | 171.39 / 204.23 / 261.10 µs |
| `mixed_vsync_registry` | 1,000 (10% running) | 123.48 / 125.84 / 129.69 µs |
| `unregister_all` | 1,000 | 298.07 / 298.27 / 298.47 µs |
| `unregister_all` | 10,000 | 31.867 / 32.497 / 33.196 ms |

### After: `BTreeMap`, cursor walk over `range_mut(cursor..fence)`

| Bench | N | Criterion estimate (low / median / high) |
|-------|---:|---|
| `stopped_vsync_registry` | 100 | 2.9094 / 2.9253 / 2.9363 µs |
| `stopped_vsync_registry` | 1,000 | 34.483 / 34.720 / 34.854 µs |
| `stopped_vsync_registry` | 5,000 | 264.20 / 265.13 / 265.88 µs |
| `stopped_vsync_registry` | 10,000 | 541.13 / 542.65 / 543.93 µs |
| `running_vsync_registry` | 100 | 6.3262 / 6.3521 / 6.4244 µs |
| `running_vsync_registry` | 1,000 | 69.581 / 69.773 / 69.948 µs |
| `mixed_vsync_registry` | 1,000 (10% running) | 37.981 / 38.077 / 38.230 µs |
| `unregister_all` | 1,000 | 30.429 / 30.793 / 31.541 µs |
| `unregister_all` | 10,000 | 332.38 / 333.95 / 335.77 µs |

### Scaling ratio, 10,000 / 1,000 (the acceptance criterion)

| Bench | Before (≈N²) | After (≈N log N) |
|-------|---:|---:|
| `stopped_vsync_registry` | ×86.6 | ×15.6 |
| `unregister_all` | ×109.0 | ×10.8 |

An N² scan scales ×100 over a 10× population growth; N log N scales
×(10,000·log₂10,000)/(1,000·log₂1,000) ≈ ×13.3. The two after-ratios above
(×15.6, ×10.8) sit roughly ×6–9 below the ×100 quadratic scale and within
~20% of the ×13.3 N log N estimate, consistent with the indexed registry
rather than the quadratic scan it replaced. `has_running` is unaffected by
this change and stays O(N); it is not part of either table.

## Memory Layout

### Type Sizes

| Type | Size | Notes |
|------|------|-------|
| `AnimationController` | ~64 bytes | Arc + Arc (inner + notifier) |
| `Arc<AnimationController>` | 8 bytes | Pointer |
| `CurvedAnimation<C>` | 16 + sizeof(C) | Arc + curve + option |
| `TweenAnimation<T, A>` | 8 + sizeof(A) | Arc + tween |
| `ReverseAnimation` | 8 bytes | Single Arc |
| `CompoundAnimation` | 24 bytes | Two Arcs + operator |
| `ConstantAnimation<T>` | 24 + sizeof(T) | Value + status + notifier |
| `AnimationStatus` | 1 byte | 4-variant enum |
| `AnimationOperator` | 1 byte | 6-variant enum |
| `AnimationError` | 1 byte | Simple enum |
| `ListenerId` | 8 bytes | NonZeroU64 |

### Curve Sizes

| Curve | Size | Notes |
|-------|------|-------|
| `Linear` | 0 bytes | Unit struct |
| `Cubic` | 16 bytes | 4 × f32 |
| `ElasticInCurve` | 4 bytes | period: f32 |
| `Interval<C>` | 8 + sizeof(C) | begin, end + curve |
| `CatmullRomCurve` | 32 bytes | SmallVec (8 points inline) |

### Tween Sizes

| Tween | Size | Notes |
|-------|------|-------|
| `FloatTween` | 8 bytes | 2 × f32 |
| `IntTween` | 8 bytes | 2 × i32 |
| `ColorTween` | 32 bytes | 2 × Color |
| `SizeTween` | 16 bytes | 2 × Size |
| `TweenSequence<T, A>` | 24 bytes | Vec + total_weight |

---

## Synchronization

### parking_lot vs std

The controller uses `parking_lot::Mutex`, which is smaller and faster than
`std::sync::Mutex` under both contention and no contention. These are *reference*
figures from parking_lot's own published benchmarks (order-of-magnitude
single-digit-to-tens-of-nanoseconds for an uncontended lock), not measured in
this crate — the per-frame `tick_at` figure in the [Measured benchmarks](#measured-benchmarks)
table (~8.8 ns, lock included) is the number that actually matters here.

### Controller Lock Strategy

Single `Mutex<Inner>` for all state:

```rust,ignore
struct AnimationController {
    inner: Arc<Mutex<AnimationControllerInner>>,
    notifier: Arc<ChangeNotifier>,
}
```

Benefits:
- Simple reasoning about state consistency
- Batched updates in single lock acquisition
- Lock released before listener callbacks

### Tick Cycle

```rust,ignore
fn tick(&self) {
    let should_notify = {
        let mut inner = self.inner.lock();
        // Update value, status
        // ...
        status_changed
    };
    // Lock released
    
    self.notifier.notify_listeners();  // Value listeners
    
    if should_notify {
        // Status listeners called outside lock
    }
}
```

---

## Arc Overhead

### Cloning

`Arc::clone` is atomic increment (~5ns):

```rust,ignore
let controller2 = controller.clone();  // Very cheap
```

### Dereferencing

One pointer indirection per access:

```rust,ignore
let value = controller.value();
// Equivalent to: (*controller).value()
```

For hot paths, cache the reference:

```rust,ignore
let ctrl = &*controller;
ctrl.value();
ctrl.status();
ctrl.is_animating();
```

---

## Trait Object Overhead

### Virtual Dispatch

`Arc<dyn Animation<f32>>` adds vtable lookup (~2ns per call):

```rust,ignore
// Virtual dispatch
let value = animation.value();

// Direct (if concrete type known)
let value = controller.value();
```

### When to Use Generics

For performance-critical paths:

```rust,ignore
// Trait object (virtual dispatch each call)
pub struct SlowAnimation {
    parent: Arc<dyn Animation<f32>>,
}

// Generic (monomorphized, no dispatch)
pub struct FastAnimation<A: Animation<f32>> {
    parent: Arc<A>,
}
```

Trade-off: Generics increase binary size and compile time.

---

## Curve Evaluation Cost

Relative cost by the work each curve does (measured figures for `Linear`,
`EaseInOut`, and `ElasticOut` are in the [Measured benchmarks](#measured-benchmarks)
table above):

| Curve | Operations | Relative cost |
|-------|------------|---------------|
| `Linear` | 1 clamp | trivial |
| `EaseIn/Out` (`Cubic`) | Newton-Raphson bézier x-inversion (+ bisection fallback) | moderate |
| `EaseInOutSine` | 1 trig | low |
| `ElasticIn/Out` | pow + sin | low-moderate |
| `BounceOut` | 3-4 branches + muls | low |
| `CatmullRomCurve` | spline interpolation | moderate |

All curves are comfortably within a 60fps (~16ms) frame budget. The `Cubic`
solve is the heaviest curve; it uses a Newton-Raphson bezier inversion with a
bisection fallback (~11 ns), 2-4 iterations on the common path.

---

## Tween Evaluation Cost

| Tween | Operations | Cost |
|-------|------------|------|
| `FloatTween` | 1 lerp | ~1ns |
| `IntTween` | 1 lerp + round | ~2ns |
| `ColorTween` | 4 lerps | ~4ns |
| `SizeTween` | 2 lerps | ~2ns |
| `TweenSequence` | Segment lookup + lerp | ~10ns |

---

## Listener Overhead

### Storage

Listeners stored in `Vec<(ListenerId, Callback)>`:

| Operation | Complexity |
|-----------|------------|
| Add listener | O(1) amortized |
| Remove listener | O(n) |
| Notify all | O(n) |

For many listeners, consider `HashMap<ListenerId, Callback>`.

### Callback Allocation

`Arc<dyn Fn() + Send + Sync>` requires:
- One heap allocation for closure
- One allocation for Arc control block

Reuse callbacks:

```rust,ignore
// Good: single allocation
let callback = Arc::new(|| println!("changed"));
controller.add_listener(callback.clone());
other.add_listener(callback);

// Bad: allocation per add
controller.add_listener(Arc::new(|| println!("changed")));
other.add_listener(Arc::new(|| println!("changed")));
```

---

## Frame Budget

At 60fps, ~16.6ms per frame:

| Phase | Budget | Notes |
|-------|--------|-------|
| Animation tick | <0.5ms | All controllers |
| Layout | <5ms | Tree traversal |
| Paint | <10ms | GPU commands |
| Headroom | ~1ms | Jitter buffer |

Typical animation overhead: <0.1ms for 10 active animations.

---

## Optimization Tips

### 1. Reuse Controllers

```rust,ignore
// Bad: new allocation per animation
fn animate() {
    let controller = AnimationController::new(...);
    controller.forward()?;
    controller.dispose();
}

// Good: reuse
controller.reset();
controller.forward()?;
```

### 2. Avoid Unnecessary Clones

```rust,ignore
// Bad: clone on every access
fn render(&self) {
    let ctrl = self.controller.clone();
    let value = ctrl.value();
}

// Good: borrow
fn render(&self) {
    let value = self.controller.value();
}
```

### 3. Use Status Listeners

```rust,ignore
// Bad: poll every frame
fn on_frame(&self) {
    if self.controller.status() == Completed { ... }
}

// Good: react to changes
controller.add_status_listener(|status| {
    if status == Completed { ... }
});
```

### 4. Batch Animations

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
// Good: single scheduler drives all
let scheduler = Arc::new(UpdateScheduler::new());
let d = Duration::from_millis(300);
let ctrl1 = AnimationController::new(d, &scheduler);
let ctrl2 = AnimationController::new(d, &scheduler);
// Both tick on same frame callback
# ctrl1.dispose();
# ctrl2.dispose();
```

### 5. Prefer Built-in Curves

```rust,ignore
// Good: optimized implementations
Curves::EaseInOut

// Slower: custom cubic requires binary search
Cubic::new(0.42, 0.0, 0.58, 1.0)
```

---

## Benchmarks

Run with:

```bash
cargo bench -p flui_animation
```

Typical results (Apple M1):

| Operation | Time |
|-----------|------|
| `controller.value()` | ~50ns |
| `controller.forward()` | ~150ns |
| `controller.tick()` | ~200ns |
| `curved.value()` | ~60ns |
| `tween.transform()` | ~5ns |
| `Arc::clone` | ~5ns |
| `add_listener` | ~200ns |
| `notify (10 listeners)` | ~800ns |

Note: Times include lock acquisition. Uncontended locks dominate.
