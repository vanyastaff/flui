# Design Patterns

Patterns used in `flui_animation` and their rationale.

## Persistent Object Pattern

### Problem

React-style hooks recreate state each render. Animations need continuous state across rebuilds.

### Solution

Animations are long-lived objects with explicit lifecycle:

```rust
// Created once
let controller = AnimationController::new(duration, scheduler);

// Used across rebuilds
controller.forward()?;
controller.reverse()?;

// Explicit cleanup
controller.dispose();
```

### Flutter Equivalent

```dart
class _State extends State<W> with SingleTickerProviderStateMixin {
  late AnimationController _controller;
  
  @override
  void initState() {
    _controller = AnimationController(duration: Duration(ms: 300), vsync: this);
  }
  
  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }
}
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
    parent: Arc<dyn Animation<f32>>,
    curve: C,
}

pub struct TweenAnimation<T, A: Animatable<T>> {
    parent: Arc<dyn Animation<f32>>,
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
let controller = AnimationController::builder(duration, scheduler)
    .bounds(0.0, 100.0)?      // Validates immediately
    .reverse_duration(Duration::from_millis(500))
    .initial_value(50.0)?
    .build()?;
```

### Why Result in Builder Methods?

Unlike builders that defer validation to `build()`, we validate immediately:

```rust
pub fn bounds(mut self, lower: f32, upper: f32) -> Result<Self, AnimationError> {
    if lower >= upper {
        return Err(AnimationError::InvalidBounds);
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

## Extension Trait Pattern

### Problem

Adding convenience methods to core types bloats their API.

### Solution

Extension traits for fluent composition:

```rust
pub trait AnimationExt: Animation<f32> + Sized + 'static {
    fn curved<C: Curve>(self: Arc<Self>, curve: C) -> Arc<CurvedAnimation<C>> {
        Arc::new(CurvedAnimation::new(self, curve))
    }
    
    fn reversed(self: Arc<Self>) -> Arc<ReverseAnimation> {
        Arc::new(ReverseAnimation::new(self))
    }
}

impl<A: Animation<f32> + 'static> AnimationExt for A {}
```

### Usage

```rust
use flui_animation::AnimationExt;

let animation = Arc::new(controller)
    .curved(Curves::EaseInOut)
    .reversed();
```

### Benefits

- Core types stay focused
- Optional import
- Easy to extend

---

## Type Erasure Pattern

### Problem

Generic animations need uniform storage and handling.

### Solution

Use `Arc<dyn Animation<T>>`:

```rust
pub struct Container {
    animations: Vec<Arc<dyn Animation<f32>>>,
}

impl Container {
    fn add<A: Animation<f32> + 'static>(&mut self, anim: A) {
        self.animations.push(Arc::new(anim));
    }
}
```

### DynAnimation Trait

For collections requiring both Animation and Listenable:

```rust
pub trait DynAnimation<T>: Animation<T> + Listenable {}

impl<T, A> DynAnimation<T> for A
where
    T: Clone + Send + Sync + 'static,
    A: Animation<T> + Listenable + ?Sized,
{}
```

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
pub fn forward(&self) -> Result<(), AnimationError> {
    let inner = self.inner.lock();
    if inner.disposed {
        return Err(AnimationError::AlreadyDisposed);
    }
    // ...
}
```

### Why Not Drop?

- `Drop` can't return errors
- `Drop` takes `&mut self`, incompatible with `Arc<Self>`
- Explicit disposal is idempotent (safe to call multiple times)

---

## Validated Construction Pattern

### Problem

Invalid parameters (zero mass, negative duration) cause runtime failures.

### Solution

Validate in constructors, panic on violation:

```rust
impl SpringDescription {
    pub fn new(mass: f32, stiffness: f32, damping: f32) -> Self {
        assert!(mass > 0.0, "Mass must be positive");
        assert!(stiffness > 0.0, "Stiffness must be positive");
        assert!(damping >= 0.0, "Damping must be non-negative");
        Self { mass, stiffness, damping }
    }
}

impl FrictionSimulation {
    pub fn new(drag: f32, position: f32, velocity: f32) -> Self {
        assert!(drag > 0.0, "Drag must be positive");
        assert!((drag - 1.0).abs() > 1e-6, "Drag cannot be 1.0");
        // ...
    }
}
```

### Boundary Guarantees

Curves guarantee exact boundary values:

```rust
impl Curve for ElasticInCurve {
    fn transform(&self, t: f32) -> f32 {
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
