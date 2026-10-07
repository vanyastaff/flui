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
controller.fling_with(1.0, Some(spring))?;

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

### Tween Sequences

Chain tweens with weights:

```rust,ignore
let sequence = TweenSequence::new(vec![
    TweenSequenceItem::new(FloatTween::new(0.0, 100.0), 1.0),   // 0.0–0.25
    TweenSequenceItem::new(FloatTween::new(100.0, 100.0), 2.0), // 0.25–0.75 (hold)
    TweenSequenceItem::new(FloatTween::new(100.0, 0.0), 1.0),   // 0.75–1.0
]);

// Weights: 1 + 2 + 1 = 4
// First segment: t ∈ [0, 0.25]
// Second segment: t ∈ [0.25, 0.75]  
// Third segment: t ∈ [0.75, 1.0]
```

### Tween Composition

```rust,ignore
use flui_animation::AnimatableExt;

// Chain: first tween, then second
let chained = tween1.chain(tween2);

// Apply curve to tween output
let curved = tween.with_curve(Curves::EaseIn);

// Reverse direction
let reversed = tween.reversed();
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

## Extension Traits

### AnimationExt

```rust,ignore
use flui_animation::AnimationExt;

let anim: Arc<dyn Animation<f32>> = Arc::new(controller);

// Apply curve
let curved = anim.clone().curved(Curves::EaseIn);

// Reverse
let reversed = anim.clone().reversed();

// Combine with operator
let combined = anim.clone().combine(other, AnimationOperator::Add);

// Shorthand operators
let sum = anim.clone().add(other);
let diff = anim.clone().subtract(other);
let prod = anim.clone().multiply(other);
let quot = anim.clone().divide(other);
```

### AnimatableExt (for tweens)

```rust,ignore
use flui_animation::AnimatableExt;

let tween = FloatTween::new(0.0, 100.0);

// Animate with a controller
let animated = tween.animate(controller.clone());

// Chain tweens
let chained = tween.chain(other_tween);

// Apply curve
let eased = tween.with_curve(Curves::EaseOut);

// Reverse
let reversed = tween.reversed();
```

### CurveExt

```rust,ignore
use flui_animation::CurveExt;

// Convert curve to CurveTween
let tween = Curves::EaseIn.into_tween();

// Chain curves
let combined = Curves::EaseIn.then(Curves::EaseOut);
```

---

## Physics Simulations

Every simulation is an immutable value. Constructors validate their input and
return `Result<_, SimulationError>`; a simulation that was built publishes a
finite position and velocity for every `t`. Each one computes at construction
the time from which it stays within its `Tolerance`: from then on `x` is
exactly the resting position, `dx` is `0.0` and `is_done` stays `true`, however
the time is sampled.

### SpringDescription

A spring is stored as its natural frequency `ω` and damping ratio `ζ`; the
fields are private, so an undamped or non-finite spring cannot be written down.

```rust
use flui_animation::simulation::{SimulationError, SpringDescription, SpringType};
use std::time::Duration;

# fn main() -> Result<(), SimulationError> {
// Explicit parameters: mass, stiffness (k), damping (c), all finite and > 0.
let physical = SpringDescription::new(1.0, 500.0, 10.0)?;

// Perceptual parameters (SwiftUI's `Spring(duration:bounce:)`).
let perceptual = SpringDescription::with_duration_and_bounce(Duration::from_millis(500), 0.3)?;
assert_eq!(perceptual.spring_type(), SpringType::Underdamped);

// A constant spring from a damping ratio; panics on invalid input.
let critical = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);
assert_eq!(critical.spring_type(), SpringType::CriticallyDamped);

assert!(SpringDescription::new(1.0, 500.0, 0.0).is_err()); // never rests
# let _ = physical;
# Ok(())
# }
```

### SpringSimulation

```rust
use flui_animation::simulation::{Simulation, SpringDescription, SpringSimulation, Tolerance};

# fn main() -> Result<(), flui_animation::simulation::SimulationError> {
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.7);
let sim = SpringSimulation::try_new(spring, 0.0, 1.0, 0.0, Tolerance::DEFAULT)?;

assert_eq!(sim.x(0.0), 0.0);
assert!(sim.dx(0.05) > 0.0);
assert!(sim.is_done(5.0));
assert_eq!(sim.x(5.0), 1.0); // exactly the target once at rest
# Ok(())
# }
```

### FrictionSimulation

Friction displacement uses `f64::exp_m1` and inverse position-to-time uses
`f64::ln_1p`, so a drag approaching one keeps small motion instead of rounding
it to zero. It rests once the remaining glide is within the tolerance. The
consumer test `friction_preserves_small_decay_and_position_time_roundtrips`
([tests/contracts/simulation.rs](tests/contracts/simulation.rs)) checks the
constant-velocity limit, round trips and unreachable queries.

```rust
use flui_animation::simulation::{FrictionSimulation, Simulation, Tolerance};

# fn main() -> Result<(), flui_animation::simulation::SimulationError> {
// drag in (0, 1): the fraction of velocity kept per second.
let sim = FrictionSimulation::new(0.135, 0.0, 1000.0, Tolerance::DEFAULT)?;
let rest = sim.final_x();
assert!(sim.time_at_x(rest / 2.0) > 0.0);
assert_eq!(sim.time_at_x(-1.0), f64::INFINITY); // behind the start: never
# Ok(())
# }
```

### Bounded and bouncing scroll flings

`BoundedFrictionSimulation` stops at the bound it travels toward.
`BouncingScrollSimulation` lets friction carry past an edge, hands over to a
spring there with the same position and velocity, overshoots and returns to
rest exactly on the edge.

```rust
use flui_animation::simulation::{
    BouncingScrollSimulation, Simulation, SimulationBounds, SpringDescription, Tolerance,
};

# fn main() -> Result<(), flui_animation::simulation::SimulationError> {
let bounds = SimulationBounds::new(0.0, 100.0)?;
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.75);
let sim = BouncingScrollSimulation::new(spring, 0.135, 90.0, 2000.0, bounds, Tolerance::DEFAULT)?;
assert!(sim.is_done(10.0));
assert_eq!(sim.x(10.0), 100.0);
assert!(SimulationBounds::new(1.0, 0.0).is_err());
# Ok(())
# }
```

### Tolerance

```rust
use flui_animation::simulation::Tolerance;

# fn main() -> Result<(), flui_animation::simulation::SimulationError> {
// Distance and speed limits, in the simulation's units.
let precise = Tolerance::new(1e-4, 1e-4)?;
// Half a device pixel at a ratio of 2, with a velocity limit derived from
// the motion's own time scale: what scroll physics use.
let screen = Tolerance::for_device_pixel_ratio(2.0)?;
assert_eq!(screen, Tolerance::new(0.25, f64::INFINITY)?);
# let _ = precise;
# Ok(())
# }
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

Simulation constructors return `Err(SimulationError)` on invalid input
(`SpringDescription::new`, `with_duration_and_bounce`,
`with_response_and_damping`, `SpringSimulation::try_new`,
`FrictionSimulation::new`, `BoundedFrictionSimulation::new`,
`BouncingScrollSimulation::new`, `SimulationBounds::new`, `Tolerance::new`,
`Tolerance::for_device_pixel_ratio`). These constructors, meant for constants,
panic instead:

| Constructor | Panics if |
|-------------|-----------|
| `SpringDescription::with_damping_ratio` | mass, stiffness or ratio is NaN, infinite or ≤ 0 |
| `SpringSimulation::new` | start, end or velocity is not finite |
| `TweenSequenceItem::new` | weight ≤ 0, weight is infinite |
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
| Perceptually uniform color interpolation | Ottosson, Oklab (2020) | `OklabColorTween`, `Color::lerp_oklab` |
| M3 emphasized easing + full Penner catalog | Material 3 / Penner | `Curves::EaseInOutCubicEmphasized`, `ThreePointCubic`, `Split` |
| Interruptible springs with velocity-preserving retarget | analytic closed forms | `AnimatedValue`, `#[derive(Animatable)]` |

See `examples/oklab_gradient.rs`.
