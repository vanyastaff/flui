# flui_animation

Animation system for FLUI: values over time driven by a presentation clock, with controllers, curves, tweens and simulations.

Every `rust` block in this document is compiled as a doctest against the
current API. Lines starting with `#` are hidden setup (a scheduler, a
controller) that the rendered page leaves out.

## Core Concepts

### Presentation motion policy

Application `MotionPreference` is `FollowSystem` (the default), `Reduce` or
`Full`. Each presentation resolves it against the host's `SystemPreferences`.
The runtime updates its clock and publishes `MediaQuery::motion_of` before
the next frame. Application Full uses authored durations even when the host
requests reduced motion or a duration scale.

Controllers default to `AnimationBehavior::Normal`. Under Reduce, a finite
run settles at its terminal value on the next registry tick; an infinite
repeat parks without completing its future or requesting continuous frames.
Full resumes a parked repeat with a fresh time anchor. Under FollowSystem,
a positive host duration scale multiplies Normal durations.

Select `AnimationBehavior::Preserve` for physical inertia, activity indicators
and essential timers. It keeps authored timing under motion preferences;
debug playback and registry muting still apply. Changing policy commits clock
state without sampling controllers or invoking their listeners.

The [guide](docs/GUIDE.md#motion-preferences) gives an executable clock example.
The interactive `motion_lab` example at the workspace root compares property
interruption, independent deadlines, gestures and preserved timers:

```bash
cargo run --example motion_lab --features material -- --full
cargo run --example motion_lab --features material -- --reduce
```

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
# use flui_scheduler::UpdateScheduler;
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
controller.forward()?;                // Animate to upper_bound
controller.reverse()?;                // Animate to lower_bound
controller.forward_from(Some(0.5))?;  // Jump to 0.5, then animate forward
controller.reverse_from(Some(0.8))?;  // Jump to 0.8, then animate backward
controller.animate_to(0.6, None)?;    // Animate to a value over the forward duration
controller.stop()?;                   // Stop at current value
controller.reset()?;                  // Jump to lower_bound, status = Dismissed
# drop(controller);
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
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
controller.repeat(false)?; // Loop:   0→1, 0→1, ...
controller.repeat(true)?;  // Bounce: 0→1→0→1→...

// Bounded: three legs between 0.2 and 0.8, one period each (forward, reverse, forward)
controller.repeat_with(Some(0.2), Some(0.8), true, None, Some(3))?;
# drop(controller);
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
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
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
# drop(controller);
# Ok(())
# }
```

### Listening

Value listeners come from `flui_foundation::Listenable`; status listeners
from `Animation`. Both take an `Rc`'d callback and return a `ListenerId`.

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, AnimationStatus};
# use flui_scheduler::UpdateScheduler;
use flui_foundation::Listenable;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

// Value changes
let id = controller.add_listener(Rc::new(|| println!("value changed")));
controller.remove_listener(id);

// Status changes
let subscription = controller.subscribe_status(Rc::new(|status| {
    if status == AnimationStatus::Completed {
        println!("done");
    }
}));
drop(subscription);
# drop(controller);
```

### Lifecycle

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
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
use flui_animation::{Cubic, Curves, ElasticOutCurve, Interval};

// Cubic bezier (CSS-style)
let curve = Cubic::new(0.25, 0.1, 0.25, 1.0);

// Elastic with custom period
let elastic = ElasticOutCurve::new(0.3);

// Interval: active only in [0.2, 0.8]
let interval = Interval::new(0.2, 0.8, Curves::EaseIn);

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

### Curve Modifiers

```rust
use flui_animation::{Curve, Curves};

let flipped = Curves::EaseIn.flipped();   // 180° rotation: 1.0 - curve(1.0 - t)
assert!((flipped.transform(0.0) - 0.0).abs() < 1e-9);
```

---

## Tweens

An `Animatable` transforms `t ∈ [0, 1]` into its associated `Value` type.

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
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

let tween = FloatTween::new(0.0, 100.0);
assert_eq!(tween.transform(0.5), 50.0);

// With animation
let position = tween.transform(controller.value());
# drop(controller);
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

```rust
use flui_animation::{ChainedTween, CurveTween, Curves, FloatTween, ReverseTween};
let tween = FloatTween::new(0.0, 100.0);
// Chain: a curve, then a value tween
let curved = ChainedTween::new(CurveTween::new(Curves::EaseIn), tween);

// Reverse direction
let reversed = ReverseTween::new(tween);
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

Composition types take their parent as `Rc<dyn Animation<f64>>`.

### CurvedAnimation

Apply a curve to an animation's output:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, CurvedAnimation, Curves};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let curved = CurvedAnimation::new(Rc::new(controller.clone()), Curves::EaseInOut);

// Value is: curve.transform(controller.value())
let value = curved.value();
# drop(controller);
```

### TweenAnimation

Map animation output through a tween:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, FloatTween, TweenAnimation};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let tween = FloatTween::new(0.0, 300.0);
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
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let reversed = ReverseAnimation::new(Rc::new(controller.clone()));

// value = 1.0 - parent.value()
// Forward becomes Reverse, Completed becomes Dismissed
assert_eq!(reversed.value(), 1.0);
# drop(controller);
```

### ProxyAnimation

Hot-swap the parent animation:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, ProxyAnimation};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
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
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationSwitch};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
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

`AnimatableExt::animate` wraps a tween and a parent animation in a
`TweenAnimation`; a curve over an animation is `CurvedAnimation::new`.

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, CurvedAnimation, Curves, FloatTween};
# use flui_scheduler::UpdateScheduler;
use flui_animation::AnimatableExt;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let curved = Rc::new(CurvedAnimation::new(Rc::new(controller.clone()), Curves::EaseOut));
let animated = FloatTween::new(0.0, 100.0).animate(curved);
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
assert!(midpoint.0.color.r >= 254 && midpoint.0.color.b <= 1);
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

Fallible operations return `Result<_, AnimationError>`:

| Variant | When |
|---------|------|
| `Disposed` | Fallible driving operations refuse; queries remain readable and closed channels do not resume delivery |
| `InvalidBounds(String)` | `lower >= upper`, a non-finite bound or span, or a bad `repeat_with` range |
| `IdentityExhausted` | A run or sample namespace has exhausted its non-reusable identities |
| `InvalidSpring(String)` | An underdamped (oscillating) spring passed to `fling_with`; use `animate_with` for those |
| `NonFiniteTarget(String)` | A `NaN` target or `from` (always), an infinite one when the bound it would clamp to is itself infinite (on a bounded controller infinities clamp to the bound), or a non-finite fling velocity or simulation start |

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
let err = flui_animation::ValueRange::new(1.0, 0.0);
assert!(matches!(err, Err(AnimationError::InvalidBounds(_))));
```

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
| Perceptually uniform color interpolation | Ottosson, Oklab (2020) | `ColorTween`, `Color::lerp` (premultiplied alpha, ADR-0149) |
| M3 emphasized easing + full Penner catalog | Material 3 / Penner | `Curves::EaseInOutCubicEmphasized`, `ThreePointCubic`, `Split` |
| Interruptible springs with velocity-preserving retarget | analytic closed forms | `AnimatedValue`, `#[derive(TwoWayConverter)]` |

See `examples/oklab_gradient.rs`.
