# Animation Guide

Practical guide to using `flui_animation`.

Every `rust` block is compiled as a doctest against the current API. Lines
starting with `#` are hidden setup (a controller, a
`Result`-returning `main`) that the rendered page leaves out.

## Setup

```rust
use flui_animation::{AnimationController, Animation};
use std::time::Duration;

let controller = AnimationController::builder(Duration::from_millis(300)).build();
controller.forward().expect("live controller");
controller.tick_at(Duration::from_millis(150));
assert_eq!(controller.value(), 0.5);
```

`build()` creates a manually sampled controller. In a presentation, retain the
owner returned by `build_on(Some(&vsync))`; its controller receives the
presentation's `FrameTick` through that Vsync registry.

## Motion preferences

Use `AppConfig::with_motion_override` to choose FollowSystem, Reduce or Full
for an application. Widgets read the resolved `MediaQuery::motion_of`; they
do not subscribe to another host preference producer. Under FollowSystem,
Normal motion takes the host duration scale into account. Preserve motion
keeps authored timing, including while the resolved policy is Reduce.

This standalone example drives the same registry policy used by runtime
frames. Policy changes take effect on a tick, so listeners see committed
controller state rather than a synchronous sample during configuration.

```rust
use std::time::Duration;
use flui_animation::{Animation, AnimationBehavior, AnimationController,
    MotionClock, MotionOverride, Vsync};

let vsync = Vsync::new();
let mut clock = MotionClock::new();
clock.set_override(MotionOverride::Reduce);
let transition = AnimationController::builder(Duration::from_secs(1))
    .build_on(Some(&vsync));
let timer = AnimationController::builder(Duration::from_secs(3))
    .behavior(AnimationBehavior::Preserve)
    .build_on(Some(&vsync));
let _ = transition.controller().forward().expect("live transition");
let _ = timer.controller().forward().expect("live timer");
vsync.tick_all(&clock.frame(Duration::ZERO));
assert_eq!(transition.controller().value(), 1.0);
assert_eq!(timer.controller().value(), 0.0);
vsync.tick_all(&clock.frame(Duration::from_secs(1)));
assert!((timer.controller().value() - 1.0 / 3.0).abs() < 1e-9);
```

A finite repeat settles at its last leg's endpoint. An infinite Normal
repeat parks at its first leg's start and keeps its run future pending;
Full resumes it from a new zero-time anchor. A saturated selected timeline
also settles finite work and parks infinite work; that timeline cannot
resume until the owner is rebound to a registry with time available.

In headless tests, `HeadlessHost::set_motion_override` and `LaidOut::set_motion_override`
use the runtime's application override path; pump a frame after changing it.

## AnimationController

### Creating

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# fn main() -> Result<(), Box<dyn std::error::Error>> {
// Simple
let controller = AnimationController::builder(Duration::from_millis(300)).build();

// With custom bounds
let controller = AnimationController::builder(Duration::from_millis(300))
    .bounds(flui_animation::ValueRange::new(0.0, 100.0)?)
    .build();

// With builder (full control)
let controller = AnimationController::builder(
    Duration::from_millis(300),
)
.bounds(flui_animation::ValueRange::new(0.0, 100.0)?)
.initial_value(50.0)
.reverse_duration(Duration::from_millis(500))
.build();
# drop(controller);
# Ok(())
# }
```

### Driving

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
// Forward (toward upper_bound)
controller.forward()?;

// Reverse (toward lower_bound)
controller.reverse()?;

// From specific value
controller.forward_from(Some(0.5))?;
controller.reverse_from(Some(0.8))?;

// Stop at current position
controller.stop()?;

// Jump to lower_bound, status = ReverseSettled
controller.reset()?;

// Set value directly (no animation)
controller.set_value(0.5);
# drop(controller);
# Ok(())
# }
```

### Finishing a transition

An application can finish a finite transition immediately, keeping the original
run future successful. This also works while the run is paused.

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

`finish()` leaves an idle controller idle. Infinite repeats and arbitrary custom
simulations refuse with `AnimationError::NoFiniteEndpoint` and keep their original
run. Use `stop()` to cancel at the current value instead.

### Repeating

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError, RepeatMode};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
// Loop: 0→1, 0→1, 0→1, ...
controller.repeat(RepeatMode::Restart)?;

// Alternate: 0→1→0→1→0, ...
controller.repeat(RepeatMode::Alternate)?;

// Stop repeating
controller.stop()?;
# drop(controller);
# Ok(())
# }
```

### Physics

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
use flui_animation::{SpringDescription, SpringSimulation};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

// Fling with velocity
controller.fling(1.0)?;   // toward upper_bound
controller.fling(-1.0)?;  // toward lower_bound

// Custom spring (fling refuses an oscillating spring)
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0).expect("valid spring");
controller.fling_with(1.0, Some(spring))?;

// Arbitrary simulation
let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0, flui_animation::Tolerance::DEFAULT)?;
controller.animate_with(sim)?;
# drop(controller);
# Ok(())
# }
```

### Reading State

```rust
# use std::time::Duration;
# use flui_animation::AnimationController;
use flui_animation::Animation;
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

let value = controller.value();   // Current value
let status = controller.status(); // AnimationStatus

controller.is_animating(); // Owns a run, including a paused run
controller.status().is_forward_settled(); // Forward phase settled; value may be interior
controller.status().is_reverse_settled(); // Reverse phase settled; value may be interior
# drop(controller);
```

### Listening

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::AnimationController;
use flui_animation::{Animation, AnimationStatus};
use flui_foundation::Listenable;
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

// Value changes (the callback owns its own handle to the controller)
let observed = controller.clone();
let id = controller.add_listener(Rc::new(move || {
    println!("value: {}", observed.value());
}));
controller.remove_listener(id);

// Status changes
let subscription = controller.subscribe_status(Rc::new(|status| {
    match status {
        AnimationStatus::ForwardSettled => println!("forward phase settled"),
        AnimationStatus::ReverseSettled => println!("reverse phase settled"),
        _ => {}
    }
}));
drop(subscription);
# drop(controller);
```

### Cleanup

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# let mut owner = AnimationController::builder(Duration::from_millis(300)).build_on(None);
# let controller = owner.controller().clone();
owner.dispose();
// Driving operations now return Err(AnimationError::Disposed)
assert!(matches!(controller.forward(), Err(AnimationError::Disposed)));
```

---

## Curves

### Using predefined curves

```rust
use flui_animation::{Curve, curves};
# let t = 0.25;

let value = curves::EASE_IN.transform(0.5);
let value = curves::BOUNCE_OUT.transform(t);
let value = curves::ELASTIC_OUT.transform(t);
```

### Available curves

| Category | Curves |
|----------|--------|
| Linear | `LINEAR` |
| CSS easing | `EASE`, `EASE_IN`, `EASE_OUT`, `EASE_IN_OUT` |
| Material | `FAST_OUT_SLOW_IN`, `LINEAR_OUT_SLOW_IN` |
| Sine approximation | `EASE_IN_SINE`, `EASE_OUT_SINE`, `EASE_IN_OUT_SINE` |
| Exponential approximation | `EASE_IN_EXPO`, `EASE_OUT_EXPO`, `EASE_IN_OUT_EXPO` |
| Circular approximation | `EASE_IN_CIRC`, `EASE_OUT_CIRC`, `EASE_IN_OUT_CIRC` |
| Back | `EASE_IN_BACK`, `EASE_OUT_BACK`, `EASE_IN_OUT_BACK` |
| Elastic | `ELASTIC_IN`, `ELASTIC_OUT`, `ELASTIC_IN_OUT` |
| Bounce | `BOUNCE_IN`, `BOUNCE_OUT`, `BOUNCE_IN_OUT` |
| Other | `DECELERATE` |

### Authored piecewise and spring easing

```rust
use flui_animation::{ArcCurve, Curve, LinearStop, PiecewiseLinear, SpringCurve,
    SpringDescription, Tolerance};

let piecewise = PiecewiseLinear::new(&[
    LinearStop::new(0.0), LinearStop::new(0.25), LinearStop::new(1.0),
])?;
assert_eq!(piecewise.transform(0.5), 0.25);
let spring = SpringCurve::new(
    SpringDescription::new(1.0, 100.0, 20.0)?, Tolerance::DEFAULT,
)?;
assert!(!spring.duration().is_zero());
let shared = ArcCurve::new(spring);
assert_eq!(shared.transform(1.0), 1.0);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`LinearStop::at` supplies normalized input progress. Missing first/last inputs
become 0/1; missing interior inputs are spaced between their explicit neighbours,
after decreasing explicit inputs have been raised. The first and last outputs
must be 0 and 1. Interior outputs can overshoot, and interior equal inputs jump
to the last output at that input. At a regular knot `slope` selects the outgoing
segment; at a jump it returns zero. Unrepresentable rates saturate finitely.
Optional serde stores canonical stops, including their assigned inputs.

This borrows the stop-placement rules of
[CSS Easing Level 2](https://drafts.csswg.org/css-easing-2/#linear-easing-function)
while retaining FLUI's exact endpoints, NaN preservation and exterior clamp.
It does not promise CSS parsing, source-text serialization or extrapolation.

`SpringCurve` maps progress to physical time without accumulating frame state.
Its tolerance is in unit-output distance and output units per physical second;
distance must be less than 1. `duration()` rounds the conservative rest upward
to nanoseconds. A too-slow spring whose rest exceeds `Duration` is refused.
No sample table, frame clock or controller is retained. Use `MotionSpec::Spring`
for a physical retarget that carries the interrupted velocity into the solver,
including descriptions authored with duration and bounce. A `SpringCurve` inside
`MotionSpec::Curve` follows that variant's Hermite seam correction and assigned
deadline; stretching its keyframe duration changes its playback time scale.

### Custom curves

```rust
use flui_animation::{Cubic, curves, Ease, Elastic, Interval};

// Cubic bezier (CSS-style control points)
let curve = Cubic::new(0.25, 0.1, 0.25, 1.0);

// Elastic with custom period
let elastic = Elastic::new(Ease::Out, 0.3);

// Active only in [0.2, 0.8]
let interval = Interval::new(0.2, 0.8, curves::EASE_IN);

```

### Steps

`Steps` is CSS `steps(n, <jump>)` (CSS Easing 1 §2.3.1); `JumpAt` places the
jumps. The ends keep the curve contract: `0 → 0`, `1 → 1`.

```rust
use flui_animation::{Curve, JumpAt, Steps};

assert_eq!(Steps::new(4, JumpAt::End).transform(0.6), 0.5);
assert_eq!(Steps::new(4, JumpAt::Start).transform(0.6), 0.75);
```

A spline through points at given times is a `Keyframes` track of `cubic`
segments (see [Keyframes](#keyframes)).

### Modifiers

```rust
use flui_animation::{Cubic, Curve};
# let curve = Cubic::new(0.42, 0.0, 1.0, 1.0);

let flipped = curve.flipped();   // 180° rotation: 1.0 - curve(1.0 - t)
```

---

## Tweens

### Basic Usage

```rust
use flui_animation::{Animatable, Tween};

let tween = Tween::new(0.0, 100.0);
let value = tween.transform(0.5); // 50.0
assert_eq!(value, 50.0);
```

### Available Tweens

```rust
use flui_animation::{
    Tween, IntTween, Rounding,
};
use flui_foundation::geometry::{Edges, Offset, Rect, Size};
use flui_painting::Alignment;
use flui_painting::styling::{BorderRadius, BorderRadiusExt, Color};
# let rect1 = Rect::new(0.0, 0.0, 10.0, 10.0);
# let rect2 = Rect::new(0.0, 0.0, 50.0, 50.0);

// Numeric
let _ = Tween::new(0.0, 100.0);
let _ = IntTween::new(0, 255); // nearest, halves toward +inf
let _ = IntTween::new(0, 10).with_rounding(Rounding::Floor); // steps

// Color
let _ = Tween::new(Color::RED, Color::BLUE);

// Geometry
let _ = Tween::new(Size::ZERO, Size::new(100.0, 100.0));
let _ = Tween::new(Offset::ZERO, Offset::new(50.0, 50.0));
let _ = Tween::new(rect1, rect2);

// Layout
let _ = Tween::new(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT);
let _ = Tween::new(Edges::ZERO, Edges::all(16.0));
let _ = Tween::new(BorderRadius::ZERO, BorderRadius::circular(8.0));

```

### Keyframes

A `Keyframes<T>` track (`T: Lerp + TwoWayConverter`) is a value as a pure
function of elapsed time. Segments are timed by `Duration` and placed end to
end from zero:

| Segment | Moves | Velocity at a join with `cubic` |
|---|---|---|
| `to(value, over, curve)` | eases into `value`; the curve belongs to this segment | the curve's slope at that end |
| `cubic(value, over)` | cubic spline through the keyframe *times* | the neighbouring chord between consecutive cubic segments; an eased neighbour's velocity; zero beside hold/jump or a track endpoint |
| `hold(over)` | keeps the value | zero |
| `jump(value)` | changes the value instantly | zero |

At a boundary the track returns the keyframe value exactly; at a jump, the
value after it. A curve that returns NaN or infinity, or interpolation that
overflows, publishes the segment's start value. `build` reports a zero
total, a segment past the total, a `Duration` overflow or a non-finite
keyframe as a `KeyframesError`.

```rust
use std::time::Duration;
use flui_animation::{Animatable, curves, Keyframes};

let ms = Duration::from_millis;
// Keys 0, 1, 0 at 0, 1, 2 s on a spline: half way up at 0.5 s.
let arc = Keyframes::builder(0.0, ms(2000))
    .cubic(1.0, ms(1000))
    .cubic(0.0, ms(1000))
    .build()
    .expect("fits");
assert!((arc.value_at(ms(500)) - 0.5).abs() < 1e-12);
assert_eq!(arc.value_at(ms(1000)), 1.0);

// One controller's progress reads every track of a group at one time.
let fade = Keyframes::builder(1.0, ms(2000))
    .hold(ms(1000)) // a delay is a leading hold
    .to(0.0, ms(1000), curves::EASE_IN)
    .build()
    .expect("fits");
assert_eq!(fade.transform(0.5), 1.0);
```

Repeat with a repeating controller (or `value_at_looped`). `Stagger` gives
element `i` of `n` the delay `step · |origin − i|` (`First`, `Last`,
`Center`, `Index`), so one controller drives every element. For a repeating
track, sample the phase `(elapsed − delay) mod total`. Compute this in `u128`
nanoseconds, reducing elapsed and delay modulo the period before subtraction;
`(phase + period − delay) mod period` then stays nonnegative and fits `u128`.
Direct `Duration` addition or subtraction can overflow when elapsed is near
`Duration::MAX` or a delay exceeds the period.

### Chaining and Composition

```rust
use flui_animation::{Animatable, Curve, curves, Tween};
let tween = Tween::new(0.0, 100.0);
// A curve, then a value tween
let eased = tween.transform(curves::EASE_IN.transform(0.5));
assert!(eased < 50.0);

// Reverse
let reversed = Tween::new(tween.end, tween.begin);
assert_eq!(reversed.transform(0.25), 75.0);
```

---

## Composition

Every composition type takes its parent as `Rc<dyn Animation<f64>>`;
`AnimationController` is a cheap handle, so `Rc::new(controller.clone())`
shares the same controller.

### CurvedAnimation

Apply curve to animation output:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, curves};
use flui_animation::CurvedAnimation;
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

let curved = CurvedAnimation::new(
    Rc::new(controller.clone()),
    curves::EASE_IN_OUT,
);

# drop(controller);
```

### TweenAnimation

Map 0–1 to any type:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, Tween};
use flui_animation::TweenAnimation;
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

let animated = TweenAnimation::new(
    Tween::new(0.0, 300.0),
    Rc::new(controller.clone()),
);

let pixels = animated.value(); // 0.0 to 300.0
# drop(controller);
```

### ReverseAnimation

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController};
use flui_animation::ReverseAnimation;
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

let reversed = ReverseAnimation::new(Rc::new(controller.clone()));
// value = 1.0 - parent.value()
// Forward ↔ Reverse, ForwardSettled ↔ ReverseSettled

# drop(controller);
```

### ProxyAnimation

Hot-swap parent:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::AnimationController;
use flui_animation::ProxyAnimation;
# let controller1 = AnimationController::builder(Duration::from_millis(300)).build();
# let controller2 = AnimationController::builder(Duration::from_millis(300)).build();

let proxy = ProxyAnimation::new(Rc::new(controller1.clone()));
// Later...
proxy.set_parent(Rc::new(controller2.clone()));
# drop(controller1);
# drop(controller2);
```

### ConstantAnimation

Fixed value:

```rust
use flui_animation::{UNIT_END_ANIMATION, UNIT_START_ANIMATION, Animation, AnimationStatus, ConstantAnimation};

let stopped = ConstantAnimation::with_status(0.5, AnimationStatus::ForwardSettled);
let end = ConstantAnimation::with_status(1.0, AnimationStatus::ForwardSettled);

// Fixed normalized endpoints
assert_eq!(UNIT_END_ANIMATION.value(), 1.0);
assert_eq!(UNIT_START_ANIMATION.value(), 0.0);
```

### AnimationSwitch

Switch at crossover:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::AnimationController;
use flui_animation::AnimationSwitch;
# let anim1 = AnimationController::builder(Duration::from_millis(300)).build();
# let anim2 = AnimationController::builder(Duration::from_millis(300)).build();

let switch = AnimationSwitch::new(Rc::new(anim1.clone()), Some(Rc::new(anim2.clone())));
// When values cross, switches from anim1 to anim2
# switch.dispose();
# drop(anim1);
# drop(anim2);
```

---

## Physics Simulations

Simulations are immutable values with validating constructors
(`Result<_, SimulationError>`). Each computes at construction the time from
which it stays within its `Tolerance`; from then on `x` is exactly the resting
position and `is_done` stays `true`.

### SpringDescription

```rust
use flui_animation::simulation::{SimulationError, SpringDescription};
use std::time::Duration;

# fn main() -> Result<(), SimulationError> {
let physical = SpringDescription::new(1.0, 500.0, 10.0)?; // mass, stiffness, damping
let perceptual = SpringDescription::with_duration_and_bounce(Duration::from_millis(500), 0.3)?;
let response = SpringDescription::with_response_and_damping(Duration::from_millis(300), 0.8)?;
// A damping ratio uses the same fallible contract as the physical coefficients.
let critical = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0)?;
# let _ = (physical, perceptual, response, critical);
# Ok(())
# }
```

### SpringSimulation

```rust
use flui_animation::simulation::{Simulation, SpringDescription, SpringSimulation, Tolerance};

# fn main() -> Result<(), flui_animation::simulation::SimulationError> {
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.7)?;
let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0, Tolerance::DEFAULT)?;
assert!(sim.dx(0.1).is_finite()); // velocity at t = 0.1 s
assert!(sim.is_done(5.0));
assert_eq!(sim.x(5.0), 1.0);
# Ok(())
# }
```

### FrictionSimulation

```rust
use flui_animation::simulation::{FrictionSimulation, Simulation, Tolerance};

# fn main() -> Result<(), flui_animation::simulation::SimulationError> {
// drag in (0, 1), position, velocity
let sim = FrictionSimulation::new(0.05, 0.0, 100.0, Tolerance::DEFAULT)?;
let resting = sim.x(f64::INFINITY);
assert!(sim.time_at_x(resting / 2.0) > 0.0);
# Ok(())
# }
```

Scroll physics use `BoundedFrictionSimulation` (stops at a bound) and
`BouncingScrollSimulation` (overscrolls an edge and springs back), with
`Tolerance::for_device_pixel_ratio` so a fling rests within half a device
pixel.

---

## Error Handling

```rust
# use std::time::Duration;
use flui_animation::{AnimationController, AnimationError};
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

match controller.forward() {
    Ok(_future) => { /* started; the future resolves when the run ends */ }
    Err(AnimationError::Disposed) => { /* disposed */ }
    Err(AnimationError::NonFiniteInput { input, value }) => { /* refused scalar input */ }
    Err(AnimationError::NonFiniteComponent { input, component, value }) => { /* refused vector input */ }
    Err(e) => { /* other error */ }
}

// Propagation
fn animate(d: Duration) -> Result<AnimationController, Box<dyn std::error::Error>> {
    let controller = AnimationController::builder(d)
        .bounds(flui_animation::ValueRange::new(0.0, 100.0)?)
        .build();
    controller.forward()?;
    // The caller samples this manually driven controller with tick_at(Duration).
    Ok(controller)
}
# let running = animate(Duration::from_millis(300)).unwrap();
# drop(running);
# drop(controller);
```

---

## Best Practices

### Own the UI Lifetime

```rust
# use std::time::Duration;
# use flui_animation::AnimationController;
# let duration = Duration::from_millis(300);
let owner = AnimationController::builder(duration).build_on(None);
let controller = owner.controller();
// Observe or operate the controller during this owner's lifetime.
drop(owner); // Withdraw registration and cancel the run.
```

### Share One Controller

`AnimationController` is a handle over shared state: `clone()` shares the
controller, it does not copy it.

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, CurvedAnimation, curves};
let controller = AnimationController::builder(Duration::from_millis(300)).build();
let curved1 = CurvedAnimation::new(Rc::new(controller.clone()), curves::EASE_IN);
let curved2 = CurvedAnimation::new(Rc::new(controller.clone()), curves::EASE_OUT);
# drop(controller);
```

### Reuse Controllers

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
// Don't create new controller each time
controller.reset()?;
controller.forward()?;
# drop(controller);
# Ok(())
# }
```

### Use Status Listeners (Not Polling)

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, AnimationStatus};
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
// Bad: check every frame
if controller.status() == AnimationStatus::ForwardSettled { /* ... */ }

// Good: react to changes
let _subscription = controller.subscribe_status(Rc::new(|status| {
    if status == AnimationStatus::ForwardSettled { /* ... */ }
}));
# drop(controller);
```
