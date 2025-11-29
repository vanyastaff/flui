# Animation Design Patterns

This document describes the design patterns used in `flui_animation` and their rationale.

## Table of Contents

- [Persistent Object Pattern](#persistent-object-pattern)
- [Composition Pattern](#composition-pattern)
- [Builder Pattern](#builder-pattern)
- [Extension Trait Pattern](#extension-trait-pattern)
- [Type-Erased Trait Objects](#type-erased-trait-objects)
- [Callback Pattern](#callback-pattern)
- [Disposal Pattern](#disposal-pattern)

## Persistent Object Pattern

### Problem

React-style hooks recreate state each render, which doesn't work for animations that need to maintain continuous state across rebuilds.

### Solution

Animations are persistent objects created once and used across widget rebuilds:

```rust
// Created once (outside widget build)
let controller = AnimationController::new(duration, scheduler);

// Used many times (in widget build)
let value = controller.value();

// Explicit cleanup
controller.dispose();
```

### Flutter Comparison

This matches Flutter's approach where `AnimationController` is created in `initState()` and disposed in `dispose()`:

```dart
// Flutter
class _MyWidgetState extends State<MyWidget> with SingleTickerProviderStateMixin {
  late AnimationController _controller;
  
  @override
  void initState() {
    super.initState();
    _controller = AnimationController(duration: Duration(milliseconds: 300), vsync: this);
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
- Explicit control over lifecycle
- Predictable memory management
- Easy to reason about animation state

## Composition Pattern

### Problem

Inheritance hierarchies are rigid and don't fit Rust's ownership model.

### Solution

Build complex animations by composing simple ones:

```rust
// Each layer wraps the previous
let controller = Arc::new(AnimationController::new(duration, scheduler));
let curved = Arc::new(CurvedAnimation::new(controller, Curves::EaseInOut));
let tween = TweenAnimation::new(ColorTween::new(RED, BLUE), curved);
```

### Implementation

All composed animations store `Arc<dyn Animation<T>>`:

```rust
pub struct CurvedAnimation<C: Curve> {
    parent: Arc<dyn Animation<f32>>,
    curve: C,
}

pub struct TweenAnimation<T, A: Animatable<T>> {
    tween: A,
    parent: Arc<dyn Animation<f32>>,
}
```

### Benefits

- Type-safe composition
- No inheritance hierarchies
- Easy to add new animation types
- Clear ownership semantics

## Builder Pattern

### Problem

`AnimationController` has many optional configuration parameters. Multiple constructors or method chains become unwieldy.

### Solution

Use a builder with type-safe validation:

```rust
let controller = AnimationController::builder(duration, scheduler)
    .bounds(0.0, 100.0)?      // Returns Result
    .reverse_duration(Duration::from_millis(500))
    .initial_value(50.0)
    .build()?;                 // Returns Result
```

### Implementation

```rust
pub struct AnimationControllerBuilder {
    duration: Duration,
    scheduler: Arc<Scheduler>,
    lower_bound: f32,
    upper_bound: f32,
    reverse_duration: Option<Duration>,
    initial_value: Option<f32>,
}

impl AnimationControllerBuilder {
    pub fn bounds(mut self, lower: f32, upper: f32) -> Result<Self, AnimationError> {
        if lower >= upper {
            return Err(AnimationError::InvalidBounds(...));
        }
        self.lower_bound = lower;
        self.upper_bound = upper;
        Ok(self)
    }
    
    pub fn build(self) -> Result<AnimationController, AnimationError> {
        // Construct with validated parameters
    }
}
```

### Why Result in Builder?

Unlike some builders that defer validation to `build()`, we validate immediately in `bounds()`:

- **Fail fast** - Errors caught at the point of misconfiguration
- **Better error messages** - Context preserved where error occurs
- **No silent failures** - Can't accidentally create invalid bounds

### Benefits

- Fluent, readable API
- Compile-time required parameters (in `new()`)
- Runtime validation with clear errors
- Immutable after construction

## Extension Trait Pattern

### Problem

Adding convenience methods to core types bloats their API and creates coupling.

### Solution

Use extension traits for fluent composition:

```rust
// Core type stays minimal
pub struct CurvedAnimation<C: Curve> { ... }

// Extension trait adds fluent methods
pub trait AnimationExt: Animation<f32> + Sized + 'static {
    fn curved<C>(self: Arc<Self>, curve: C) -> CurvedAnimation<C>
    where
        C: Curve + Clone + Send + Sync + Debug + 'static,
    {
        CurvedAnimation::new(self as Arc<dyn Animation<f32>>, curve)
    }
    
    fn reversed(self: Arc<Self>) -> ReverseAnimation {
        ReverseAnimation::new(self as Arc<dyn Animation<f32>>)
    }
}

// Blanket implementation
impl<A: Animation<f32> + 'static> AnimationExt for A {}
```

### Usage

```rust
use flui_animation::AnimationExt;

// Fluent chaining
let animation = Arc::new(controller)
    .curved(Curves::EaseInOut)
    .reversed();
```

### Rust API Guidelines

This follows [C-CONV-SPECIFIC](https://rust-lang.github.io/api-guidelines/predictability.html#c-conv-specific):

> Conversions should live on the most specific type involved.

Extension traits let us add conversions without modifying core types.

### Benefits

- Core types stay focused
- Optional import (users choose fluency vs explicitness)
- Easy to add new composition methods
- No trait object overhead

## Type-Erased Trait Objects

### Problem

Generic animations need to be stored and passed uniformly.

### Solution

Use `Arc<dyn Animation<T>>` for type erasure:

```rust
pub struct TweenAnimation<T, A: Animatable<T>> {
    tween: A,
    parent: Arc<dyn Animation<f32>>,  // Type-erased
}
```

### Why Arc?

- **Shared ownership** - Multiple animations can reference same parent
- **Thread-safe** - Works across threads
- **Cheap cloning** - Just pointer copy

### DynAnimation Trait

For storing animations in collections:

```rust
pub trait DynAnimation<T>: Animation<T> + Listenable {}

impl<T, A> DynAnimation<T> for A
where
    T: Clone + Send + Sync + 'static,
    A: Animation<T> + Listenable + ?Sized,
{}

// Usage
let animations: Vec<Arc<dyn DynAnimation<f32>>> = vec![
    Arc::new(controller1),
    Arc::new(controller2),
];
```

### Benefits

- Uniform storage and handling
- Maintains type safety for value type T
- Zero-cost when not using trait objects

## Callback Pattern

### Problem

Animations need to notify listeners when values or status changes.

### Solution

Use closures with `Send + Sync` bounds:

```rust
pub type StatusCallback = Arc<dyn Fn(AnimationStatus) + Send + Sync>;

impl AnimationController {
    pub fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        let id = ListenerId::new();
        self.status_listeners.write().push((id, callback));
        id
    }
}
```

### Why Arc<dyn Fn>?

- **Shared** - Callback can be stored and called multiple times
- **Thread-safe** - `Send + Sync` enables multi-threaded UI
- **Flexible** - Closures can capture environment

### Listener IDs

Listeners return IDs for removal:

```rust
let id = controller.add_status_listener(callback);
// ... later
controller.remove_status_listener(id);
```

### Benefits

- Idiomatic Rust (closures vs inheritance)
- Thread-safe by default
- No lifetime issues with callbacks
- Clean removal API

## Disposal Pattern

### Problem

Animation controllers hold resources (tickers) that must be cleaned up.

### Solution

Explicit `dispose()` method with atomic flag:

```rust
pub struct AnimationController {
    disposed: AtomicBool,
    ticker: RwLock<Option<Ticker>>,
    // ...
}

impl AnimationController {
    pub fn dispose(&self) {
        if self.disposed.swap(true, Ordering::SeqCst) {
            return; // Already disposed
        }
        
        // Stop ticker
        if let Some(ticker) = self.ticker.write().take() {
            ticker.stop();
        }
        
        // Clear listeners
        self.listeners.write().clear();
        self.status_listeners.write().clear();
    }
}
```

### Guard Against Use After Dispose

```rust
pub fn forward(&self) -> Result<(), AnimationError> {
    if self.disposed.load(Ordering::SeqCst) {
        return Err(AnimationError::Disposed);
    }
    // ... proceed
}
```

### Why Not Drop?

`Drop` can't return errors or take `&self`. Explicit disposal:
- Returns immediately if already disposed
- Can be called multiple times safely
- Errors returned from operations after disposal

### Benefits

- Explicit resource cleanup
- Thread-safe (atomic flag)
- Idempotent (safe to call multiple times)
- Clear error on use after dispose

## Summary

| Pattern | Problem | Solution |
|---------|---------|----------|
| Persistent Object | Hooks don't work for animations | Long-lived objects with explicit lifecycle |
| Composition | Inheritance doesn't fit Rust | Wrap animations via Arc |
| Builder | Many optional parameters | Fluent builder with validation |
| Extension Trait | API bloat | Optional fluent methods |
| Type-Erased Trait Objects | Uniform handling | `Arc<dyn Animation<T>>` |
| Callback | Change notification | `Arc<dyn Fn + Send + Sync>` |
| Disposal | Resource cleanup | Explicit dispose() with atomic guard |
