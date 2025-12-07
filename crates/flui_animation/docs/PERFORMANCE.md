# Animation Performance

This document describes performance characteristics and optimization techniques in `flui_animation`.

## Memory Layout

### Type Sizes

| Type | Size | Notes |
|------|------|-------|
| `AnimationController` | ~256 bytes | Main struct with all state |
| `Arc<AnimationController>` | 8 bytes | Pointer only |
| `CurvedAnimation<C>` | 16 + size_of::<C>() | Arc + curve |
| `TweenAnimation<T, A>` | 8 + size_of::<A>() | Arc + tween |
| `ReverseAnimation` | 8 bytes | Single Arc |
| `CompoundAnimation` | 24 bytes | Two Arcs + operator |
| `AnimationDirection` | 1 byte | Enum with 2 variants |
| `AnimationOperator` | 1 byte | Enum with 4 variants |
| `AnimationError` | 24 bytes | Enum with String variant |
| `ListenerId` | 8 bytes | NonZeroU64 |

### Synchronization Strategy

The controller uses a single `Mutex<AnimationControllerInner>` for all mutable state:

```rust
pub struct AnimationController {
    inner: Arc<Mutex<AnimationControllerInner>>,
    notifier: Arc<ChangeNotifier>,
}
```

This provides strong safety guarantees with good performance for typical animation workloads:
- Uncontended lock acquisition: ~10ns (parking_lot)
- State access batching: Multiple reads/writes in single lock acquisition
- Lock held only during state updates, not during listener callbacks

## Synchronization

### parking_lot vs std

We use `parking_lot` instead of `std::sync`:

| Operation | std::sync | parking_lot | Improvement |
|-----------|-----------|-------------|-------------|
| Uncontended lock | ~25ns | ~10ns | 2.5x |
| Contended lock | ~100ns | ~40ns | 2.5x |
| RwLock read | ~30ns | ~12ns | 2.5x |

### Optimization Techniques

The controller minimizes lock contention through several techniques:

1. **Batched State Access**: All state reads/writes happen within a single lock acquisition
2. **Lock-Free Notifications**: Listeners are notified after releasing the lock
3. **Optimized tick() Method**: Single lock acquisition per frame, immediate unlock before callbacks
4. **ScheduledTicker Integration**: Automatic frame scheduling via `flui-scheduler`

### Avoiding Lock Contention

The animation loop avoids holding locks during callbacks:

```rust
fn notify_listeners(&self) {
    // Clone listeners under lock
    let listeners: Vec<_> = self.listeners.read().clone();
    
    // Call callbacks outside lock
    for (_, callback) in listeners {
        callback();
    }
}
```

## Arc Overhead

### Cloning Cost

`Arc::clone` is a single atomic increment (~5ns):

```rust
// Very cheap
let controller2 = controller.clone();
```

### Dereferencing Cost

Accessing through Arc adds one pointer indirection:

```rust
// One indirection
let value = controller.value();

// Same as
let value = (*controller).value();
```

For hot paths, consider caching the reference:

```rust
// If calling many methods
let ctrl = &*controller;
ctrl.value();
ctrl.status();
ctrl.is_animating();
```

## Trait Object Overhead

### Virtual Dispatch

Using `Arc<dyn Animation<f32>>` adds vtable lookup (~2ns per call):

```rust
// Virtual dispatch
let value = (parent as &dyn Animation<f32>).value();

// vs direct call (if type known)
let value = controller.value();
```

### When to Use Generics

For performance-critical code, use generics instead of trait objects:

```rust
// Trait object (virtual dispatch)
pub struct SlowAnimation {
    parent: Arc<dyn Animation<f32>>,
}

// Generic (monomorphized, no dispatch)
pub struct FastAnimation<A: Animation<f32>> {
    parent: Arc<A>,
}
```

However, generics increase code size and compilation time. The vtable overhead is usually negligible for animations.

## Callback Performance

### Allocation

`StatusCallback = Arc<dyn Fn(AnimationStatus) + Send + Sync>` involves:
- One allocation for the closure
- One allocation for Arc control block

Create callbacks once and reuse:

```rust
// Good - single allocation
let callback = Arc::new(|status| println!("{:?}", status));
controller.add_status_listener(callback.clone());
other_controller.add_status_listener(callback);

// Bad - multiple allocations
controller.add_status_listener(Arc::new(|s| println!("{:?}", s)));
other_controller.add_status_listener(Arc::new(|s| println!("{:?}", s)));
```

### Listener Removal

Listeners are stored in a `Vec`, so removal is O(n):

```rust
pub fn remove_status_listener(&self, id: ListenerId) {
    self.status_listeners.write().retain(|(lid, _)| *lid != id);
}
```

For many listeners, consider using a `HashMap<ListenerId, Callback>` instead.

## Curve Evaluation

### Polynomial Curves

Most curves use polynomial evaluation, which is very fast:

```rust
// EaseIn: t^2 (one multiply)
fn transform(&self, t: f32) -> f32 {
    t * t
}

// EaseInOut: cubic (few operations)
fn transform(&self, t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}
```

### Expensive Curves

Some curves are more expensive:

| Curve | Operations | Notes |
|-------|------------|-------|
| `Linear` | 0 | Identity |
| `EaseIn/Out` | 1-3 | Polynomial |
| `EaseInOutSine` | 1 trig | `cos()` call |
| `ElasticIn/Out` | 2 trig + pow | `sin()`, `pow()` |
| `EaseInBack` | 3-4 | Polynomial with overshoot |

For smooth 60fps, even elastic curves are fast enough (<1μs).

## Tween Evaluation

### Simple Tweens

Linear interpolation is very fast:

```rust
impl Animatable<f32> for FloatTween {
    fn transform(&self, t: f32) -> f32 {
        self.begin + (self.end - self.begin) * t
    }
}
```

### Complex Tweens

Color tweens may involve color space conversion:

```rust
impl Animatable<Color> for ColorTween {
    fn transform(&self, t: f32) -> Color {
        // RGB interpolation (fast)
        Color::rgba(
            lerp(self.begin.r, self.end.r, t),
            lerp(self.begin.g, self.end.g, t),
            lerp(self.begin.b, self.end.b, t),
            lerp(self.begin.a, self.end.a, t),
        )
    }
}
```

For perceptually uniform color interpolation, consider HSL or LAB space (more expensive).

## Frame Timing

### Target Frame Rate

At 60fps, each frame has ~16.6ms budget:

| Phase | Target | Notes |
|-------|--------|-------|
| Animation tick | <0.1ms | All controllers |
| Layout | <5ms | Tree traversal |
| Paint | <10ms | GPU commands |
| Buffer | ~1.5ms | Headroom |

Animation updates are typically <0.1ms even with many controllers.

### Batching Updates

Multiple animations should batch their updates:

```rust
// Scheduler batches all ticker callbacks
scheduler.on_frame(|delta| {
    // All animations update together
    for controller in &controllers {
        controller.tick(delta);
    }
    
    // Single notification pass
    notify_all_listeners();
});
```

## Optimization Tips

### 1. Reuse Controllers

```rust
// Bad - new allocation each animation
fn animate() {
    let controller = AnimationController::new(...);
    // ...
    controller.dispose();
}

// Good - reuse existing controller
controller.reset()?;
controller.forward()?;
```

### 2. Avoid Unnecessary Clones

```rust
// Bad - clone on every access
fn render(&self) {
    let ctrl = self.controller.clone();
    let value = ctrl.value();
}

// Good - borrow
fn render(&self) {
    let value = self.controller.value();
}
```

### 3. Use Status Listeners Sparingly

```rust
// Bad - check status every frame
fn on_frame(&self) {
    if self.controller.status() == AnimationStatus::Completed {
        // ...
    }
}

// Good - use status listener
self.controller.add_status_listener(Arc::new(|status| {
    if status == AnimationStatus::Completed {
        // ...
    }
}));
```

### 4. Prefer Curves Over Custom Interpolation

```rust
// Bad - custom easing in tween
impl Animatable<f32> for CustomTween {
    fn transform(&self, t: f32) -> f32 {
        let eased = t * t; // Duplicates curve logic
        self.begin + (self.end - self.begin) * eased
    }
}

// Good - compose curve and linear tween
let curved = controller.curved(Curves::EaseIn);
let animation = FloatTween::new(0.0, 100.0).animate(curved);
```

## Benchmarks

Run benchmarks with:

```bash
cargo bench -p flui_animation
```

Typical results (M1 Mac):

| Operation | Time |
|-----------|------|
| `controller.value()` | ~50-100ns |
| `controller.forward()` | ~100-200ns |
| `controller.tick()` | ~100-300ns |
| `curved.value()` | ~50-100ns |
| `tween.value()` | ~50-100ns |
| `add_listener` | ~150-300ns |
| `notify_listeners (10)` | ~500-1000ns |
| `Arc::clone` | ~5ns |

Note: Actual performance depends on lock contention and callback complexity.
