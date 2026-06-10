# Performance

Performance characteristics of `flui_animation`.

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
| `Curves::EaseInOut` (Cubic, 8-iter solve) | ~54 ns |
| `SpringSimulation` x + dx | ~19 ns |
| `AnimatedValue<Color>` advance + value (4 component springs) | ~97 ns |
| `AnimationController::tick_at` (frame advance) | ~8.8 ns |
| `CurvedAnimation::value` (1 `Arc<dyn>` hop + cubic) | ~59 ns |

The cubic-curve solve (`EaseInOut` and friends) is the dominant per-curve cost —
it runs a fixed Newton/bisection solve every frame. A compile-time lookup table
for the const presets is the planned optimization; until then, prefer cheaper
curves on very hot paths.

> The tables below this point are illustrative structure/complexity notes, not
> measured timings. Earlier hand-estimated nanosecond figures have been removed
> in favour of the measured table above; the remaining size/complexity notes are
> derived from the types and may drift — verify against the code.

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

```rust
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

```rust
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

```rust
let controller2 = controller.clone();  // Very cheap
```

### Dereferencing

One pointer indirection per access:

```rust
let value = controller.value();
// Equivalent to: (*controller).value()
```

For hot paths, cache the reference:

```rust
let ctrl = &*controller;
ctrl.value();
ctrl.status();
ctrl.is_animating();
```

---

## Trait Object Overhead

### Virtual Dispatch

`Arc<dyn Animation<f32>>` adds vtable lookup (~2ns per call):

```rust
// Virtual dispatch
let value = animation.value();

// Direct (if concrete type known)
let value = controller.value();
```

### When to Use Generics

For performance-critical paths:

```rust
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
| `EaseIn/Out` (`Cubic`) | fixed Newton/bisection bézier solve | highest |
| `EaseInOutSine` | 1 trig | low |
| `ElasticIn/Out` | pow + sin | low-moderate |
| `BounceOut` | 3-4 branches + muls | low |
| `CatmullRomCurve` | spline interpolation | moderate |

All curves are comfortably within a 60fps (~16ms) frame budget. The `Cubic`
solve dominates; a const lookup table for the preset cubics is the planned
optimization.

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

```rust
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

```rust
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

```rust
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

```rust
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
// Good: single scheduler drives all
let scheduler = Arc::new(Scheduler::new());
let ctrl1 = AnimationController::new(d, scheduler.clone());
let ctrl2 = AnimationController::new(d, scheduler.clone());
// Both tick on same frame callback
```

### 5. Prefer Built-in Curves

```rust
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
