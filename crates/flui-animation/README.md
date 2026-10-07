# flui_animation

Animation system for FLUI: values over time driven by a ticker, with controllers, curves, tweens and simulations.

Standalone `rust` blocks in this document are compiled as doctests. Blocks
marked `rust,ignore` are excerpts that depend on values introduced by the
surrounding narrative rather than complete programs.

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

```rust,ignore
pub enum AnimationStatus {
    Dismissed,  // At the beginning (value = lower_bound)
    Forward,    // Playing toward end
    Reverse,    // Playing toward beginning  
    Completed,  // At the end (value = upper_bound)
}
```

Status indicates direction, not position. A `Completed` animation at value 1.0 that starts reversing becomes `Reverse` immediately, even before the value changes.

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

```rust,ignore
controller.forward()?;           // Animate to upper_bound
controller.reverse()?;           // Animate to lower_bound
controller.forward_from(0.5)?;   // Jump to 0.5, then animate forward
controller.reverse_from(0.8)?;   // Jump to 0.8, then animate backward
controller.stop();               // Stop at current value
controller.reset();              // Jump to lower_bound, status = Dismissed
```

### Repeating

```rust,ignore
controller.repeat(false)?;               // Loop: 0→1→0→1→...
controller.repeat(true)?;                // Bounce: 0→1→0→1→...
```

### Physics-Based Animation

```rust,ignore
// Fling with velocity (uses spring physics)
controller.fling(1.0)?;   // velocity toward upper_bound
controller.fling(-1.0)?;  // velocity toward lower_bound

// Custom spring
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.7);
controller.fling_with(1.0, spring)?;

// Arbitrary simulation
let sim = SpringSimulation::new(spring, 0.0, 1.0, 2.0);
controller.animate_with(sim)?;
```

### Listening

```rust,ignore
// Value changes
let id = controller.add_listener(|| println!("value changed"));
controller.remove_listener(id);

// Status changes
let id = controller.add_status_listener(|status| {
    if status == AnimationStatus::Completed {
        println!("done");
    }
});
```

### Lifecycle

```rust,ignore
controller.dispose();  // Stop animation, release ticker
// Controller is unusable after dispose
```

---

## Curves

A `Curve` maps animation progress `t` to eased progress. Used for easing.

**Contract** (the `Curve` trait docs): `transform(0.0) == 0.0` and
`transform(1.0) == 1.0` exactly; inside `[0, 1]` the output is finite and may
overshoot (`Back`, `Elastic`); `t` outside `[0, 1]` clamps to the nearest end;
`transform(NaN)` is NaN.

### Predefined Curves

```rust
use flui_animation::{Curve, Curves};

let _ = Curves::Linear;        // Identity
let _ = Curves::EaseIn;        // Slow start
let _ = Curves::EaseOut;       // Slow end
let _ = Curves::EaseInOut;     // Slow start and end
let _ = Curves::FastOutSlowIn; // Material Design standard

let _ = Curves::BounceIn;      // Bounce at start
let _ = Curves::BounceOut;     // Bounce at end
let _ = Curves::BounceInOut;   // Bounce both

let _ = Curves::ElasticIn;     // Overshoot at start
let _ = Curves::ElasticOut;    // Overshoot at end
let _ = Curves::ElasticInOut;  // Overshoot both

let _ = Curves::Decelerate;    // Fast start, gradual stop

assert_eq!(Curves::EaseInOut.transform(0.5), 0.5);
```

### Custom Curves

Parameters are validated however a curve is made: `new` panics (a compile
error in a `const`), `try_new` returns a `CurveError`, and serde decoding
rejects what `try_new` rejects.

```rust
use flui_animation::{Cubic, CurveError, Curves, ElasticOutCurve, Interval};

// Cubic bezier (CSS `cubic-bezier`); x1 and x2 must lie in [0, 1]
const EASE: Cubic = Cubic::new(0.25, 0.1, 0.25, 1.0);
assert!(matches!(
    Cubic::try_new(1.5, 0.0, 0.5, 1.0),
    Err(CurveError::OutOfRange { parameter: "x1", .. })
));

// Elastic with custom period (finite, > 0)
let elastic = ElasticOutCurve::new(0.3);

// Interval: active only in [0.2, 0.8]
let interval = Interval::new(0.2, 0.8, Curves::EaseIn);
```

### Curve Modifiers

```rust
use flui_animation::{Curve, Curves};

// The 180° rotation 1.0 - curve(1.0 - t): an ease-in becomes an ease-out.
let flipped = Curves::EaseIn.flipped();
assert!((flipped.transform(0.25) - (1.0 - Curves::EaseIn.transform(0.75))).abs() < 1e-12);
```

To run a curve backwards in time, reverse the animation that drives it
(`ReverseAnimation`), not the curve.

### Steps

`Steps` is CSS `steps(n, <jump>)`: `n` equal intervals, each holding one
value, with the jumps placed by `JumpAt`.

```rust
use flui_animation::{Curve, JumpAt, Steps};

let ticks = Steps::new(8, JumpAt::End);
assert_eq!(ticks.transform(0.124), 0.0);
assert_eq!(ticks.transform(0.125), 0.125);
```

To run a curve backwards in time, reverse the animation that drives it
(`ReverseAnimation`, `AnimationExt::reversed`), not the curve.

---

## Tweens

An `Animatable<T>` transforms `t ∈ [0, 1]` into a value of type `T`.

A `Tween<T>` is an `Animatable` with explicit `begin` and `end` values.

### Built-in Tweens

```rust,ignore
// Numeric
FloatTween::new(0.0, 100.0)
IntTween::new(0, 255)      // Rounds to nearest
StepTween::new(0, 10)      // Floors to integer

// Geometric
ColorTween::new(Color::RED, Color::BLUE)
SizeTween::new(Size::new(0.0, 0.0), Size::new(100.0, 100.0))
OffsetTween::new(Offset::ZERO, Offset::new(50.0, 50.0))
RectTween::new(rect1, rect2)
AlignmentTween::new(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT)
EdgeInsetsTween::new(EdgeInsets::ZERO, EdgeInsets::all(16.0))
BorderRadiusTween::new(BorderRadius::ZERO, BorderRadius::circular(8.0))

// Constant (always returns same value)
ConstantTween::new(42.0)
```

### Using Tweens

```rust,ignore
let tween = FloatTween::new(0.0, 100.0);
let value = tween.transform(0.5);  // 50.0

// With animation
let position = tween.transform(controller.value());
```

### Keyframes

A `Keyframes<T>` track is a value as a pure function of time. Segments are
timed by `Duration` and laid end to end; each `to` segment carries the curve
that eases *into* its value, `cubic` segments form a Catmull-Rom spline
through the keyframe times, `hold` pauses and `jump` changes the value
instantly. At a boundary the track returns the keyframe exactly (the value
after a jump), and a non-finite sample is never published.

```rust
use std::time::Duration;
use flui_animation::{Animatable, Curves, Keyframes, Linear};

let ms = Duration::from_millis;
let pulse = Keyframes::builder(0.0, ms(1000))
    .to(100.0, ms(250), Curves::EaseOut) // 0–250 ms
    .hold(ms(500))                       // 250–750 ms
    .to(0.0, ms(250), Linear)            // 750–1000 ms
    .build()
    .expect("the segments fit in 1 s");

assert_eq!(pulse.value_at(ms(500)), 100.0);
assert_eq!(pulse.transform(0.875), 50.0); // progress of a 1 s controller
assert_eq!(pulse.value_at_looped(ms(1250)), 100.0);
```

A delay is a leading `hold`; tracks that share one `total` read one
controller as a group. `Stagger` gives per-index delays (`step · |origin − i|`)
so one controller drives many elements.

### Tween Composition

```rust,ignore
// Chain: a curve, then a value tween
let curved = ChainedTween::new(CurveTween::new(Curves::EaseIn), tween);

// Reverse direction
let reversed = ReverseTween::new(tween);
```

### CurveTween

Apply a curve as an Animatable:

```rust,ignore
let curve_tween = CurveTween::new(Curves::EaseIn);
let eased = curve_tween.transform(0.5);  // EaseIn applied to 0.5
```

---

## Animation Composition

### CurvedAnimation

Apply a curve to an animation's output:

```rust,ignore
let curved = CurvedAnimation::new(controller.clone(), Curves::EaseInOut);

// Value is: curve.transform(controller.value())
let value = curved.value();
```

### TweenAnimation

Map animation output through a tween:

```rust,ignore
let tween = FloatTween::new(0.0, 300.0);
let animated = TweenAnimation::new(controller.clone(), tween);

// Value is: tween.transform(controller.value())
let pixels = animated.value();  // 0.0 to 300.0
```

### ReverseAnimation

Invert an animation:

```rust,ignore
let reversed = ReverseAnimation::new(controller.clone());

// value = 1.0 - parent.value()
// Forward becomes Reverse, Completed becomes Dismissed
```

### ProxyAnimation

Hot-swap the parent animation:

```rust,ignore
let proxy = ProxyAnimation::new(controller1.clone());

// Later, switch to different animation
proxy.set_parent(controller2.clone());
```

### CompoundAnimation

Combine two animations with an operator:

```rust,ignore
use flui_animation::AnimationOperator;

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
```

### ConstantAnimation

Animation with a fixed value (never changes):

```rust,ignore
let stopped = ConstantAnimation::new(0.5, AnimationStatus::Completed);
let complete = ConstantAnimation::completed(1.0);
let dismissed = ConstantAnimation::dismissed(0.0);

// Predefined constants
use flui_animation::{ALWAYS_COMPLETE, ALWAYS_DISMISSED};
```

### AnimationSwitch

Switch between animations when they cross:

```rust,ignore
let switch = AnimationSwitch::new(anim1.clone(), Some(anim2.clone()));

// When anim1 and anim2 values cross, switches to anim2
// Useful for "train hopping" between overlapping animations
```

---

## Driving a tween

`AnimatableExt::animate` wraps a tween and a parent animation in a
`TweenAnimation`; a curve over an animation is `CurvedAnimation::new`.

```rust,ignore
use flui_animation::AnimatableExt;

let curved = Arc::new(CurvedAnimation::new(controller.clone(), Curves::EaseOut));
let animated = FloatTween::new(0.0, 100.0).animate(curved);
```

---

## Physics Simulations

### SpringDescription

Defines spring physics parameters:

```rust,ignore
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
spring.damping_ratio();  // 0.0–∞
spring.bounce();         // Inverse of damping ratio
```

### SpringSimulation

```rust,ignore
let sim = SpringSimulation::new(
    spring,
    0.0,    // start position
    1.0,    // end position  
    0.0,    // initial velocity
);

sim.x(0.1);       // Position at t=0.1
sim.dx(0.1);      // Velocity at t=0.1
sim.is_done(0.1); // Within tolerance?
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

```rust,ignore
let sim = FrictionSimulation::new(
    0.05,   // drag coefficient (0 < drag < 1, drag ≠ 1)
    0.0,    // initial position
    100.0,  // initial velocity
);

sim.final_x();     // Resting position
sim.time_at_x(x);  // Time to reach position x
```

### GravitySimulation

Constant acceleration:

```rust,ignore
let sim = GravitySimulation::new(
    9.8,    // acceleration
    0.0,    // initial position
    10.0,   // initial velocity
    100.0,  // end position (simulation ends here)
);
```

### Tolerance

All simulations use tolerance for `is_done()`:

```rust,ignore
let tolerance = Tolerance {
    distance: 0.01,   // Position tolerance
    velocity: 0.01,   // Velocity tolerance  
    time: 0.001,      // Time tolerance
};

let sim = SpringSimulation::with_tolerance(spring, 0.0, 1.0, 0.0, tolerance);
```

---

## Error Handling

```rust,ignore
pub enum AnimationError {
    InvalidBounds,      // lower_bound >= upper_bound
    InvalidValue,       // Value outside bounds
    InvalidDuration,    // Duration is zero or negative
    AlreadyDisposed,    // Operation on disposed controller
    AlreadyAnimating,   // Conflicting animation command
    TickerError,        // UpdateScheduler/ticker failure
}
```

All fallible operations return `Result<_, AnimationError>`.

---

## Thread Safety

- `AnimationController` is `Send + Sync`
- All animations are `Send + Sync`  
- Listeners are invoked synchronously on the ticker thread
- Internal state protected by `parking_lot::RwLock`

---

## Validation

Constructors validate parameters and panic on invalid input:

| Constructor | Panics if |
|-------------|-----------|
| `SpringDescription::new` | mass ≤ 0, stiffness ≤ 0, damping < 0 |
| `SpringDescription::with_damping_ratio` | mass ≤ 0, stiffness ≤ 0, ratio < 0 |
| `FrictionSimulation::new` | drag ≤ 0, drag = 1.0 |
| `Steps::new` | count = 0, or count = 1 with `JumpAt::None` |
| `Interval::new` | begin/end not finite or outside [0,1], end < begin |
| `Cubic::new` | any argument not finite, x1 or x2 outside [0,1] |
| `ThreePointCubic::new` | midpoint not strictly inside the unit square, a control x outside its segment, a coordinate not finite |
| `Elastic{In,Out,InOut}Curve::new` | period not finite or ≤ 0 |
| `Split::with_curves` | split not finite or outside [0,1] |

---

## Additional capabilities

Each implemented from the canonical published source:

| Capability | Source | API |
|---|---|---|
| Frame-rate-independent smoothing (half-life exponential decay) | Holmér, "lerp smoothing is broken" | `smoothing::exp_decay`, `Smoothed` |
| Critically damped follower with max-speed clamp | Unity `SmoothDamp` / Game Programming Gems 4 ch. 1.10 | `smoothing::SmoothDamp` |
| Perceptually uniform color interpolation | Ottosson, Oklab (2020) | `OklabColorTween`, `Color::lerp_oklab` |
| M3 emphasized easing + full Penner catalog | Material 3 / Penner | `Curves::EaseInOutCubicEmphasized`, `ThreePointCubic`, `Split` |
| Interruptible springs with velocity-preserving retarget | analytic closed forms | `AnimatedValue`, `#[derive(TwoWayConverter)]` |

See `examples/smoothing_follow.rs` and `examples/oklab_gradient.rs`.
