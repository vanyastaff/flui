# Animation Architecture

This document describes the internal architecture of the `flui_animation` crate.

## Overview

The animation system follows Flutter's proven three-layer architecture adapted for Rust's ownership model:

```
┌─────────────────────────────────────────────────────────┐
│                   flui_widgets                          │
│  AnimatedWidget, AnimatedBuilder, ImplicitAnimations    │
└──────────────────────┬──────────────────────────────────┘
                       │ uses
┌──────────────────────▼──────────────────────────────────┐
│                 flui_animation                          │
│  Animation<T>, AnimationController, CurvedAnimation     │
└──────────────────────┬──────────────────────────────────┘
                       │ uses
┌──────────────────────▼──────────────────────────────────┐
│             flui_types/animation                        │
│  Curve, Tween<T>, AnimationStatus (data only)          │
└──────────────────────┬──────────────────────────────────┘
                       │ uses
┌──────────────────────▼──────────────────────────────────┐
│              flui-scheduler                             │
│  Scheduler, Ticker, FrameBudget, Priority               │
└─────────────────────────────────────────────────────────┘
```

## Core Concepts

### Persistent Objects

Unlike React-style hooks that recreate state each render, FLUI animations are **persistent objects** that survive widget rebuilds:

```rust
// Created once, lives until disposed
let controller = AnimationController::new(duration, scheduler);

// Used across many widget rebuilds
controller.forward()?;
controller.reverse()?;

// Explicit cleanup required
controller.dispose();
```

This matches Flutter's `AnimationController` lifecycle and enables:
- Continuous animations across rebuilds
- Explicit control over animation timing
- Efficient memory usage via `Arc` sharing

### The Animation Trait

The core abstraction is `Animation<T>`:

```rust
pub trait Animation<T>: Listenable + Send + Sync + Debug
where
    T: Clone + Send + Sync + 'static,
{
    fn value(&self) -> T;
    fn status(&self) -> AnimationStatus;
    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId;
    fn remove_status_listener(&self, id: ListenerId);
}
```

Key design decisions:
- **Generic over T** - Supports any value type (f32, Color, Size, etc.)
- **Extends Listenable** - Integrates with FLUI's change notification system
- **Thread-safe** - `Send + Sync` bounds enable multi-threaded UI
- **Debug required** - All animations can be inspected

### Composition Over Inheritance

Instead of class hierarchies, animations compose:

```
AnimationController (0.0 → 1.0)
        │
        ▼
CurvedAnimation (applies easing)
        │
        ▼
TweenAnimation<Color> (maps to color)
```

Each layer wraps the previous via `Arc<dyn Animation<f32>>`:

```rust
let controller = Arc::new(AnimationController::new(duration, scheduler));
let curved = Arc::new(CurvedAnimation::new(controller, Curves::EaseInOut));
let color = TweenAnimation::new(ColorTween::new(RED, BLUE), curved);
```

## Module Structure

```
src/
├── animation.rs      # Animation trait, AnimationDirection
├── controller.rs     # AnimationController implementation
├── builder.rs        # AnimationControllerBuilder
├── curved.rs         # CurvedAnimation
├── tween.rs          # TweenAnimation<T>
├── reverse.rs        # ReverseAnimation
├── proxy.rs          # ProxyAnimation
├── compound.rs       # CompoundAnimation (add, multiply, min, max)
├── error.rs          # AnimationError
├── ext.rs            # AnimatableExt, AnimationExt traits
└── lib.rs            # Public API and re-exports
```

## AnimationController Internals

The controller is the heart of the animation system:

```rust
pub struct AnimationController {
    // Current state
    value: AtomicF32,
    status: RwLock<AnimationStatus>,
    direction: RwLock<AnimationDirection>,
    
    // Configuration
    duration: Duration,
    reverse_duration: Option<Duration>,
    lower_bound: f32,
    upper_bound: f32,
    
    // Timing
    ticker: RwLock<Option<Ticker>>,
    scheduler: Arc<Scheduler>,
    
    // Listeners
    listeners: RwLock<Vec<(ListenerId, Listener)>>,
    status_listeners: RwLock<Vec<(ListenerId, StatusCallback)>>,
    
    // Lifecycle
    disposed: AtomicBool,
}
```

### State Machine

```
                    forward()
    ┌─────────────────────────────────────┐
    │                                     ▼
┌───────────┐                       ┌───────────┐
│ Dismissed │                       │  Forward  │
│  (0.0)    │                       │ (running) │
└───────────┘                       └───────────┘
    ▲                                     │
    │              reaches 1.0            │
    │         ┌───────────────────────────┘
    │         ▼
    │   ┌───────────┐
    │   │ Completed │
    │   │   (1.0)   │
    │   └───────────┘
    │         │
    │         │ reverse()
    │         ▼
    │   ┌───────────┐
    └───│  Reverse  │
        │ (running) │
        └───────────┘
```

### Ticker Integration

The controller uses a `Ticker` from `flui-scheduler` for frame callbacks:

```rust
fn start_ticker(&self, direction: AnimationDirection) -> Result<(), AnimationError> {
    let ticker = self.scheduler.create_ticker(callback);
    ticker.start();
    *self.ticker.write() = Some(ticker);
    Ok(())
}
```

Each frame:
1. Ticker fires callback with delta time
2. Controller updates `value` based on duration and direction
3. Controller notifies all listeners
4. If complete, updates status and notifies status listeners

## Thread Safety Model

All types use `Arc` + `parking_lot` for thread safety:

| Type | Synchronization |
|------|-----------------|
| `value` | `AtomicF32` (lock-free) |
| `status` | `RwLock` (read-heavy) |
| `listeners` | `RwLock<Vec<...>>` |
| `ticker` | `RwLock<Option<...>>` |
| `disposed` | `AtomicBool` (lock-free) |

Why `parking_lot` over `std`:
- 2-3x faster than std::sync primitives
- No poisoning (panics don't leave locks unusable)
- Smaller memory footprint

## Error Handling

Operations return `Result<T, AnimationError>`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AnimationError {
    #[error("AnimationController has been disposed")]
    Disposed,
    
    #[error("Invalid animation bounds: {0}")]
    InvalidBounds(String),
    
    #[error("Ticker not available")]
    TickerNotAvailable,
}
```

Design decisions:
- `#[non_exhaustive]` - Can add variants without breaking changes
- `thiserror` - Automatic `Error` trait implementation
- `Clone` - Errors can be shared across threads

## Extension Traits

Extension traits provide fluent APIs without cluttering core types:

### AnimatableExt

Adds `.animate()` to any `Animatable<T>`:

```rust
pub trait AnimatableExt<T>: Animatable<T> + Clone + Send + Sync + 'static {
    fn animate(self, parent: Arc<dyn Animation<f32>>) -> TweenAnimation<T, Self>;
}

// Usage
let animation = FloatTween::new(0.0, 100.0).animate(controller);
```

### AnimationExt

Adds composition methods to `Animation<f32>`:

```rust
pub trait AnimationExt: Animation<f32> + Sized + 'static {
    fn curved<C>(self: Arc<Self>, curve: C) -> CurvedAnimation<C>;
    fn reversed(self: Arc<Self>) -> ReverseAnimation;
    fn add(self: Arc<Self>, other: Arc<dyn Animation<f32>>) -> CompoundAnimation;
    fn multiply(self: Arc<Self>, other: Arc<dyn Animation<f32>>) -> CompoundAnimation;
}

// Usage
let animation = controller.curved(Curves::EaseIn).reversed();
```

## Memory Management

### Arc-Based Sharing

All animations use `Arc` for shared ownership:

```rust
let controller = Arc::new(AnimationController::new(...));
let curved = Arc::new(CurvedAnimation::new(controller.clone(), curve));
```

Benefits:
- Cheap cloning (pointer copy)
- Automatic cleanup when last reference drops
- Thread-safe sharing

### Explicit Disposal

Controllers require explicit disposal to stop tickers:

```rust
controller.dispose();
```

After disposal:
- All operations return `Err(AnimationError::Disposed)`
- Ticker is stopped and dropped
- Listeners are cleared

## Future Considerations

### Hook Integration

Future integration with `flui-reactivity` hooks:

```rust
fn build(self, ctx: &BuildContext) -> impl IntoElement {
    let controller = use_animation_controller(ctx, Duration::from_millis(300));
    // Automatically disposed when widget unmounts
}
```

### Implicit Animations

Higher-level APIs that automatically animate property changes:

```rust
AnimatedContainer::new()
    .width(expanded ? 200.0 : 100.0)
    .duration(Duration::from_millis(300))
    .curve(Curves::EaseInOut)
```

### Staggered Animations

Coordinated animations with delays:

```rust
let stagger = StaggeredAnimation::new(Duration::from_millis(50))
    .add(fade_in)
    .add(slide_up)
    .add(scale_up);
```
