# Animation Architecture

Internal architecture of `flui_animation`.

## System Overview

`flui_animation` is the framework-agnostic animation **engine**. The widget
layer that consumes it (`AnimatedFoo`, `AnimatedBuilder`, implicit animations,
`TickerProvider` ownership) is **planned, not yet implemented** — shown dashed
below.

```
┌ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┐
   widget layer (planned, not yet built)
│  AnimatedFoo, AnimatedBuilder, ImplicitAnimations        │
└ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘
                       │ will use
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
│  UpdateScheduler, Ticker, FrameBudget, Priority         │
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
    
    // Animation state. Time comes from the ticker's elapsed seconds (scaled by
    // time_dilation), not wall-clock Instants: `run_epoch_secs` marks where the
    // current run/repeat-cycle began on the ticker timeline.
    run_epoch_secs: f64,
    run_duration: Option<Duration>, // per-run override (animate_to), never clobbers `duration`
    start_value: f32,
    target_value: f32,
    
    // Physics
    simulation: Option<Box<dyn Simulation>>,
    
    // Repeat
    is_repeating: bool,
    repeat_reverse: bool,
    repeat_min: f32,
    repeat_max: f32,
    repeat_period: Option<Duration>,
    repeat_count: Option<u32>,
    
    // Ticker
    ticker: Option<Ticker>,
    
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

Each frame (via the scheduler-driven `Ticker`):

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

## Mapping decisions

### `AnimationController` owns the one future each run resolves

**Rule:** every run-starting method (`forward`, `forward_from`, `reverse`,
`reverse_from`, `animate_to`, `animate_back`, `animate_to_curved`,
`animate_back_curved`, `repeat`, `repeat_with`, `fling`, `fling_with`,
`animate_with`, `animate_back_with`) returns
`Result<TickerFuture, AnimationError>`: `Err` means the run could not start
(the controller is disposed); the `TickerFuture` is how the run ends,
`Ok(())` on a normal finish and `Err(TickerCanceled)` when it is superseded
or torn down. `AnimationControllerInner.active_run: Option<TickerCompleter>`
is the one completer this controller ever holds; every run-ending or
run-starting site funnels through `AnimationController::finish`, the single
chokepoint that owns `drop(inner)` then delivers — see
`docs/adr/ADR-0064-animation-completion-is-one-controller-resolved-future.md`
for why resolution moved here rather than staying on `Ticker` (a lock-order
fact, not a Flutter divergence).

**Per-site table** (guard held → what happens → delivered after unlock):

| Site | Publishes | Displaces |
|---|---|---|
| `forward_from`/`reverse_from`/`drive_to`/`repeat_with`/`fling_with`/`drive_simulation` (non-settling path) | nothing (installs a fresh completer) | previous `active_run`, canceled |
| `forward_from`/`reverse_from`/`drive_to`'s zero-distance path (`settle_at_target`) | the trivial run, complete | previous `active_run`, canceled |
| `tick_time_based` (non-repeating end, repeat-exhausted end) | the finishing run, complete | — |
| `tick_simulation` (`is_done`) | the finishing run, complete | — |
| `stop`/`set_value` (`stop_running`) | — | `active_run`, canceled |
| `reset` (`stop_running`) | — | `active_run`, canceled |
| `dispose` | — | `active_run`, canceled |
| last `Arc<Mutex<Inner>>` drop (no explicit `dispose()`) | — | `active_run`'s own `Drop`, canceled — reachable only for `without_ticker`(`_bounds`) controllers; `new` **and** `with_detached_ticker` both install a real `Ticker`, and once a run starts `restart_ticker` gives it a callback capturing `self.clone()` regardless of whether that ticker is scheduler-driven or detached, so both hold `inner.ticker → callback → controller clone → inner` — a cycle that never reaches zero strong references without `dispose()` |

**Publish-before-listeners.** A natural end (`tick_time_based`,
`tick_simulation`) takes `active_run` and calls
`TickerCompleter::complete()` **before** `drop(inner)` — the same guard scope
that sets `status`. Flutter's `_tick` completes its `Completer` before
`notifyListeners()`/`_checkStatusChanged()` too
(`animation_controller.dart:951`); only *delivery* (continuations, wakers) is
deferred past the unlock. This is what makes a panicking status listener
leave the run `Ok`: the unwind drops the `TickerDelivery` `finish` was mid-way
through handing off, which delivers the already-published outcome instead of
losing it.

**Status-before-cancel.** A run-starting site displaces `active_run` under
the lock, but `finish` fires the run's own (new) status listeners **before**
delivering the displaced run's cancellation — the observable order a caller
sees is "the new run started" then "the old one was canceled", matching
Flutter's own sync-status/microtask-cancel split.

**Recorded gap: zero-duration runs complete on the first tick, not at
`forward()`.** `AnimationController::new(Duration::ZERO, &scheduler)` behaves
like any other run: `forward()` installs a pending `TickerFuture` and the run
completes on the controller's first `tick_at`/`execute_frame`, because
`tick_time_based`'s `duration.is_zero() => t = 1.0` branch is itself only
reached from a tick. Flutter's `AnimationController.forward()` on a
zero-duration `Ticker` returns an *already-complete* `TickerFuture`
synchronously, before the first frame — Flutter special-cases this in
`_animateToInternal` (`animation_controller.dart:674-684`: `stop(); if
(simulationDuration == Duration.zero) { … return TickerFuture.complete(); }`).
FLUI's `settle_at_target` covers the *zero-distance* case (`forward()` when
already at the bound) exactly this way, but not the *zero-duration* case (a
real distance covered in `Duration::ZERO`). Recorded rather than silently
accepted, and tracked as issue #1171: a caller relying on Flutter's
synchronous completion for a zero-duration controller must drive one tick
first.

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

### AnimatableExt

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
