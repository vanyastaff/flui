# Performance

Performance characteristics of `flui_animation`.

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

| Operation | std::sync | parking_lot | Improvement |
|-----------|-----------|-------------|-------------|
| Uncontended Mutex lock | ~25ns | ~10ns | 2.5× |
| Contended Mutex lock | ~100ns | ~40ns | 2.5× |
| RwLock read | ~30ns | ~12ns | 2.5× |

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

| Curve | Operations | Cost |
|-------|------------|------|
| `Linear` | 1 clamp | ~1ns |
| `EaseIn/Out` | 2-3 muls | ~2ns |
| `Cubic` | 8 iterations binary search | ~50ns |
| `EaseInOutSine` | 1 trig | ~10ns |
| `ElasticIn/Out` | pow + sin | ~20ns |
| `BounceOut` | 3-4 branches + muls | ~5ns |
| `CatmullRomCurve` | Spline interpolation | ~30ns |

All curves are fast enough for 60fps (~16ms frame budget).

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
