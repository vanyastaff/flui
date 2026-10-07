# flui_animation

Animation system for FLUI: values over time driven by a ticker, with controllers, curves, tweens and simulations.

Every `rust` block in this document is compiled as a doctest against the
current API. Lines starting with `#` are hidden setup (a scheduler, a
controller) that the rendered page leaves out.

## Core Concepts

### The Animation Model

In FLUI an `Animation<T>` produces values of type `T` over time. The animation itself doesn't know about time—it's driven externally by a ticker.

```text
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│     Ticker      │────▶│   Controller    │────▶│    Animation    │
│  (time source)  │     │  (0.0 → 1.0)    │     │   (any value)   │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

The separation allows:
- Same animation logic for different time sources (vsync, manual, tests)
- Composition without knowledge of timing
- Pause/resume/reverse without rebuilding

### Animation Status

`AnimationStatus` has four variants:

| Variant | Meaning |
|---------|---------|
| `Dismissed` | Settled after a reverse run or a reset; the value is the lower bound only if the run finished (a `stop` mid-reverse leaves it between the bounds) |
| `Forward` | Forward directional status; a run may be active or stopped between the bounds |
| `Reverse` | Reverse directional status; a run may be active or stopped between the bounds |
| `Completed` | Settled after a forward run; the value is the upper bound only if the run finished (a `stop` mid-forward leaves it between the bounds) |

```rust
use flui_animation::AnimationStatus;

assert!(AnimationStatus::Forward.is_running());
assert!(AnimationStatus::Completed.is_stopped());
assert_eq!(AnimationStatus::Forward.flip(), AnimationStatus::Reverse);
```

Status indicates direction, not position or whether a run is active. Use
`Animation::is_animating` to check activity: `AnimationStatus::is_running`
only identifies the `Forward` and `Reverse` status variants. A `Completed`
animation at value 1.0 that starts reversing becomes `Reverse` immediately,
even before the value changes; `set_value` and disposal can leave a directional
status without an active run.

---

## AnimationController

The primary driver. Holds a value in `[lower_bound, upper_bound]` (default 0.0–1.0) and drives it over a duration.

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
# let duration = Duration::from_millis(300);
let controller = AnimationController::new(
    Duration::from_millis(300),
    &scheduler,
);

// Or with builder for full control
let controller = AnimationController::builder(duration, &scheduler)
    .bounds(0.0, 1.0)?
    .initial_value(0.5)
    .reverse_duration(Duration::from_millis(200))
    .build()?;
# controller.dispose();
# Ok(())
# }
```

### Driving Animations

Every run-starting method (`forward`, `reverse`, `*_from`, `animate_to`,
`repeat*`, `fling*`, `animate_with`) returns a `TickerFuture` that resolves
when the run ends; dropping it does not cancel the run. `stop` and `reset`
return `Result<(), AnimationError>` and give no future.

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
controller.forward()?;                // Animate to upper_bound
controller.reverse()?;                // Animate to lower_bound
controller.forward_from(Some(0.5))?;  // Jump to 0.5, then animate forward
controller.reverse_from(Some(0.8))?;  // Jump to 0.8, then animate backward
controller.animate_to(0.6, None)?;    // Animate to a value over the forward duration
controller.stop()?;                   // Stop at current value
controller.reset()?;                  // Jump to lower_bound, status = Dismissed
# controller.dispose();
# Ok(())
# }
```

### Repeating

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
controller.repeat(false)?; // Loop:   0→1, 0→1, ...
controller.repeat(true)?;  // Bounce: 0→1→0→1→...

// Bounded: three legs between 0.2 and 0.8, one period each (forward, reverse, forward)
controller.repeat_with(Some(0.2), Some(0.8), true, None, Some(3))?;
# controller.dispose();
# Ok(())
# }
```

### Physics-Based Animation

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError, SpringDescription, SpringSimulation};
# use flui_scheduler::UpdateScheduler;
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
// Fling with velocity (uses spring physics)
controller.fling(1.0)?;   // velocity toward upper_bound
controller.fling(-1.0)?;  // velocity toward lower_bound

// Custom spring: fling needs a non-oscillating (ratio >= 1) spring
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);
controller.fling_with(1.0, Some(spring))?;

// Arbitrary simulation, oscillating springs included
let bouncy = SpringDescription::with_damping_ratio(1.0, 500.0, 0.7);
let sim = SpringSimulation::new(bouncy, 0.0, 1.0, 2.0);
controller.animate_with(sim)?;
# controller.dispose();
# Ok(())
# }
```

### Listening

Value listeners come from `flui_foundation::Listenable`; status listeners
from `Animation`. Both take an `Arc`'d callback and return a `ListenerId`.

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, AnimationStatus};
# use flui_scheduler::UpdateScheduler;
use flui_foundation::Listenable;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);

// Value changes
let id = controller.add_listener(Arc::new(|| println!("value changed")));
controller.remove_listener(id);

// Status changes
let id = controller.add_status_listener(Arc::new(|status| {
    if status == AnimationStatus::Completed {
        println!("done");
    }
}));
controller.remove_status_listener(id);
# controller.dispose();
```

### Lifecycle

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
controller.dispose(); // Stop the run, release the ticker
assert!(matches!(controller.forward(), Err(AnimationError::Disposed)));
```

---

## Curves

A `Curve` maps `t ∈ [0, 1]` to an output, usually in `[0, 1]` (elastic and
back curves overshoot). Used for easing.

**Contract**: `transform(0.0) == 0.0` and `transform(1.0) == 1.0`.

### Predefined Curves

`Curves` holds the catalog as associated constants:

```rust
use flui_animation::{Curve, Curves};

let curves: [&dyn Curve; 12] = [
    &Curves::Linear,        // Identity
    &Curves::EaseIn,        // Slow start
    &Curves::EaseOut,       // Slow end
    &Curves::EaseInOut,     // Slow start and end
    &Curves::FastOutSlowIn, // Material Design standard
    &Curves::BounceIn,      // Bounce at start
    &Curves::BounceOut,     // Bounce at end
    &Curves::BounceInOut,   // Bounce both
    &Curves::ElasticIn,     // Overshoot at start
    &Curves::ElasticOut,    // Overshoot at end
    &Curves::ElasticInOut,  // Overshoot both
    &Curves::Decelerate,    // Fast start, gradual stop
];
for curve in curves {
    assert!((curve.transform(1.0) - 1.0).abs() < 1e-9);
}
```

### Custom Curves

```rust
use flui_animation::{CatmullRomCurve, Cubic, Curves, ElasticOutCurve, Interval};

// Cubic bezier (CSS-style)
let curve = Cubic::new(0.25, 0.1, 0.25, 1.0);

// Elastic with custom period
let elastic = ElasticOutCurve::new(0.3);

// Interval: active only in [0.2, 0.8]
let interval = Interval::new(0.2, 0.8, Curves::EaseIn);

// Catmull-Rom spline through points
let spline = CatmullRomCurve::with_points(vec![
    (0.0, 0.0),
    (0.3, 0.8),
    (0.7, 0.2),
    (1.0, 1.0),
]);
```

### Curve Modifiers

```rust
use flui_animation::{Curve, Curves};

let flipped = Curves::EaseIn.flipped();   // 180° rotation: 1.0 - curve(1.0 - t)
assert!((flipped.transform(0.0) - 0.0).abs() < 1e-9);
```

---

## Tweens

An `Animatable<T>` transforms `t ∈ [0, 1]` into a value of type `T`.

A `Tween<T>` is an `Animatable` with explicit `begin` and `end` values.

### Built-in Tweens

```rust
use flui_animation::{
    AlignmentTween, BorderRadiusTween, ColorTween, ConstantTween, EdgeInsetsTween,
    FloatTween, IntTween, OffsetTween, RectTween, SizeTween, StepTween,
};
use flui_foundation::geometry::{Edges, Offset, Rect, Size};
use flui_painting::Alignment;
use flui_painting::styling::{BorderRadius, BorderRadiusExt, Color};

// Numeric
let _ = FloatTween::new(0.0, 100.0);
let _ = IntTween::new(0, 255);  // Rounds to nearest
let _ = StepTween::new(0, 10);  // Floors to integer

// Geometric
let _ = ColorTween::new(Color::RED, Color::BLUE);
let _ = SizeTween::new(Size::new(0.0, 0.0), Size::new(100.0, 100.0));
let _ = OffsetTween::new(Offset::ZERO, Offset::new(50.0, 50.0));
let _ = RectTween::new(Rect::new(0.0, 0.0, 10.0, 10.0), Rect::new(0.0, 0.0, 50.0, 50.0));
let _ = AlignmentTween::new(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT);
let _ = EdgeInsetsTween::new(Edges::ZERO, Edges::all(16.0));
let _ = BorderRadiusTween::new(BorderRadius::ZERO, BorderRadius::circular(8.0));

// Constant (always returns same value)
let _ = ConstantTween::new(42.0);
```

### Using Tweens

```rust
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController};
# use flui_scheduler::UpdateScheduler;
use flui_animation::{Animatable, FloatTween};
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);

let tween = FloatTween::new(0.0, 100.0);
assert_eq!(tween.transform(0.5), 50.0);

// With animation
let position = tween.transform(controller.value());
# controller.dispose();
```

### Tween Sequences

Chain tweens with weights:

```rust
use flui_animation::{Animatable, FloatTween, TweenSequence, TweenSequenceItem};

let sequence = TweenSequence::new(vec![
    TweenSequenceItem::new(FloatTween::new(0.0, 100.0), 1.0),   // 0.0–0.25
    TweenSequenceItem::new(FloatTween::new(100.0, 100.0), 2.0), // 0.25–0.75 (hold)
    TweenSequenceItem::new(FloatTween::new(100.0, 0.0), 1.0),   // 0.75–1.0
]);

// Weights: 1 + 2 + 1 = 4
assert_eq!(sequence.total_weight(), 4.0);
assert!((sequence.transform(0.5) - 100.0).abs() < 1e-9);
```

### Tween Composition

```rust
use flui_animation::{AnimatableExt, Curves, FloatTween};

let tween = FloatTween::new(0.0, 100.0);

// Chain: the first animatable's output is the second one's `t`
let chained = FloatTween::new(0.0, 1.0).chain(tween);

// Apply a curve to `t` before the tween
let curved = tween.with_curve(Curves::EaseIn);

// Reverse direction
let reversed = tween.reversed();
```

### CurveTween

Apply a curve as an Animatable:

```rust
use flui_animation::{Animatable, CurveTween, Curves};

let curve_tween = CurveTween::new(Curves::EaseIn);
let eased = curve_tween.transform(0.5); // EaseIn applied to 0.5
assert!(eased < 0.5);
```

---

## Animation Composition

Composition types take their parent as `Arc<dyn Animation<f64>>`.

### CurvedAnimation

Apply a curve to an animation's output:

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, CurvedAnimation, Curves};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
let curved = CurvedAnimation::new(Arc::new(controller.clone()), Curves::EaseInOut);

// Value is: curve.transform(controller.value())
let value = curved.value();
# controller.dispose();
```

### TweenAnimation

Map animation output through a tween:

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, FloatTween, TweenAnimation};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
let tween = FloatTween::new(0.0, 300.0);
let animated = TweenAnimation::new(tween, Arc::new(controller.clone()));

// Value is: tween.transform(controller.value())
let pixels = animated.value(); // 0.0 to 300.0
# controller.dispose();
```

### ReverseAnimation

Invert an animation:

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, ReverseAnimation};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
let reversed = ReverseAnimation::new(Arc::new(controller.clone()));

// value = 1.0 - parent.value()
// Forward becomes Reverse, Completed becomes Dismissed
assert_eq!(reversed.value(), 1.0);
# controller.dispose();
```

### ProxyAnimation

Hot-swap the parent animation:

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{AnimationController, ProxyAnimation};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller1 = AnimationController::new(Duration::from_millis(300), &scheduler);
# let controller2 = AnimationController::new(Duration::from_millis(300), &scheduler);
let proxy = ProxyAnimation::new(Arc::new(controller1.clone()));

// Later, switch to different animation
proxy.set_parent(Arc::new(controller2.clone()));
# controller1.dispose();
# controller2.dispose();
```

### CompoundAnimation

Combine two animations with an operator:

```rust
# use std::sync::Arc;
# use flui_animation::{Animation, ConstantAnimation};
use flui_animation::{AnimationOperator, CompoundAnimation};
# let a: Arc<dyn Animation<f64>> = Arc::new(ConstantAnimation::new(0.25));
# let b: Arc<dyn Animation<f64>> = Arc::new(ConstantAnimation::new(0.75));

// Arithmetic
let sum = CompoundAnimation::new(a.clone(), b.clone(), AnimationOperator::Add);
let diff = CompoundAnimation::new(a.clone(), b.clone(), AnimationOperator::Subtract);
let prod = CompoundAnimation::new(a.clone(), b.clone(), AnimationOperator::Multiply);
let quot = CompoundAnimation::new(a.clone(), b.clone(), AnimationOperator::Divide);

// Selection
let minimum = CompoundAnimation::new(a.clone(), b.clone(), AnimationOperator::Min);
let maximum = CompoundAnimation::new(a.clone(), b.clone(), AnimationOperator::Max);

// Average
let mean = CompoundAnimation::mean(a.clone(), b.clone());
assert_eq!(sum.value(), 1.0);
assert_eq!(mean.value(), 0.5);
```

### ConstantAnimation

Animation with a fixed value (never changes):

```rust
use flui_animation::{ALWAYS_COMPLETE, ALWAYS_DISMISSED, Animation, AnimationStatus, ConstantAnimation};

let stopped = ConstantAnimation::with_status(0.5, AnimationStatus::Completed);
let complete = ConstantAnimation::completed(1.0);
let dismissed = ConstantAnimation::dismissed(0.0);

// Predefined statics
assert_eq!(ALWAYS_COMPLETE.value(), 1.0);
assert_eq!(ALWAYS_DISMISSED.value(), 0.0);
```

### AnimationSwitch

Switch between animations when they cross:

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationSwitch};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let anim1 = AnimationController::new(Duration::from_millis(300), &scheduler);
# let anim2 = AnimationController::new(Duration::from_millis(300), &scheduler);
let switch = AnimationSwitch::new(Arc::new(anim1.clone()), Some(Arc::new(anim2.clone())));

// When anim1 and anim2 values cross, switches to anim2
// Useful for "train hopping" between overlapping animations
# switch.dispose();
# anim1.dispose();
# anim2.dispose();
```

---

## Extension Traits

### AnimationExt

`AnimationExt` is implemented for every sized, `'static` `Animation<f64>`; its methods
take `self: Arc<Self>` and return the composed animation by value.

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, ConstantAnimation};
# use flui_scheduler::UpdateScheduler;
use flui_animation::{AnimationExt, AnimationOperator, Curves};
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
# let other: Arc<dyn Animation<f64>> = Arc::new(ConstantAnimation::new(0.5));

let anim = Arc::new(controller.clone());

// Apply curve
let curved = anim.clone().curved(Curves::EaseIn);

// Reverse
let reversed = anim.clone().reversed();

// Combine with operator
let combined = anim.clone().combine(other.clone(), AnimationOperator::Add);

// Shorthand operators
let sum = anim.clone().add(other.clone());
let diff = anim.clone().subtract(other.clone());
let prod = anim.clone().multiply(other.clone());
let quot = anim.clone().divide(other);
# controller.dispose();
```

### AnimatableExt (for tweens)

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
use flui_animation::{AnimatableExt, Curves, FloatTween};
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);

let tween = FloatTween::new(0.0, 100.0);

// Animate with a controller
let animated = tween.animate(Arc::new(controller.clone()));

// Chain: feed a 0..1 animatable into the tween
let chained = FloatTween::new(0.0, 1.0).chain(tween);

// Apply curve
let eased = tween.with_curve(Curves::EaseOut);

// Reverse
let reversed = tween.reversed();
# controller.dispose();
```

### CurveExt

```rust
use flui_animation::{Animatable, CurveExt, Curves, FloatTween};

// Convert curve to CurveTween
let tween = Curves::EaseIn.into_tween();

// Feed the curve's output into an animatable
let eased_pixels = Curves::EaseIn.then(FloatTween::new(0.0, 100.0));
assert!(eased_pixels.transform(0.5) < 50.0);
```

---

## Physics Simulations

### SpringDescription

Defines spring physics parameters:

```rust
use flui_animation::SpringDescription;

// Explicit parameters
let spring = SpringDescription::new(
    1.0,    // mass
    500.0,  // stiffness (k)
    10.0,   // damping (c)
);

// From damping ratio (more intuitive)
let spring = SpringDescription::with_damping_ratio(
    1.0,    // mass
    500.0,  // stiffness
    1.0,    // ratio: 1.0 = critically damped, <1 = bouncy, >1 = overdamped
);

// From animation feel
let spring = SpringDescription::with_duration_and_bounce(
    0.5,    // perceptual duration in seconds
    0.3,    // bounce: 0 = no bounce, higher = more bounce
);

// Query properties
let ratio = spring.damping_ratio(); // 0.0–∞
let bounce = spring.bounce();
```

### SpringSimulation

```rust
use flui_animation::{Simulation, SpringDescription, SpringSimulation};
# let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);

let sim = SpringSimulation::new(
    spring,
    0.0,    // start position
    1.0,    // end position
    0.0,    // initial velocity
);

let x = sim.x(0.1);         // Position at t=0.1
let dx = sim.dx(0.1);       // Velocity at t=0.1
let done = sim.is_done(0.1); // Within tolerance?
```

### FrictionSimulation

Friction displacement uses `f64::exp_m1`, and inverse position-to-time uses
`f64::ln_1p`, so finite drag approaching one retains small motion instead of
rounding it to zero. The public integration test
`friction_preserves_small_decay_and_position_time_roundtrips` in the
[consumer tests](tests/contracts/simulation.rs) checks the constant-velocity
limit, normal positive and negative flings, position/time round trips, and
unreachable/non-finite queries.

Deceleration with drag:

```rust
use flui_animation::FrictionSimulation;

let sim = FrictionSimulation::new(
    0.05,   // drag coefficient (0 < drag < 1, drag ≠ 1)
    0.0,    // initial position
    100.0,  // initial velocity
);

let rest = sim.final_x();             // Resting position
let t = sim.time_at_x(rest * 0.5);    // Time to reach half of it
```

### GravitySimulation

Constant acceleration:

```rust
use flui_animation::GravitySimulation;

let sim = GravitySimulation::new(
    9.8,    // acceleration
    0.0,    // initial position
    10.0,   // initial velocity
    100.0,  // end position (simulation ends here)
);
```

### Tolerance

All simulations use tolerance for `is_done()`:

```rust
use flui_animation::{SpringDescription, SpringSimulation, Tolerance};
# let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);

let tolerance = Tolerance::new(
    0.01,   // distance: position tolerance
    0.01,   // velocity: velocity tolerance
    0.001,  // time: time tolerance
);

let sim = SpringSimulation::with_tolerance(spring, 0.0, 1.0, 0.0, tolerance);
```

---

## Error Handling

Fallible operations return `Result<_, AnimationError>`:

| Variant | When |
|---------|------|
| `Disposed` | Any operation on a disposed controller |
| `InvalidBounds(String)` | `lower >= upper`, a non-finite bound or span, or a bad `repeat_with` range |
| `TickerNotAvailable` | Declared for a missing ticker; no current operation returns it |
| `InvalidSpring(String)` | An underdamped (oscillating) spring passed to `fling_with`; use `animate_with` for those |
| `NonFiniteTarget(String)` | A `NaN` target or `from` (always), an infinite one when the bound it would clamp to is itself infinite (on a bounded controller infinities clamp to the bound), or a non-finite fling velocity or simulation start |

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
let err = AnimationController::with_bounds(Duration::from_millis(300), &scheduler, 1.0, 0.0);
assert!(matches!(err, Err(AnimationError::InvalidBounds(_))));
```

---

## Thread Safety

- `AnimationController` is `Send + Sync`
- All animations are `Send + Sync`
- Listeners are invoked synchronously by whoever drives the tick, after the
  controller's lock is released
- Internal state is protected by `parking_lot::Mutex`

---

## Validation

Constructors validate parameters and panic on invalid input:

| Constructor | Panics if |
|-------------|-----------|
| `SpringDescription::new` | mass ≤ 0, stiffness ≤ 0, damping < 0, or any non-finite |
| `SpringDescription::with_damping_ratio` | mass ≤ 0, stiffness ≤ 0, ratio < 0, or any non-finite |
| `FrictionSimulation::new` | drag ≤ 0, drag = 1.0 |
| `TweenSequenceItem::new` | weight ≤ 0, weight is infinite |
| `Interval::new` | begin/end not finite or outside [0,1], end < begin |
| `Cubic::new` | any argument not finite, x1 or x2 outside [0,1], y1 or y2 outside [-1e6, 1e6] |
| `ThreePointCubic::new` | midpoint not strictly inside the unit square, a control x outside its segment, a control y outside [-1e6, 1e6], a coordinate not finite |
| `Elastic{In,Out,InOut}Curve::new` | period not finite or outside [1e-6, 1e6] |
| `Split::with_curves` | split not finite or outside [0,1] |
| `CatmullRomCurve::new`, `CatmullRomSpline::new` | fewer than two points |

---

## Additional capabilities

Each implemented from the canonical published source:

| Capability | Source | API |
|---|---|---|
| Frame-rate-independent smoothing (half-life exponential decay) | Holmér, "lerp smoothing is broken" | `smoothing::exp_decay`, `Smoothed` |
| Critically damped follower with max-speed clamp | Unity `SmoothDamp` / Game Programming Gems 4 ch. 1.10 | `smoothing::SmoothDamp` |
| Perceptually uniform color interpolation | Ottosson, Oklab (2020) | `ColorTween`, `Color::lerp` (premultiplied alpha, ADR-0149) |
| M3 emphasized easing + full Penner catalog | Material 3 / Penner | `Curves::EaseInOutCubicEmphasized`, `ThreePointCubic`, `Split` |
| Interruptible springs with velocity-preserving retarget | analytic closed forms | `AnimatedValue`, `#[derive(Animatable)]` |

See `examples/smoothing_follow.rs` and `examples/oklab_gradient.rs`.
