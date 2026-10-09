# Animation design patterns

## Persistent owners and observer handles

A widget creates its `DrivenController` during lifecycle initialization and
retains it across rebuilds. The owner holds the Vsync registration. A clone
obtained from `controller()` shares the kernel and observes the same run;
dropping that clone does not withdraw the registration.

Dropping the owning controller withdraws its registration before disposing
the kernel. Rebinding preserves the last sampled elapsed time. Missing clocks
settle finite runs and park infinite repeats (ADR-0175).

```rust
use flui_animation::{AnimationController, Vsync};
use std::time::Duration;

let vsync = Vsync::new();
let owner = AnimationController::builder(Duration::from_millis(300))
    .build_on(Some(&vsync));
let observer = owner.controller().clone();
observer.forward().expect("live controller");
drop(owner); // unregister, then cancel
```

## Validated configuration

`ValueRange` validates finite endpoints and a finite positive span before a
builder accepts bounds. An unbounded controller is an explicit builder choice.
Building a manual controller acquires no scheduler or presentation registry.

```rust
use flui_animation::{AnimationController, ValueRange};
use std::time::Duration;

let controller = AnimationController::builder(Duration::from_millis(300))
    .bounds(ValueRange::new(0.0, 100.0).expect("finite range"))
    .initial_value(50.0)
    .build();
```

## Composition and type erasure

Wrappers store parents as `Rc<dyn Animation<T>>`. `Animation<T>` extends
`Listenable`, so erased parents expose value and status subscription APIs
without an additional combined trait. Static curve and tween types remain
generic where their concrete shape is known.

Parent queries and subscription removal run after internal borrows end.
Replacement commits the new parent before outgoing ownership retires.

## Owner-local callbacks

Value and status callbacks use `Rc<dyn Fn(...)>` and can capture `Rc`, `Cell`
or other UI owner state. They run synchronously on the owner that commits the
change, outside state borrows. Frame wakers separately provide cross-thread
wake capability without sharing the controller kernel.

Status commits and run deliveries join one FIFO. Reentrant transitions append
to the active drain. Removed subscriptions are silent, and the first failure
remains authoritative while healthy peers and retirement finish (ADR-0173,
ADR-0174). Containment cannot rescue an opaque user aggregate whose own
destructors double-panic before reaching the framework boundary.

## One outcome per run

Run-starting operations return `Result<AnimationRunFuture, AnimationError>`.
The controller publishes completion or cancellation before invoking outcome
callbacks. Dropping the future does not cancel the run. Replacement, stop,
reset and owner retirement cancel the displaced run.

## Immutable tracks

`Keyframes` and `Stagger` describe immutable tracks sampled from a controller's
progress. Several paint properties can share one repeating controller and
one registration. A track has no listener or independent scheduling state.

## Typed presentation time

A presentation owns one `MotionClock`, which produces `FrameTick` values for
its Vsync registry. Manual controllers accept `Duration`. Playback rates are
validated values; applying a new rate samples the previous segment first to
preserve local-time continuity. A paused presentation can admit a deterministic
step without starting continuous frame demand.
