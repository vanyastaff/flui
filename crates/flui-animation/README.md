# flui_animation

Animation system for FLUI: values over time driven by a presentation clock, with controllers, curves, tweens and simulations.

Every `rust` block in this document is compiled as a doctest against the
current API. Lines starting with `#` are hidden controller setup that the
rendered page leaves out.

## Core Concepts

### Optional diagnostics and profiling

Enable `diagnostics` on this crate, or `animation-diagnostics` on `flui` /
`flui-sdk`, to compile structured DEBUG events at `flui_animation::diagnostics`.
For example, `RUST_LOG=info,flui_animation::diagnostics=debug` admits these
events with FLUI's managed log subscriber. The subscriber supplies controller
span identities; run generations and resolved outcome generations correlate
start, replacement, cancellation, completion, disposal, direction, playback
and clock/policy changes. Ordinary unchanged samples emit no transition event.

The same feature emits `animation_tick`, `animation_sampling` and
`animation_listeners` CPU spans at target `flui_animation`.
`flui-devtools/profiling` enables it and attributes those spans to their owning
scheduler frame (ADR-0202). Tick durations include sampling and delivery;
they are not three additive portions of a CPU total. Standalone ticks have no
frame attribution, and these spans do not measure GPU execution.

Instrumentation is off by default. Enabled but filtered observation emits no
events or spans; active profiling carries the subscriber's timing/allocation
cost. Subscriber callouts run outside controller borrows, with independent
panic containment and explicit failure custody during span retirement.
Observation can lose data on failure, but does not own animation outcomes.

### Imports and ownership

Use `flui_animation::prelude::*` for common motion, observation and mapping.
The SDK exposes exactly the same types at `flui_sdk::animation::prelude`.
Specialized curves, physical models and frame/registry protocols use explicit
root imports; the prelude introduces no wrapper or additional owner.

```rust
use flui_animation::prelude::*;
use flui_animation::Vsync;
use std::time::Duration;

let registry = Vsync::new();
let owner = AnimationController::builder(Duration::from_millis(200))
    .build_on(Some(&registry));
let observed = std::rc::Rc::new(owner.controller().clone());
let mapped = Tween::new(0.0, 100.0).animate(observed);
assert_eq!(mapped.value(), 0.0);
```

`Tween<T>` delegates interpolation to `T: Lerp`; integers go through `IntTween`,
whose `Rounding` (nearest by default, floor, ceil or toward zero) is explicit. `Animatable::animate`
and `TweenAnimation::new` produce the same observer. Neither requires `Debug`
on the mapping or its value, or `Clone` on the mapping until the observer is
cloned. Curved, proxy and constant observations likewise impose no diagnostic
bound on their curves or output values; an owned curve is cloned only when
cloning its observer. `ConstantAnimation::with_status` is the single
explicit directional-phase constructor for fixed values.

Fallible curve constructors report a `CurveParameter` enum. Match its variant
to locate a rejected argument; diagnostic range text belongs to `Display`.
`CurveError::RescalingOutOfRange` distinguishes otherwise admitted points whose
normalization exceeds the segment solver's safe range. That includes both
overflowing ratios and finite ratios beyond the solver's admitted magnitude.

### Presentation motion policy

Application `MotionOverride` is `FollowSystem` (the default), `Reduce` or
`Full`. Each presentation resolves it against the host's `SystemPreferences`.
The runtime updates its clock and publishes `MediaQuery::motion_of` before
the next frame. Application Full uses authored durations even when the host
requests reduced motion or a duration scale.

Controllers default to `AnimationBehavior::Normal`. Under Reduce, a finite
run settles at its terminal value on the next registry tick; an infinite
repeat parks without completing its future or requesting continuous frames.
Full resumes a parked repeat with a fresh time anchor. Under FollowSystem,
a positive host duration scale multiplies Normal durations.

Select `AnimationBehavior::Preserve` for physical inertia and essential timers.
Loading indicators use Normal and visible static feedback under Reduce (ADR-0192).
Preserve keeps authored timing under motion preferences;
debug playback and registry muting still apply. Changing policy commits clock
state without sampling controllers or invoking their listeners.

The [guide](docs/GUIDE.md#motion-preferences) gives an executable clock example.

### The Animation Model

In FLUI an `Animation<T>` produces values of type `T` over time. The animation itself doesn't know about time—it's sampled by a presentation registry or manually.

```text
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   MotionClock   │────▶│   Controller    │────▶│    Animation    │
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
| `ReverseSettled` | Reverse-settled phase or a value at the lower bound; an interior stop also uses this phase |
| `Forward` | Forward directional status; a run may be active or stopped between the bounds |
| `Reverse` | Reverse directional status; a run may be active or stopped between the bounds |
| `ForwardSettled` | Forward-settled phase or a value at the upper bound; an interior stop also uses this phase |

```rust
use flui_animation::AnimationStatus;

assert!(AnimationStatus::Forward.is_directional());
assert!(AnimationStatus::ForwardSettled.is_settled());
assert_eq!(AnimationStatus::Forward.flip(), AnimationStatus::Reverse);
```

Status indicates direction, not position or whether a run is active. Use
`Animation::is_animating` to check activity: `AnimationStatus::is_directional`
only identifies the `Forward` and `Reverse` status variants. A `ForwardSettled`
animation at value 1.0 that starts reversing becomes `Reverse` immediately,
even before the value changes; `set_value` and disposal can leave a directional
status without an active run.

---

## AnimationController

The primary driver. Holds a value in `[lower_bound, upper_bound]` (default 0.0–1.0) and drives it over a duration.

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let duration = Duration::from_millis(300);
let controller = AnimationController::builder(Duration::from_millis(300)).build();

// Or with builder for full control
let controller = AnimationController::builder(duration)
    .bounds(flui_animation::ValueRange::new(0.0, 1.0)?)
    .initial_value(0.5)
    .reverse_duration(Duration::from_millis(200))
    .build();
# drop(controller);
# Ok(())
# }
```

### Driving Animations

Every run-starting method (`forward`, `reverse`, `*_from`, `animate_to`,
`repeat*`, `fling*`, `animate_with`) returns a `AnimationRunFuture` that resolves
when the run ends; dropping it does not cancel the run. `stop` and `reset`
return `Result<(), AnimationError>` and give no future.

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
controller.forward()?;                // Animate to upper_bound
controller.reverse()?;                // Animate to lower_bound
controller.forward_from(Some(0.5))?;  // Jump to 0.5, then animate forward
controller.reverse_from(Some(0.8))?;  // Jump to 0.8, then animate backward
controller.animate_to(0.6, None)?;    // Animate to a value over the forward duration
controller.stop()?;                   // Stop at current value
controller.reset()?;                  // Jump to lower_bound, status = ReverseSettled
# drop(controller);
# Ok(())
# }
```

### Finishing a transition

Use `finish()` when the application needs the current finite transition to
reach its declared endpoint immediately. The original run future succeeds.

```rust
use flui_animation::{Animation, AnimationController};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

let controller = AnimationController::builder(Duration::from_millis(300)).build();
let mut run = controller.forward().expect("live controller");
controller.tick_at(Duration::from_millis(150));
assert_eq!(controller.value(), 0.5);
controller.finish().expect("finite transition");
assert_eq!(controller.value(), 1.0);
assert!(!controller.is_animating());
let mut context = Context::from_waker(Waker::noop());
assert_eq!(Pin::new(&mut run).poll(&mut context), Poll::Ready(Ok(())));
```

A paused finite run can also finish. An idle controller stays idle. Infinite
repeats and arbitrary custom simulations return `AnimationError::NoFiniteEndpoint`
without changing the run; `stop()` instead cancels at the current value.

### Repeating

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError, Iterations, RepeatMode, RepeatSpec, ValueRange};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
controller.repeat(RepeatMode::Restart)?;   // Loop:      0→1, 0→1, ...
controller.repeat(RepeatMode::Alternate)?; // Alternate: 0→1→0→1→...

// Three legs between 0.2 and 0.8, one period each (forward, reverse, forward)
let range = ValueRange::new(0.2, 0.8).expect("finite, ordered range");
controller.repeat(
    RepeatSpec::new(RepeatMode::Alternate)
        .with_range(range)
        .with_iterations(Iterations::Finite(3)),
)?;
# drop(controller);
# Ok(())
# }
```

### Physics-Based Animation

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError, SpringDescription, SpringSimulation, Tolerance};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
// Fling with velocity (uses spring physics)
controller.fling(1.0)?;   // velocity toward upper_bound
controller.fling(-1.0)?;  // velocity toward lower_bound

// Custom spring: fling needs a non-oscillating (ratio >= 1) spring
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0).expect("valid spring");
controller.fling_with(1.0, Some(spring))?;

// Arbitrary simulation, oscillating springs included
let bouncy = SpringDescription::with_damping_ratio(1.0, 500.0, 0.7).expect("valid spring");
let sim = SpringSimulation::new(bouncy, 0.0, 1.0, 2.0, Tolerance::DEFAULT)?;
controller.animate_with(sim)?;
# drop(controller);
# Ok(())
# }
```

### Listening

Value listeners come from `flui_foundation::Listenable` and return a `ListenerId`.
Status subscriptions come from `Animation` and return a `StatusSubscription`
that removes the callback when dropped. Both take an `Rc`'d callback.

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, AnimationStatus};
use flui_foundation::Listenable;
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

// Value changes
let id = controller.add_listener(Rc::new(|| println!("value changed")));
controller.remove_listener(id);

// Status changes
let subscription = controller.subscribe_status(Rc::new(|status| {
    if status == AnimationStatus::ForwardSettled {
        println!("forward phase settled");
    }
}));
drop(subscription);
# drop(controller);
```

### Lifecycle

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# let mut owner = AnimationController::builder(Duration::from_millis(300)).build_on(None);
# let controller = owner.controller().clone();
owner.dispose(); // Cancel the run and close its callbacks
assert!(matches!(controller.forward(), Err(AnimationError::Disposed)));
```

---

## Curves

A `Curve` maps `t ∈ [0, 1]` to an output, usually in `[0, 1]` (elastic and
back curves overshoot). Used for easing.

**Contract**: `transform(0.0) == 0.0` and `transform(1.0) == 1.0`.

### Predefined curves

The `curves` module contains the easing preset constants:

```rust
use flui_animation::{Curve, curves};

let curves: [&dyn Curve; 12] = [
    &curves::LINEAR,           // Identity
    &curves::EASE_IN,          // Slow start
    &curves::EASE_OUT,         // Slow end
    &curves::EASE_IN_OUT,      // Slow start and end
    &curves::FAST_OUT_SLOW_IN, // Material Design standard
    &curves::BOUNCE_IN,        // Bounce at start
    &curves::BOUNCE_OUT,       // Bounce at end
    &curves::BOUNCE_IN_OUT,    // Bounce both
    &curves::ELASTIC_IN,       // Overshoot at start
    &curves::ELASTIC_OUT,      // Overshoot at end
    &curves::ELASTIC_IN_OUT,   // Overshoot both
    &curves::DECELERATE,       // Fast start, gradual stop
];
for curve in curves {
    assert!((curve.transform(1.0) - 1.0).abs() < 1e-9);
}
```

### Custom curves

```rust
use flui_animation::{Cubic, curves, Ease, Elastic, Interval};

// Cubic bezier (CSS-style)
let curve = Cubic::new(0.25, 0.1, 0.25, 1.0);

// Elastic with custom period
let elastic = Elastic::new(Ease::Out, 0.3);

// Interval: active only in [0.2, 0.8]
let interval = Interval::new(0.2, 0.8, curves::EASE_IN);

```

### Steps

`Steps` is CSS `steps(n, <jump>)`: `n` equal intervals, each holding one
value, with the jumps placed by `JumpAt`.

```rust
use flui_animation::{Curve, JumpAt, Steps};

let ticks = Steps::new(8, JumpAt::End);
assert_eq!(ticks.transform(0.124), 0.0);
assert_eq!(ticks.transform(0.125), 0.125);
```

---

### Authored easing

`PiecewiseLinear` places omitted inputs evenly between their neighbours and
raises decreasing inputs to the preceding maximum. Equal interior inputs make
a right-continuous jump; repeated outputs make a plateau. Endpoints, exterior
inputs and NaN follow the normalized `Curve` contract.

```rust
use flui_animation::{Curve, LinearStop, PiecewiseLinear};

let easing = PiecewiseLinear::new(&[
    LinearStop::new(0.0),
    LinearStop::new(0.5).at(0.25),
    LinearStop::new(0.5).at(0.75),
    LinearStop::new(1.0),
])?;
assert_eq!(easing.transform(0.5), 0.5);
# Ok::<(), flui_animation::PiecewiseLinearError>(())
```

`SpringCurve` samples a zero-velocity unit spring analytically. Its duration
comes from the conservative physical rest boundary; place a keyframe over that
duration to retain the spring's time scale. Interior overshoot remains visible.
Use `MotionSpec::Spring` for physical motion that inherits velocity on retarget.

```rust
use flui_animation::{Keyframes, SpringCurve, SpringDescription, Tolerance};

let easing = SpringCurve::new(
    SpringDescription::new(1.0, 100.0, 10.0)?, Tolerance::DEFAULT,
)?;
let duration = easing.duration();
let track = Keyframes::builder(0.0, duration)
    .to(100.0, duration, easing)
    .build()?;
assert_eq!(track.value_at(duration), 100.0);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Both types are immutable and compare by canonical value through `ArcCurve`.
They are application-authoring surfaces; a stock widget consumer can be added
when its visual contract calls for these curves.

### Curve Modifiers

```rust
use flui_animation::{Curve, curves};

let flipped = curves::EASE_IN.flipped();   // 180° rotation: 1.0 - curve(1.0 - t)
assert!((flipped.transform(0.0) - 0.0).abs() < 1e-9);
```

---

## Tweens

An `Animatable` transforms `t ∈ [0, 1]` into its associated `Value` type.

A `Tween<T>` is an `Animatable` with explicit `begin` and `end` values.

### Built-in Tweens

```rust
use flui_animation::{
    Tween, IntTween, Rounding,
};
use flui_foundation::geometry::{Edges, Offset, Rect, Size};
use flui_painting::Alignment;
use flui_painting::styling::{BorderRadius, BorderRadiusExt, Color};

// Numeric
let _ = Tween::new(0.0, 100.0);
let _ = IntTween::new(0, 255);  // Rounds to nearest (halves toward +inf)
let _ = IntTween::new(0, 10).with_rounding(Rounding::Floor);  // Steps

// Geometric
let _ = Tween::new(Color::RED, Color::BLUE);
let _ = Tween::new(Size::new(0.0, 0.0), Size::new(100.0, 100.0));
let _ = Tween::new(Offset::ZERO, Offset::new(50.0, 50.0));
let _ = Tween::new(Rect::new(0.0, 0.0, 10.0, 10.0), Rect::new(0.0, 0.0, 50.0, 50.0));
let _ = Tween::new(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT);
let _ = Tween::new(Edges::ZERO, Edges::all(16.0));
let _ = Tween::new(BorderRadius::ZERO, BorderRadius::circular(8.0));

```

### Using Tweens

```rust
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController};
use flui_animation::{Animatable, Tween};
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

let tween = Tween::new(0.0, 100.0);
assert_eq!(tween.transform(0.5), 50.0);

// With animation
let position = tween.transform(controller.value());
# drop(controller);
```

### Keyframes

A `Keyframes<T>` track is a value as a pure function of time. Segments are
timed by `Duration` and laid end to end; each `to` segment carries the curve
that eases *into* its value. Consecutive `cubic` segments form a Catmull-Rom
spline through the keyframe times. At a join with an eased segment, a cubic
segment takes that segment's velocity; next to `hold`, `jump` or a track
endpoint, its velocity is zero. `hold` pauses and `jump` changes the value
instantly. At a boundary the track returns the keyframe exactly (the value
after a jump), and a non-finite sample is never published.

```rust
use std::time::Duration;
use flui_animation::{Animatable, curves, Keyframes, Linear};

let ms = Duration::from_millis;
let pulse = Keyframes::builder(0.0, ms(1000))
    .to(100.0, ms(250), curves::EASE_OUT) // 0–250 ms
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

```rust
use flui_animation::{Animatable, Curve, curves, Tween};
let tween = Tween::new(0.0, 100.0);
let eased = tween.transform(curves::EASE_IN.transform(0.5));
assert!(eased < 50.0);

// Reverse direction
let reversed = Tween::new(tween.end, tween.begin);
assert_eq!(reversed.transform(0.25), 75.0);
```

---

## Animation Composition

Composition types take their parent as `Rc<dyn Animation<f64>>`.

### CurvedAnimation

Apply a curve to an animation's output:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, CurvedAnimation, curves};
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let curved = CurvedAnimation::new(Rc::new(controller.clone()), curves::EASE_IN_OUT);

// Value is: curve.transform(controller.value())
let value = curved.value();
# drop(controller);
```

### TweenAnimation

Map animation output through a tween:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, Tween, TweenAnimation};
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let tween = Tween::new(0.0, 300.0);
let animated = TweenAnimation::new(tween, Rc::new(controller.clone()));

// Value is: tween.transform(controller.value())
let pixels = animated.value(); // 0.0 to 300.0
# drop(controller);
```

### ReverseAnimation

Invert an animation:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, ReverseAnimation};
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let reversed = ReverseAnimation::new(Rc::new(controller.clone()));

// value = 1.0 - parent.value()
// Forward becomes Reverse, ForwardSettled becomes ReverseSettled
assert_eq!(reversed.value(), 1.0);
# drop(controller);
```

### ProxyAnimation

Hot-swap the parent animation:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, ProxyAnimation};
# let controller1 = AnimationController::builder(Duration::from_millis(300)).build();
# let controller2 = AnimationController::builder(Duration::from_millis(300)).build();
let proxy = ProxyAnimation::new(Rc::new(controller1.clone()));

// Later, switch to different animation
proxy.set_parent(Rc::new(controller2.clone()));
# drop(controller1);
# drop(controller2);
```

### ConstantAnimation

Animation with a fixed value (never changes):

```rust
use flui_animation::{UNIT_END_ANIMATION, UNIT_START_ANIMATION, Animation, AnimationStatus, ConstantAnimation};

let stopped = ConstantAnimation::with_status(0.5, AnimationStatus::ForwardSettled);
let end = ConstantAnimation::with_status(1.0, AnimationStatus::ForwardSettled);
let start = ConstantAnimation::with_status(0.0, AnimationStatus::ReverseSettled);

// Fixed normalized endpoints
assert_eq!(UNIT_END_ANIMATION.value(), 1.0);
assert_eq!(UNIT_START_ANIMATION.value(), 0.0);
```

### AnimationSwitch

Switch between animations when they cross:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationSwitch};
# let anim1 = AnimationController::builder(Duration::from_millis(300)).build();
# let anim2 = AnimationController::builder(Duration::from_millis(300)).build();
let switch = AnimationSwitch::new(Rc::new(anim1.clone()), Some(Rc::new(anim2.clone())));

// When anim1 and anim2 values cross, switches to anim2
// Useful for "train hopping" between overlapping animations
# switch.dispose();
# drop(anim1);
# drop(anim2);
```

---

## Driving a tween

`Animatable::animate` wraps a mapping and a parent animation in a
`TweenAnimation`; a curve over an animation is `CurvedAnimation::new`.

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, CurvedAnimation, curves, Tween};
use flui_animation::Animatable;
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let curved = Rc::new(CurvedAnimation::new(Rc::new(controller.clone()), curves::EASE_OUT));
let animated = Tween::new(0.0, 100.0).animate(curved);
# drop(controller);
```

---

## Custom value motion

A custom value can combine geometry, color and nested values into one owned
motion. Derive `TwoWayConverter` for a nonempty `Clone` struct whose fields
implement `TwoWayConverter` and `Lerp`. Field vectors are concatenated in
declaration order. Interpolation delegates to each field, so a color keeps its
premultiplied Oklab behavior rather than becoming four independent channel tweens.

```rust
use flui_animation::{Lerp, TwoWayConverter};
use flui_foundation::geometry::Offset;
use flui_painting::styling::Color;

#[derive(Clone, TwoWayConverter)]
struct Appearance {
    position: Offset<f64>,
    color: Color,
}

#[derive(Clone, TwoWayConverter)]
struct CardMotion(Appearance, f64);

let start = CardMotion(Appearance {
    position: Offset::ZERO,
    color: Color::rgb(255, 0, 0),
}, 0.0);
let end = CardMotion(Appearance {
    position: Offset::new(20.0, 40.0),
    color: Color::rgba(0, 0, 255, 0),
}, 1.0);
let midpoint = start.lerp_to(&end, 0.5);
assert_eq!(midpoint.0.position, Offset::new(10.0, 20.0));
let [red, _, blue, alpha] = midpoint.0.color.to_rgba_f32_array();
assert!(red >= 254.0 / 255.0 && blue <= 1.0 / 255.0);
assert_eq!(alpha, 0.5);
assert_eq!(midpoint.1, 0.5);
```

Pass this value to `AnimatedValue` to share registration, admission, time and
retargeting across its components. Fields need concrete vector widths; stable
Rust cannot sum generic-dependent widths into an array length. A manual
`TwoWayConverter` implementation can choose a concrete representation for a
generic value.

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

// A damping ratio uses the same fallible contract as the physical coefficients.
let critical = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0)?;
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
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.7)?;
let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0, Tolerance::DEFAULT)?;

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
let rest = sim.x(f64::INFINITY);
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
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.75)?;
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

Controller operations return `Result<_, AnimationError>`:

| Variant | When |
|---------|------|
| `Disposed` | Fallible driving operations refuse; queries remain readable and closed channels do not resume delivery |
| `NoFiniteEndpoint` | `finish()` refuses an infinite repeat or a custom simulation without a declared endpoint |
| `IdentityExhausted` | A run or sample namespace has exhausted its non-reusable identities |
| `ReentrantMotion` | User motion code repeatedly replaces the run while retargeting is prepared |
| `AdmissionAborted` | Grouped preparation was interrupted before admission |
| `InvalidExtent` | The gesture extent is not finite and strictly positive |
| `InvalidRepeatRange { .. }` | Repeat endpoints or their span are invalid; `defect` identifies the range failure |
| `UnderdampedFlingSpring { .. }` | The supplied damping ratio oscillates; use `animate_with` for that motion |
| `NonFiniteInput { .. }` | A scalar input or inherited velocity is not finite; `input` and `value` identify it |
| `UnboundedTarget { .. }` | The input requests an endpoint the unbounded controller does not have |
| `SpanOverflow { .. }` | The finite `from` and `to` have an unrepresentable difference |
| `VelocityOverflow { .. }` | Finite gesture velocity, extent and span yield an unrepresentable converted velocity |
| `NonFiniteComponent { .. }` | A converter produces a non-finite component before admission |
| `ComponentSimulation { .. }` | A vector component's motion construction fails; the original `SimulationError` is preserved |
| `Simulation(_)` | Scalar physical motion construction fails with a typed `SimulationError` |

```rust
use flui_animation::{RangeDefect, ValueRange};

assert_eq!(ValueRange::new(1.0, 0.0), Err(RangeDefect::Inverted));
assert_eq!(ValueRange::new(f64::NAN, 1.0), Err(RangeDefect::NonFinite));
assert_eq!(ValueRange::new(-f64::MAX, f64::MAX), Err(RangeDefect::SpanOverflow));
```

`ValueRange::new()` returns the narrow `RangeDefect`, so configuration code
can distinguish invalid endpoint data, ordering and span overflow without
parsing a message. Controller lifetime and admission errors remain separate.

Bounded `target` and `from` infinities still clamp to the indicated finite
bound. A refused operation preserves the installed run, its displayed value
and its original future. [ADR-0198](../../docs/adr/ADR-0198-typed-animation-admission-errors.md)
records the input, range and physical-construction error boundaries.

With optional serde, human-readable error diagnostics encode finite values as
numbers and non-finite values as the exact strings `NaN`, `Infinity` and
`-Infinity`, including nested simulation causes. Decoding rejects null and other
strings. NaN payload bits are canonicalized; binary formats retain native floats.

---

## UI ownership

Controllers, animation wrappers and listeners belong to one UI owner and share
state through `Rc`. Listener captures can contain owner-local values. Callbacks,
curves, simulations and outgoing captures run after `RefCell` borrows end.

Create a `DrivenController` with `builder(duration).build_on(Some(&vsync))`
and retain it for the widget's lifetime. Its observer clones share the kernel;
dropping the owner unregisters and cancels the run. A manual controller from
`build()` advances only through `tick_at(Duration)`.

---

## Validation

Simulation constructors return `Err(SimulationError)` on invalid input
(`SpringDescription::new`, `with_damping_ratio`, `with_duration_and_bounce`,
`with_response_and_damping`, `SpringSimulation::new`,
`FrictionSimulation::new`, `BoundedFrictionSimulation::new`,
`BouncingScrollSimulation::new`, `SimulationBounds::new`, `Tolerance::new`,
`Tolerance::for_device_pixel_ratio`). These constructors, meant for constants,
panic instead:

| Constructor | Panics if |
|-------------|-----------|
| `Steps::new` | count = 0, or count = 1 with `JumpAt::None` |
| `Interval::new` | begin/end not finite or outside [0,1], end < begin |
| `Cubic::new` | any argument not finite, x1 or x2 outside [0,1], y1 or y2 outside [-1e6, 1e6] |
| `ThreePointCubic::new` | midpoint not strictly inside the unit square, a control x outside its segment, a control y outside [-1e6, 1e6], a coordinate not finite |
| `Elastic{In,Out,InOut}Curve::new` | period not finite or outside [1e-6, 1e6] |
| `Split::with_curves` | split not finite or outside [0,1] |

---

## Additional capabilities

Each implemented from the canonical published source:

| Capability | Source | API |
|---|---|---|
| Perceptually uniform color interpolation | Ottosson, Oklab (2020) | `Tween<Color>`, `Color::lerp` (premultiplied alpha, ADR-0149) |
| Emphasized easing and polynomial-style presets | Bézier approximations; elastic and bounce formulas | `curves`, `ThreePointCubic`, `Split` |
| Interruptible springs with velocity-preserving retarget | analytic closed forms | `AnimatedValue`, `#[derive(TwoWayConverter)]` |
