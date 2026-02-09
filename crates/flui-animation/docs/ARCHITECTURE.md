# Animation Architecture

Internal architecture of `flui_animation`.

## System Overview

```
┌─────────────────────────────────────────────────────────┐
│                   flui_widgets                          │
│  AnimatedWidget, AnimatedBuilder, ImplicitAnimations    │
└──────────────────────┬──────────────────────────────────┘
                       │ uses
┌──────────────────────▼──────────────────────────────────┐
│                 flui_animation                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │ Stateful: Animation<T>, AnimationController,       │ │
│  │           CurvedAnimation, TweenAnimation          │ │
│  ├────────────────────────────────────────────────────┤ │
│  │ Data: Curve, Tween, AnimationStatus, Simulation    │ │
│  └────────────────────────────────────────────────────┘ │
└──────────────────────┬──────────────────────────────────┘
                       │ uses
┌──────────────────────▼──────────────────────────────────┐
│              flui-scheduler                             │
│  Scheduler, Ticker, FrameBudget, Priority               │
└─────────────────────────────────────────────────────────┘
```

## Module Structure

```
src/
├── lib.rs            # Public API, re-exports
│
├── animation.rs      # Animation<T> trait, AnimationDirection
├── controller.rs     # AnimationController (main driver)
├── builder.rs        # AnimationControllerBuilder
│
├── curved.rs         # CurvedAnimation (applies curve)
├── tween.rs          # TweenAnimation<T> (maps to type T)
├── reverse.rs        # ReverseAnimation (inverts value)
├── proxy.rs          # ProxyAnimation (hot-swappable parent)
├── compound.rs       # CompoundAnimation (combine with operators)
├── constant.rs       # ConstantAnimation (fixed value)
├── switch.rs         # AnimationSwitch (crossover switching)
│
├── curve.rs          # Curve trait, Curves, implementations
├── tween_types.rs    # Animatable, Tween, all tween types
├── status.rs         # AnimationStatus, AnimationBehavior
├── simulation.rs     # Simulation trait, Spring, Friction, Gravity
│
├── ext.rs            # AnimatableExt, AnimationExt, CurveExt
└── error.rs          # AnimationError
```

## Core Abstractions

### Animation<T> Trait

The central abstraction:

```rust
pub trait Animation<T>: Listenable + Send + Sync + Debug
where
    T: Clone + Send + Sync + 'static,
{
    /// Current value
    fn value(&self) -> T;
    
    /// Current status (Forward, Reverse, Completed, Dismissed)
    fn status(&self) -> AnimationStatus;
    
    /// Listen to status changes
    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId;
    fn remove_status_listener(&self, id: ListenerId);
}
```

Design decisions:
- **Generic over T** — any value type (f32, Color, Size)
- **Extends Listenable** — integrates with change notification system
- **Send + Sync** — thread-safe by default
- **Debug required** — all animations inspectable

### Curve Trait

Maps unit interval to unit interval:

```rust
pub trait Curve {
    /// Transform t ∈ [0,1] → output ∈ [0,1]
    /// Contract: transform(0) = 0, transform(1) = 1
    fn transform(&self, t: f32) -> f32;
    
    fn flipped(self) -> FlippedCurve<Self>;
    fn reversed(self) -> ReverseCurve<Self>;
}
```

### Animatable<T> and Tween<T> Traits

```rust
/// Maps t ∈ [0,1] → value of type T
pub trait Animatable<T>: Clone + Send + Sync + Debug {
    fn transform(&self, t: f32) -> T;
}

/// Animatable with explicit begin/end
pub trait Tween<T>: Animatable<T> {
    fn begin(&self) -> &T;
    fn end(&self) -> &T;
    fn lerp(&self, t: f32) -> T;
}
```

### Simulation Trait

Physics-based value generation:

```rust
pub trait Simulation: Send + Sync {
    /// Position at time t
    fn x(&self, time: f32) -> f32;
    
    /// Velocity at time t
    fn dx(&self, time: f32) -> f32;
    
    /// Has simulation settled?
    fn is_done(&self, time: f32) -> bool;
    
    fn tolerance(&self) -> Tolerance;
}
```

## AnimationController Internals

### State

```rust
pub struct AnimationController {
    inner: Arc<Mutex<AnimationControllerInner>>,
    notifier: Arc<ChangeNotifier>,
}

struct AnimationControllerInner {
    // Current state
    value: f32,
    status: AnimationStatus,
    direction: AnimationDirection,
    
    // Configuration
    duration: Duration,
    reverse_duration: Option<Duration>,
    lower_bound: f32,
    upper_bound: f32,
    
    // Animation state
    animation_start_time: Option<Instant>,
    start_value: f32,
    target_value: f32,
    
    // Physics
    active_simulation: Option<Box<dyn Simulation>>,
    simulation_start_time: Option<Instant>,
    
    // Repeat
    is_repeating: bool,
    repeat_reverse: bool,
    
    // Ticker
    ticker: Option<ScheduledTicker>,
    
    // Listeners
    status_listeners: Vec<(ListenerId, StatusCallback)>,
    
    // Lifecycle
    disposed: bool,
}
```

### State Machine

```
                forward()
    ┌─────────────────────────────────────┐
    │                                     ▼
┌───────────┐                       ┌───────────┐
│ Dismissed │◄──────────────────────│  Forward  │
│  (0.0)    │      reaches 0.0      │ (running) │
└───────────┘                       └─────┬─────┘
    ▲                                     │
    │               reaches 1.0           │
    │         ┌───────────────────────────┘
    │         ▼
    │   ┌───────────┐
    │   │ Completed │
    │   │   (1.0)   │
    │   └─────┬─────┘
    │         │ reverse()
    │         ▼
    │   ┌───────────┐
    └───│  Reverse  │
        │ (running) │
        └───────────┘
```

### Tick Cycle

Each frame (via ScheduledTicker):

1. Lock `inner`
2. Calculate elapsed time
3. Update `value` based on duration/simulation
4. Check for completion, update `status`
5. Unlock `inner`
6. Notify value listeners (via ChangeNotifier)
7. Notify status listeners (if status changed)

```rust
fn tick(&self, delta: Duration) {
    let (new_status, should_notify_status) = {
        let mut inner = self.inner.lock();
        // Update value and status
        // ...
        (inner.status, status_changed)
    };
    // Lock released
    
    self.notifier.notify_listeners();
    
    if should_notify_status {
        self.notify_status_listeners(new_status);
    }
}
```

## Composition Model

Animations compose via `Arc<dyn Animation<f32>>`:

```
AnimationController (produces 0.0 → 1.0)
        │
        ▼ Arc<dyn Animation<f32>>
CurvedAnimation (applies easing curve)
        │
        ▼ Arc<dyn Animation<f32>>
TweenAnimation<Color> (maps to Color)
        │
        ▼ Animation<Color>
```

Each wrapper stores parent as `Arc<dyn Animation<f32>>`:

```rust
pub struct CurvedAnimation<C: Curve> {
    parent: Arc<dyn Animation<f32>>,
    curve: C,
    reverse_curve: Option<C>,
}

impl<C: Curve> Animation<f32> for CurvedAnimation<C> {
    fn value(&self) -> f32 {
        let t = self.parent.value();
        self.curve.transform(t)
    }
    
    fn status(&self) -> AnimationStatus {
        self.parent.status()  // Pass through
    }
}
```

## Thread Safety

All types are `Send + Sync`. Synchronization strategy:

| Component | Mechanism | Rationale |
|-----------|-----------|-----------|
| Controller state | `Mutex<Inner>` | Single lock, batch updates |
| Value listeners | `ChangeNotifier` | Separate from state lock |
| Status listeners | Inside `Inner` | Updated with state |
| Disposed flag | Inside `Inner` | Checked under lock |

Using `parking_lot::Mutex`:
- 2-3x faster than std
- No poisoning on panic
- Smaller memory footprint

## Error Handling

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnimationError {
    InvalidBounds,      // lower >= upper
    InvalidValue,       // value outside bounds
    InvalidDuration,    // duration <= 0
    AlreadyDisposed,    // operation on disposed controller
    AlreadyAnimating,   // conflicting animation command
    TickerError,        // scheduler/ticker failure
}
```

Design:
- `#[non_exhaustive]` — can add variants without breaking
- `Clone` — shareable across threads
- All fallible operations return `Result<_, AnimationError>`

## Memory Management

### Arc Sharing

```rust
let controller = Arc::new(AnimationController::new(...));
let curved = Arc::new(CurvedAnimation::new(controller.clone(), curve));
let tweened = TweenAnimation::new(tween, curved.clone());
```

Benefits:
- Cheap cloning (pointer copy + atomic increment)
- Automatic cleanup on last reference drop
- Thread-safe sharing

### Explicit Disposal

Controllers require explicit disposal:

```rust
controller.dispose();
```

After disposal:
- All operations return `Err(AnimationError::AlreadyDisposed)`
- Ticker stopped and dropped
- Listeners cleared

Why not just Drop?
- `Drop` can't return errors
- `Drop` takes `&mut self`, not compatible with `Arc<Self>`
- Explicit disposal can be called safely multiple times

## Extension Traits

Add fluent APIs without cluttering core types:

### AnimationExt

```rust
pub trait AnimationExt: Animation<f32> + Sized + 'static {
    fn curved<C: Curve>(self: Arc<Self>, curve: C) -> Arc<CurvedAnimation<C>>;
    fn reversed(self: Arc<Self>) -> Arc<ReverseAnimation>;
    fn add(self: Arc<Self>, other: Arc<dyn Animation<f32>>) -> Arc<CompoundAnimation>;
    // ...
}

impl<A: Animation<f32> + 'static> AnimationExt for A {}
```

### AnimatableExt (TweenAnimatableExt)

```rust
pub trait AnimatableExt<T>: Animatable<T> {
    fn animate<A: Animation<f32>>(self, parent: Arc<A>) -> TweenAnimation<T, Self>;
    fn chain<B: Animatable<T>>(self, next: B) -> ChainedTween<Self, B>;
    fn with_curve<C: Curve>(self, curve: C) -> ChainedTween<CurveTween<C>, Self>;
    fn reversed(self) -> ReverseTween<T, Self>;
}
```

### CurveExt

```rust
pub trait CurveExt: Curve + Sized {
    fn into_tween(self) -> CurveTween<Self>;
    fn then<C: Curve>(self, next: C) -> ChainedCurve<Self, C>;
}
```
