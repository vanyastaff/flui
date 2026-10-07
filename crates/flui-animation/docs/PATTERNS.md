# Design Patterns

Patterns used in `flui_animation` and their rationale.

## Persistent Object Pattern

### Problem

React-style hooks recreate state each render. Animations need continuous state across rebuilds.

### Solution

Animations are long-lived objects with explicit lifecycle:

```rust
// Created once
let controller = AnimationController::new(duration, &scheduler);

// Used across rebuilds
controller.forward()?;
controller.reverse()?;

// Explicit cleanup
controller.dispose();
```

### Benefits

- Animations survive widget rebuilds
- Explicit control over timing
- Predictable memory management

---

## Composition Pattern

### Problem

Inheritance hierarchies are rigid and don't fit Rust's ownership model.

### Solution

Build complex animations by wrapping simpler ones:

```rust
let controller = Arc::new(AnimationController::new(...));
let curved = Arc::new(CurvedAnimation::new(controller, curve));
let color = TweenAnimation::new(curved, ColorTween::new(RED, BLUE));
```

### Implementation

Composed animations store `Arc<dyn Animation<T>>`:

```rust
pub struct CurvedAnimation<C: Curve> {
    parent: Arc<dyn Animation<f64>>,
    curve: C,
}

pub struct TweenAnimation<T, A: Animatable<T>> {
    parent: Arc<dyn Animation<f64>>,
    tween: A,
}
```

### Benefits

- Type-safe composition
- No inheritance hierarchies
- Clear ownership via Arc

---

## Builder Pattern

### Problem

Many optional configuration parameters. Multiple constructors become unwieldy.

### Solution

Builder with validation at each step:

```rust
let controller = AnimationController::builder(duration, &scheduler)
    .bounds(0.0, 100.0)?      // Validates immediately
    .reverse_duration(Duration::from_millis(500))
    .initial_value(50.0)
    .build()?;
```

### Why Result in Builder Methods?

Unlike builders that defer validation to `build()`, we validate immediately:

```rust
pub fn bounds(mut self, lower: f64, upper: f64) -> Result<Self, AnimationError> {
    if !(lower < upper) || !(upper - lower).is_finite() {
        return Err(AnimationError::InvalidBounds(format!("{lower}..{upper}")));
    }
    self.lower_bound = lower;
    self.upper_bound = upper;
    Ok(self)
}
```

Benefits:
- Fail fast — errors caught at misconfiguration point
- Better context — error location preserved
- No silent failures

---

## Keyframe Track Pattern

### Problem

A looping indicator moves several properties on one timeline, some delayed,
some offset per element. One controller per property or per element means
one vsync registration, listener and dispose path each.

### Solution

One repeating controller; each property is a `Keyframes` track with the same
`total`, sampled in `paint` at the controller's progress. A delay is a
leading `hold`; a per-element offset is `Stagger::delay`, read with
`value_at_looped(elapsed + total − delay)`. The tracks are immutable values,
so a paint that panics leaves nothing to repair.

```rust,ignore
let elapsed = rotation.total().mul_f64(controller.value());
for i in 0..count {
    let shifted = elapsed + track.total() - stagger.delay(i, count);
    draw_tick(i, track.value_at_looped(shifted));
}
```

---

## Type Erasure Pattern

### Problem

Generic animations need uniform storage and handling.

### Solution

Use `Arc<dyn Animation<T>>`:

```rust
pub struct Container {
    animations: Vec<Arc<dyn Animation<f64>>>,
}

impl Container {
    fn add<A: Animation<f64> + 'static>(&mut self, anim: A) {
        self.animations.push(Arc::new(anim));
    }
}
```

### Listening Through the Erased Type

`Animation<T>` has `Listenable` as a supertrait, so an `Arc<dyn Animation<T>>`
already exposes both value and status listeners; no extra combined trait is
needed.

---

## Callback Pattern

### Problem

Animations need to notify listeners on changes.

### Solution

Closures with `Send + Sync`:

```rust
pub type StatusCallback = Arc<dyn Fn(AnimationStatus) + Send + Sync>;

impl AnimationController {
    pub fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        let id = ListenerId::new();
        // Store (id, callback)
        id
    }
}
```

### Why Arc<dyn Fn>?

- **Shared** — stored and called multiple times
- **Thread-safe** — `Send + Sync` for multi-threaded UI
- **Flexible** — closures capture environment

### Listener IDs

Return IDs for later removal:

```rust
let id = controller.add_status_listener(callback);
controller.remove_status_listener(id);
```

---

## Disposal Pattern

### Problem

Controllers hold resources (tickers) requiring cleanup.

### Solution

Explicit `dispose()` with guard flag:

```rust
pub fn dispose(&self) {
    let mut inner = self.inner.lock();
    if inner.disposed {
        return;
    }
    inner.disposed = true;

    if let Some(ticker) = inner.ticker.take() {
        ticker.stop();
    }
    inner.status_listeners.clear();
}
```

### Guard Against Use After Dispose

```rust
pub fn forward(&self) -> Result<TickerFuture, AnimationError> {
    let inner = self.inner.lock();
    if inner.disposed {
        return Err(AnimationError::Disposed);
    }
    // ...
}
```

### Why Not Drop?

- Clones of an `AnimationController` share one controller, so dropping one
  handle cannot mean the animation is finished; `dispose()` ends it for every
  handle at once
- Explicit disposal is idempotent (safe to call multiple times)

---

## Validated Construction Pattern

### Problem

Invalid parameters (zero mass, negative duration) cause runtime failures.

### Solution

Validate in constructors and return a typed error; keep the fields private so
an invalid value cannot be built by literal:

```rust,ignore
impl SpringDescription {
    pub fn new(mass: f64, stiffness: f64, damping: f64) -> Result<Self, SimulationError> {
        let mass = positive(SimulationParameter::Mass, mass)?.sqrt();
        let stiffness = positive(SimulationParameter::Stiffness, stiffness)?.sqrt();
        let damping = positive(SimulationParameter::Damping, damping)?;
        Self::from_omega_zeta(stiffness / mass, damping / 2.0 / stiffness / mass)
    }
}
```

A panicking constructor is kept only for constants
(`SpringDescription::with_damping_ratio`), documented under `# Panics`.

### Boundary Guarantees

Curves guarantee exact boundary values:

```rust
impl Curve for ElasticInCurve {
    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        if t == 0.0 { return 0.0; }
        if t == 1.0 { return 1.0; }
        // ... elastic formula
    }
}
```

---

## Summary

| Pattern | Problem | Solution |
|---------|---------|----------|
| Persistent Object | Hooks don't work for animations | Long-lived objects with explicit lifecycle |
| Composition | Inheritance doesn't fit Rust | Wrap via `Arc<dyn Animation>` |
| Builder | Many optional parameters | Fluent builder with immediate validation |
| Extension Trait | API bloat | Optional fluent methods via traits |
| Type Erasure | Uniform handling | `Arc<dyn Animation<T>>` |
| Callback | Change notification | `Arc<dyn Fn + Send + Sync>` |
| Disposal | Resource cleanup | Explicit `dispose()` with guard |
| Validated Construction | Invalid parameters | Assert in constructors, guarantee boundaries |
