# Animation Guide

How to use `flui_animation`. For the invariants behind it see
[ARCHITECTURE.md](ARCHITECTURE.md); for what costs what, [PERFORMANCE.md](PERFORMANCE.md).

Every `rust` block is compiled as a doctest against the current API. Lines
starting with `#` are hidden setup (a controller, a `Result`-returning
`main`) that the rendered page leaves out.

## Concepts

An `Animation<T>` produces values of type `T`. It does not know about time: a controller is
sampled by a presentation's registry (`Vsync`) or, in a test or a standalone program, by
`tick_at(Duration)`.

```text
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   MotionClock   │────▶│   Controller    │────▶│    Animation    │
│  (time source)  │     │  (0.0 → 1.0)    │     │   (any value)   │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

The separation gives the same animation logic for every time source, composition without
knowledge of timing, and pause, resume and reverse without rebuilding.

Every public item has one path, the crate root; `curves` holds the easing presets. The facade
exposes the crate as `flui::animation` and the SDK as `flui_sdk::animation`.

## Setup

```rust
use flui_animation::{AnimationController, Animation};
use std::time::Duration;

let controller = AnimationController::builder(Duration::from_millis(300)).build();
controller.forward().expect("live controller");
controller.tick_at(Duration::from_millis(150));
assert_eq!(controller.value(), 0.5);
```

`build()` creates a manually sampled controller. In a presentation, retain the owner returned
by `build_on(Some(&vsync))`; its controller receives the presentation's `FrameTick` through that
registry. Observer clones of the controller share its state but never its registration:

```rust
use flui_animation::{Animatable, Animation, AnimationController, Tween, Vsync};
use std::time::Duration;

let registry = Vsync::new();
let owner = AnimationController::builder(Duration::from_millis(200))
    .build_on(Some(&registry));
let observed = std::rc::Rc::new(owner.controller().clone());
let mapped = Tween::new(0.0, 100.0).animate(observed);
assert_eq!(mapped.value(), 0.0);
```

`Tween<T>` delegates interpolation to `T: Lerp`; `Animatable::animate` and
`TweenAnimation::new` produce the same observer. Neither requires `Debug` on the mapping or its
value, or `Clone` on the mapping until the observer is cloned; curved, proxy and constant
observations likewise impose no diagnostic bound. `ConstantAnimation::with_status` is the one
constructor for fixed values with an explicit directional phase.

### Ownership

Controllers, animation wrappers and listeners belong to one UI owner and share state through
`Rc`. Callbacks, curves, simulations and outgoing captures run after internal borrows end.

Create the owner with `builder(duration).build_on(Some(&vsync))` and retain it for the widget's
lifetime. Its observer clones share the controller; dropping the owner unregisters and cancels
the run, dropping an observer does not.

```rust
# use std::time::Duration;
# use flui_animation::AnimationController;
# let duration = Duration::from_millis(300);
let owner = AnimationController::builder(duration).build_on(None);
let controller = owner.controller();
// Observe or operate the controller during this owner's lifetime.
drop(owner); // Withdraw registration and cancel the run.
```

`AnimationController` is a handle over shared state: `clone()` shares the controller, it does
not copy it.

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, CurvedAnimation, curves};
let controller = AnimationController::builder(Duration::from_millis(300)).build();
let curved1 = CurvedAnimation::new(Rc::new(controller.clone()), curves::EASE_IN);
let curved2 = CurvedAnimation::new(Rc::new(controller.clone()), curves::EASE_OUT);
# drop(controller);
```

## Status

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
`Animation::is_animating` to check activity: `AnimationStatus::is_directional` only identifies
the `Forward` and `Reverse` variants. A `ForwardSettled` animation at value 1.0 that starts
reversing becomes `Reverse` immediately, even before the value changes; `set_value` and disposal
can leave a directional status without an active run.

## Motion preferences

Use `AppConfig::motion_override` to choose `MotionOverride::FollowSystem` (the default),
`Reduce` or `Full` for an application. Each presentation resolves it against the host's
`SystemPreferences`; widgets read the resolved `MediaQuery::motion_of` and do not subscribe to
another host preference producer. Application `Full` uses authored durations even when the host
requests reduced motion or a duration scale.

Controllers default to `AnimationBehavior::Normal`. Under `Reduce` a finite run settles at its
terminal value on the next registry tick, and an infinite repeat parks without completing its
future or requesting continuous frames; `Full` resumes a parked repeat with a fresh time anchor.
Under `FollowSystem` a positive host duration scale multiplies Normal durations. Retain owners
while a Normal repeat is parked: rebinding or returning to `Full` resumes it.

Choose `AnimationBehavior::Preserve` for physical inertia and elapsed-time obligations such as
a displayed timer. It keeps authored timing under motion preferences, while debug playback and
registry muting still apply. An ordinary decorative transition stays Normal, including on an
unbounded controller, and a loading indicator uses Normal and shows visible static feedback under
Reduce (ADR-0192). Changing the policy commits clock state without sampling controllers or
invoking their listeners.

This standalone example drives the same registry policy the runtime uses for frames. Policy
changes take effect on a tick, so listeners see committed controller state rather than a
synchronous sample during configuration.

```rust
use std::time::Duration;
use flui_animation::{Animation, AnimationBehavior, AnimationController,
    MotionClock, MotionOverride};
use flui_animation::__runtime::VsyncDriver;

// The driver a presentation owns: it ticks the registry its tree registers on.
let driver = VsyncDriver::new();
let vsync = driver.vsync();
let mut clock = MotionClock::new();
clock.set_override(MotionOverride::Reduce);
let transition = AnimationController::builder(Duration::from_secs(1))
    .build_on(Some(vsync));
let timer = AnimationController::builder(Duration::from_secs(3))
    .behavior(AnimationBehavior::Preserve)
    .build_on(Some(vsync));
let _ = transition.controller().forward().expect("live transition");
let _ = timer.controller().forward().expect("live timer");
driver.tick_all(&clock.frame(Duration::ZERO));
assert_eq!(transition.controller().value(), 1.0);
assert_eq!(timer.controller().value(), 0.0);
driver.tick_all(&clock.frame(Duration::from_secs(1)));
assert!((timer.controller().value() - 1.0 / 3.0).abs() < 1e-9);
```

A finite repeat settles at its last leg's endpoint. An infinite Normal repeat parks at its first
leg's start and keeps its run future pending. A saturated selected timeline also settles finite
work and parks infinite work; that timeline cannot resume until the owner is rebound to a
registry with time available.

In headless tests, `HeadlessHost::set_motion_override` and `LaidOut::set_motion_override` use
the runtime's application override path; pump a frame after changing it.

## AnimationController

The primary driver. It holds a value in `[lower_bound, upper_bound]` (default 0.0 to 1.0) and
drives it over a duration.

### Creating

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# fn main() -> Result<(), Box<dyn std::error::Error>> {
// Simple
let controller = AnimationController::builder(Duration::from_millis(300)).build();

// With checked bounds, an initial value and a shorter reverse run
let controller = AnimationController::builder(Duration::from_millis(300))
    .bounds(flui_animation::ValueRange::try_new(0.0, 100.0)?)
    .initial_value(50.0)
    .reverse_duration(Duration::from_millis(500))
    .build();
# drop(controller);
# Ok(())
# }
```

`ValueRange` validates finite endpoints and a finite positive span before a builder accepts
bounds. An unbounded controller is an explicit builder choice (`unbounded()`). Building a manual
controller acquires no scheduler or presentation registry.

### Driving

Every run-starting method (`forward`, `reverse`, `*_from`, `animate_to`, `animate_back`,
`repeat`, `fling*`, `animate_with`) returns an `AnimationRunFuture` that resolves when the run
ends; dropping it does not cancel the run. `stop` and `reset` return
`Result<(), AnimationError>` and give no future.

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
controller.forward()?;                // Animate to upper_bound
controller.reverse()?;                // Animate to lower_bound
controller.forward_from(0.5)?;        // Jump to 0.5, then animate forward
controller.reverse_from(0.8)?;        // Jump to 0.8, then animate backward
controller.animate_to(0.6, None)?;    // Animate to a value over the forward duration
controller.stop()?;                   // Stop at current value
controller.reset()?;                  // Jump to lower_bound, status = ReverseSettled
controller.set_value(0.5)?;           // Set the value directly, no animation
# drop(controller);
# Ok(())
# }
```

### Finishing a transition

Use `finish()` when the application needs the current finite transition to reach its declared
endpoint immediately. The original run future succeeds, and a paused finite run can finish too.

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

An idle controller stays idle. Infinite repeats and arbitrary custom simulations return
`AnimationError::NoFiniteEndpoint` without changing the run; `stop()` instead cancels at the
current value.

### Repeating

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError, Iterations, RepeatMode, RepeatSpec, ValueRange};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
controller.repeat(RepeatMode::Restart)?;   // Loop:      0→1, 0→1, ...
controller.repeat(RepeatMode::Alternate)?; // Alternate: 0→1→0→1→...

// Three legs between 0.2 and 0.8, one period each (forward, reverse, forward)
let range = ValueRange::try_new(0.2, 0.8).expect("finite, ordered range");
controller.repeat(
    RepeatSpec::new(RepeatMode::Alternate)
        .range(range)
        .iterations(Iterations::Finite(3)),
)?;

// Stop repeating
controller.stop()?;
# drop(controller);
# Ok(())
# }
```

### Physics

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError, SpringDescription, SpringSimulation, Tolerance};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
// Fling with velocity (uses spring physics)
controller.fling(1.0)?;   // toward upper_bound
controller.fling(-1.0)?;  // toward lower_bound

// Custom spring: fling needs a non-oscillating (ratio >= 1) spring
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0).expect("valid spring");
controller.fling_with(1.0, spring)?;

// Arbitrary simulation, oscillating springs included
let bouncy = SpringDescription::with_damping_ratio(1.0, 500.0, 0.7).expect("valid spring");
let sim = SpringSimulation::try_new(bouncy, 0.0, 1.0, 2.0, Tolerance::DEFAULT)?;
controller.animate_with(sim)?;
# drop(controller);
# Ok(())
# }
```

### Reading state

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

Value listeners come from `flui_foundation::Listenable`; status listeners come from
`Animation::listen_status`. Both take an `Rc`'d callback and return the channel's owned
`Subscription`, which withdraws the listener when dropped. React to a transition with a status
listener instead of reading the status on every frame.

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::AnimationController;
use flui_animation::{Animation, AnimationStatus};
use flui_foundation::Listenable;
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

// Value changes (the callback owns its own handle to the controller)
let observed = controller.clone();
let value_listening = controller.listen(Rc::new(move || {
    println!("value: {}", observed.value());
}));
drop(value_listening);

// Status changes
let subscription = controller.listen_status(Rc::new(|status| {
    match status {
        AnimationStatus::ForwardSettled => println!("forward phase settled"),
        AnimationStatus::ReverseSettled => println!("reverse phase settled"),
        _ => {}
    }
}));
drop(subscription);
# drop(controller);
```

### Cleanup and reuse

Dropping the owner ends its lifetime; `dispose` ends it explicitly while observer clones remain.

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# let mut owner = AnimationController::builder(Duration::from_millis(300)).build_on(None);
# let controller = owner.controller().clone();
owner.dispose(); // Cancel the run and close its callbacks
assert!(matches!(controller.forward(), Err(AnimationError::Disposed)));
```

Reuse a controller instead of creating one per animation:

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
controller.reset()?;
controller.forward()?;
# drop(controller);
# Ok(())
# }
```

## Curves

A `Curve` maps `t ∈ [0, 1]` to an output, usually in `[0, 1]` (elastic and back curves
overshoot). **Contract**: `transform(0.0) == 0.0` and `transform(1.0) == 1.0`.

### Presets

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

```rust
use flui_animation::{Curve, curves};

let value = curves::EASE_IN.transform(0.5);
let all: [&dyn Curve; 4] = [
    &curves::LINEAR,
    &curves::BOUNCE_OUT,
    &curves::ELASTIC_OUT,
    &curves::DECELERATE,
];
for curve in all {
    assert!((curve.transform(1.0) - 1.0).abs() < 1e-9);
}
# let _ = value;
```

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

`Steps` is CSS `steps(n, <jump>)`: `n` equal intervals, each holding one value, with the jumps
placed by `JumpAt`. The ends keep the curve contract.

```rust
use flui_animation::{Curve, JumpAt, Steps};

assert_eq!(Steps::new(4, JumpAt::End).transform(0.6), 0.5);
assert_eq!(Steps::new(4, JumpAt::Start).transform(0.6), 0.75);
```

`flipped()` rotates a curve by 180 degrees, `1.0 - curve(1.0 - t)`:

```rust
use flui_animation::{Curve, curves};

let flipped = curves::EASE_IN.flipped();
assert!((flipped.transform(0.0) - 0.0).abs() < 1e-9);
```

Fallible curve constructors report a `CurveParameter`: match its variant to locate a rejected
argument; the range text belongs to `Display`. `CurveError::RescalingOutOfRange` marks points
whose normalization exceeds the segment solver's safe range, whether the ratio overflows or is
finite but beyond the admitted magnitude. The constructors meant for constants panic instead;
their conditions are in [Validation](#validation).

### Authored easing

`PiecewiseLinear` places omitted inputs evenly between their neighbours and raises decreasing
inputs to the preceding maximum. Equal interior inputs make a right-continuous jump, repeated
outputs make a plateau, and the first and last outputs must be 0 and 1. Interior outputs can
overshoot. At a regular knot `slope` selects the outgoing segment; at a jump it returns zero.
Endpoints, exterior inputs and NaN follow the normalized `Curve` contract. The stop placement
borrows [CSS Easing Level 2](https://drafts.csswg.org/css-easing-2/#linear-easing-function);
it does not promise CSS parsing, source-text serialization or extrapolation.

```rust
use flui_animation::{Curve, LinearStop, PiecewiseLinear};

let easing = PiecewiseLinear::try_new(&[
    LinearStop::new(0.0),
    LinearStop::new(0.5).at(0.25),
    LinearStop::new(0.5).at(0.75),
    LinearStop::new(1.0),
])?;
assert_eq!(easing.transform(0.5), 0.5);
# Ok::<(), flui_animation::PiecewiseLinearError>(())
```

`SpringCurve` samples a zero-velocity unit spring analytically and maps progress to physical
time without accumulating frame state. Its tolerance is in unit-output distance and output
units per physical second, and distance must be less than 1. `duration()` rounds the
conservative rest upward to nanoseconds; a spring too slow to rest within `Duration` is refused.
Place a keyframe over that duration to keep the spring's time scale; interior overshoot remains
visible. Use `MotionSpec::Spring` for physical motion that inherits velocity on retarget; a
`SpringCurve` inside `MotionSpec::Curve` follows that variant's Hermite seam correction and
assigned deadline.

```rust
use flui_animation::{Curve, Keyframes, SharedCurve, SpringCurve, SpringDescription, Tolerance};

let easing = SpringCurve::try_new(
    SpringDescription::try_new(1.0, 100.0, 10.0)?, Tolerance::DEFAULT,
)?;
let duration = easing.duration();
let shared = SharedCurve::new(easing.clone());
assert_eq!(shared.transform(1.0), 1.0);
let track = Keyframes::builder(0.0, duration)
    .to(100.0, duration, easing)
    .build()?;
assert_eq!(track.value_at(duration), 100.0);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Both types are immutable and compare by canonical value through `SharedCurve`.

## Tweens

An `Animatable` transforms `t ∈ [0, 1]` into its associated `Value`; a `Tween<T>` is an
`Animatable` with explicit `begin` and `end`. `IntTween` rounds with an explicit `Rounding`
(nearest by default, halves toward positive infinity; floor, ceil or toward zero). Color
interpolates in Oklab with premultiplied alpha (ADR-0149).

```rust
use flui_animation::{Animatable, IntTween, Rounding, Tween};
use flui_foundation::geometry::{Edges, Offset, Rect, Size};
use flui_painting::Alignment;
use flui_painting::styling::{BorderRadius, BorderRadiusExt, Color};

let tween = Tween::new(0.0, 100.0);
assert_eq!(tween.transform(0.5), 50.0);

// Numeric
let _ = IntTween::new(0, 255);
let _ = IntTween::new(0, 10).rounding(Rounding::Floor); // steps

// Color and geometry
let _ = Tween::new(Color::RED, Color::BLUE);
let _ = Tween::new(Size::ZERO, Size::new(100.0, 100.0));
let _ = Tween::new(Offset::ZERO, Offset::new(50.0, 50.0));
let _ = Tween::new(Rect::new(0.0, 0.0, 10.0, 10.0), Rect::new(0.0, 0.0, 50.0, 50.0));

// Layout
let _ = Tween::new(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT);
let _ = Tween::new(Edges::ZERO, Edges::all(16.0));
let _ = Tween::new(BorderRadius::ZERO, BorderRadius::circular(8.0));
```

Apply a curve before a tween, or reverse one by swapping its ends:

```rust
use flui_animation::{Animatable, Curve, curves, Tween};
let tween = Tween::new(0.0, 100.0);
let eased = tween.transform(curves::EASE_IN.transform(0.5));
assert!(eased < 50.0);

let reversed = Tween::new(tween.end, tween.begin);
assert_eq!(reversed.transform(0.25), 75.0);
```

### Keyframes

A `Keyframes<T>` track (`T: Lerp + MotionValue`) is a value as a pure function of elapsed time.
Segments are timed by `Duration` and placed end to end from zero:

| Segment | Moves | Velocity at a join with `cubic` |
|---|---|---|
| `to(value, over, curve)` | eases into `value`; the curve belongs to this segment | the curve's slope at that end |
| `cubic(value, over)` | cubic spline (Catmull-Rom) through the keyframe *times* | the neighbouring chord between consecutive cubic segments; an eased neighbour's velocity; zero beside hold/jump or a track endpoint |
| `hold(over)` | keeps the value | zero |
| `jump(value)` | changes the value instantly | zero |

At a boundary the track returns the keyframe value exactly; at a jump, the value after it. A
curve that returns NaN or infinity, or interpolation that overflows, publishes the segment's
start value. `build` reports a zero total, a segment past the total, a `Duration` overflow or a
non-finite keyframe as a `KeyframesError`.

```rust
use std::time::Duration;
use flui_animation::{Animatable, curves, Keyframes, Linear};

let ms = Duration::from_millis;
let pulse = Keyframes::builder(0.0, ms(1000))
    .to(100.0, ms(250), curves::EASE_OUT) // 0–250 ms
    .hold(ms(500))                        // 250–750 ms
    .to(0.0, ms(250), Linear)             // 750–1000 ms
    .build()
    .expect("the segments fit in 1 s");

assert_eq!(pulse.value_at(ms(500)), 100.0);
assert_eq!(pulse.transform(0.875), 50.0); // progress of a 1 s controller
assert_eq!(pulse.value_at_looped(ms(1250)), 100.0);

// Keys 0, 1, 0 at 0, 1, 2 s on a spline: half way up at 0.5 s.
let arc = Keyframes::builder(0.0, ms(2000))
    .cubic(1.0, ms(1000))
    .cubic(0.0, ms(1000))
    .build()
    .expect("fits");
assert!((arc.value_at(ms(500)) - 0.5).abs() < 1e-12);
assert_eq!(arc.value_at(ms(1000)), 1.0);
```

A delay is a leading `hold`. Tracks that share one `total` read one controller as a group, so
several paint properties can share one repeating controller and one registration. `Stagger` gives
element `i` of `n` the delay `step · |origin − i|` (`StaggerOrigin::First`, `Last`, `Center`,
`Index`), so one controller drives every element. For a repeating track, sample the phase
`(elapsed − delay) mod total`: compute it in `u128` nanoseconds, reducing elapsed and delay
modulo the period before subtracting, because direct `Duration` arithmetic can overflow when
elapsed is near `Duration::MAX` or a delay exceeds the period.

### Custom value motion

A custom value can combine geometry, color and nested values into one owned motion. Derive
`MotionValue` for a nonempty `Clone` struct whose fields implement `MotionValue`; the derive
writes the field vectors concatenated in declaration order. Interpolation is the separate
`#[derive(Lerp)]` of `flui_foundation::geometry`, which delegates to each field, so a color
keeps its premultiplied Oklab behavior rather than becoming four independent channel tweens.

```rust
use flui_animation::MotionValue;
use flui_foundation::geometry::{Lerp, Offset};
use flui_painting::styling::Color;

#[derive(Clone, Lerp, MotionValue)]
struct Appearance {
    position: Offset<f64>,
    color: Color,
}

#[derive(Clone, Lerp, MotionValue)]
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

Pass such a value to `AnimatedValue` to share registration, admission, time and retargeting
across its components, with a `MotionSpec` that is a curve or a velocity-preserving spring. Fields
need concrete vector widths: stable Rust cannot sum generic-dependent widths into an array
length, so a generic value needs a manual `MotionValue` implementation with a concrete
representation.

## Composition

Every composition type takes its parent as `Rc<dyn Animation<f64>>`. A controller is a cheap
handle, so `Rc::new(controller.clone())` shares the same controller.

```text
AnimationController (produces 0.0 → 1.0)
        │ Rc<dyn Animation<f64>>
        ▼
CurvedAnimation (applies easing curve)
        │ Rc<dyn Animation<f64>>
        ▼
TweenAnimation<Tween> (maps to Color)
        │
        ▼ Animation<Color>
```

`CurvedAnimation` applies a curve to its parent's output, and `Animatable::animate` wraps a
mapping and a parent in a `TweenAnimation`:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, CurvedAnimation, curves, Tween};
use flui_animation::Animatable;
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let curved = Rc::new(CurvedAnimation::new(Rc::new(controller.clone()), curves::EASE_OUT));
let animated = Tween::new(0.0, 100.0).animate(curved); // tween.transform(curve(controller.value()))
assert_eq!(animated.value(), 0.0);
# drop(controller);
```

`TweenAnimation::new(tween, parent)` is the same observer:

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

`ReverseAnimation` inverts an animation (`value = 1.0 - parent.value()`; `Forward` becomes
`Reverse`, `ForwardSettled` becomes `ReverseSettled`):

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, ReverseAnimation};
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let reversed = ReverseAnimation::new(Rc::new(controller.clone()));
assert_eq!(reversed.value(), 1.0);
# drop(controller);
```

`ProxyAnimation` hot-swaps its parent:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, ProxyAnimation};
# let controller1 = AnimationController::builder(Duration::from_millis(300)).build();
# let controller2 = AnimationController::builder(Duration::from_millis(300)).build();
let proxy = ProxyAnimation::new(Rc::new(controller1.clone()));
// Later, switch to a different animation
proxy.set_parent(Rc::new(controller2.clone()));
# drop(controller1);
# drop(controller2);
```

`ConstantAnimation` never changes:

```rust
use flui_animation::{UNIT_END_ANIMATION, UNIT_START_ANIMATION, Animation, AnimationStatus, ConstantAnimation};

let stopped = ConstantAnimation::with_status(0.5, AnimationStatus::ForwardSettled);
assert_eq!(stopped.value(), 0.5);

// Fixed normalized endpoints
assert_eq!(UNIT_END_ANIMATION.value(), 1.0);
assert_eq!(UNIT_START_ANIMATION.value(), 0.0);
```

`AnimationSwitch` switches from one animation to another when their values cross, for "train
hopping" between overlapping animations:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationSwitch};
# let anim1 = AnimationController::builder(Duration::from_millis(300)).build();
# let anim2 = AnimationController::builder(Duration::from_millis(300)).build();
let switch = AnimationSwitch::new(Rc::new(anim1.clone()), Some(Rc::new(anim2.clone())));
// When the values cross, the switch follows anim2 instead of anim1
# switch.dispose();
# drop(anim1);
# drop(anim2);
```

## Physics simulations

Every simulation is an immutable value. Constructors validate their input and return
`Result<_, SimulationError>`; a simulation that was built publishes a finite position and
velocity for every `t`. Each computes at construction the time from which it stays within its
`Tolerance`: from then on `x` is exactly the resting position, `dx` is `0.0` and `is_done` stays
`true`, however the time is sampled.

A spring is stored as its natural frequency `ω` and damping ratio `ζ`; the fields are private,
so an undamped or non-finite spring cannot be written down.

```rust
use flui_animation::{SimulationError, SpringDescription, SpringType};
use std::time::Duration;

# fn main() -> Result<(), SimulationError> {
// Explicit parameters: mass, stiffness (k), damping (c), all finite and > 0.
let physical = SpringDescription::try_new(1.0, 500.0, 10.0)?;

// Perceptual parameters (SwiftUI's `Spring(duration:bounce:)`).
let perceptual = SpringDescription::with_duration_and_bounce(Duration::from_millis(500), 0.3)?;
assert_eq!(perceptual.spring_type(), SpringType::Underdamped);

// Response and damping.
let response = SpringDescription::with_response_and_damping(Duration::from_millis(300), 0.8)?;

// A damping ratio uses the same fallible contract as the physical coefficients.
let critical = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0)?;
assert_eq!(critical.spring_type(), SpringType::CriticallyDamped);

assert!(SpringDescription::try_new(1.0, 500.0, 0.0).is_err()); // never rests
# let _ = (physical, response);
# Ok(())
# }
```

```rust
use flui_animation::{Simulation, SpringDescription, SpringSimulation, Tolerance};

# fn main() -> Result<(), flui_animation::SimulationError> {
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.7)?;
let sim = SpringSimulation::try_new(spring, 0.0, 1.0, 0.0, Tolerance::DEFAULT)?;

assert_eq!(sim.x(0.0), 0.0);
assert!(sim.dx(0.05) > 0.0);
assert!(sim.is_done(5.0));
assert_eq!(sim.x(5.0), 1.0); // exactly the target once at rest
# Ok(())
# }
```

`FrictionSimulation` takes a drag in `(0, 1)`, the fraction of velocity kept per second. It
keeps small motion as the drag approaches one, rests once the remaining glide is within the
tolerance, and answers `f64::INFINITY` from `time_at_x` for a position it never reaches.

```rust
use flui_animation::{FrictionSimulation, Simulation, Tolerance};

# fn main() -> Result<(), flui_animation::SimulationError> {
let sim = FrictionSimulation::try_new(0.135, 0.0, 1000.0, Tolerance::DEFAULT)?;
let rest = sim.x(f64::INFINITY);
assert!(sim.time_at_x(rest / 2.0) > 0.0);
assert_eq!(sim.time_at_x(-1.0), f64::INFINITY); // behind the start: never
# Ok(())
# }
```

`BoundedFrictionSimulation` stops at the bound it travels toward. `BouncingScrollSimulation`
lets friction carry past an edge, hands over to a spring there with the same position and
velocity, overshoots and returns to rest exactly on the edge.

```rust
use flui_animation::{
    BouncingScrollSimulation, Simulation, SimulationBounds, SpringDescription, Tolerance,
};

# fn main() -> Result<(), flui_animation::SimulationError> {
let bounds = SimulationBounds::try_new(0.0, 100.0)?;
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.75)?;
let sim = BouncingScrollSimulation::try_new(spring, 0.135, 90.0, 2000.0, bounds, Tolerance::DEFAULT)?;
assert!(sim.is_done(10.0));
assert_eq!(sim.x(10.0), 100.0);
assert!(SimulationBounds::try_new(1.0, 0.0).is_err());
# Ok(())
# }
```

A `Tolerance` is a distance and a speed limit in the simulation's units. Scroll physics use
`Tolerance::for_device_pixel_ratio`, half a device pixel, with a speed limit derived from the
motion's own time scale.

```rust
use flui_animation::{SpeedLimit, Tolerance};

# fn main() -> Result<(), flui_animation::SimulationError> {
let precise = Tolerance::try_new(1e-4, SpeedLimit::AtMost(1e-4))?;
let screen = Tolerance::for_device_pixel_ratio(2.0)?;
assert_eq!(screen, Tolerance::try_new(0.25, SpeedLimit::Derived)?);
# let _ = precise;
# Ok(())
# }
```

## Error handling

Controller operations return `Result<_, AnimationError>`. A refused operation preserves the
installed run, its displayed value and its original future, and is reported once as a `warn!` on
`flui_animation::refused` (ADR-0220). The enum is `#[non_exhaustive]`; the table lists the
variants and what produces them.

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
        .bounds(flui_animation::ValueRange::try_new(0.0, 100.0)?)
        .build();
    controller.forward()?;
    // The caller samples this manually driven controller with tick_at(Duration).
    Ok(controller)
}
# let running = animate(Duration::from_millis(300)).unwrap();
# drop(running);
# drop(controller);
```

`ValueRange::try_new` returns the narrow `RangeDefect`, so configuration code can tell invalid
endpoint data, ordering and span overflow apart without parsing a message:

```rust
use flui_animation::{RangeDefect, ValueRange};

assert_eq!(ValueRange::try_new(1.0, 0.0), Err(RangeDefect::Inverted));
assert_eq!(ValueRange::try_new(f64::NAN, 1.0), Err(RangeDefect::NonFinite));
assert_eq!(ValueRange::try_new(-f64::MAX, f64::MAX), Err(RangeDefect::SpanOverflow));
```

Bounded `target` and `from` infinities still clamp to the indicated finite bound.
[ADR-0198](../../../docs/adr/ADR-0198-typed-animation-admission-errors.md) records the input,
range and physical-construction error boundaries. Error types are not serialized
([ADR-0381](../../../docs/adr/ADR-0381-animation-errors-are-not-serialized.md)): match a refusal
where it happens or convert it at the application's own boundary.

## Validation

Simulation constructors return `Err(SimulationError)` on invalid input
(`SpringDescription::try_new`, `with_damping_ratio`, `with_duration_and_bounce`,
`with_response_and_damping`, `SpringSimulation::try_new`, `FrictionSimulation::try_new`,
`BoundedFrictionSimulation::try_new`, `BouncingScrollSimulation::try_new`,
`SimulationBounds::try_new`, `Tolerance::try_new`, `Tolerance::for_device_pixel_ratio`). These
constructors, meant for constants, panic instead:

| Constructor | Panics if |
|-------------|-----------|
| `Steps::new` | count = 0, or count = 1 with `JumpAt::None` |
| `Interval::new` | begin/end not finite or outside [0,1], end < begin |
| `Cubic::new` | any argument not finite, x1 or x2 outside [0,1], y1 or y2 outside [-1e6, 1e6] |
| `ThreePointCubic::new` | midpoint not strictly inside the unit square, a control x outside its segment, a control y outside [-1e6, 1e6], a coordinate not finite |
| `Elastic{In,Out,InOut}Curve::new` | period not finite or outside [1e-6, 1e6] |
| `Split::with_curves` | split not finite or outside [0,1] |

## Diagnostics and profiling

Enable `diagnostics` on this crate, or `animation-diagnostics` on `flui` and `flui-sdk`, to
compile structured DEBUG events at target `flui.animation.diagnostics`, for example with
`RUST_LOG=info,flui.animation.diagnostics=debug` under FLUI's managed log subscriber. Run
generations and resolved outcome generations correlate start, replacement, cancellation,
completion, disposal, direction, playback and clock or policy changes; an unchanged sample emits
no transition event.

The same feature emits `animation_tick`, `animation_sampling` and `animation_listeners` CPU spans
at target `flui.animation`. `flui-devtools`' `profiling` feature enables it and attributes the
spans to their owning scheduler frame (ADR-0202). A tick's duration includes sampling and
delivery, so the three are not additive; standalone ticks have no frame attribution, and the
spans do not measure GPU execution. Instrumentation is off by default, filtered observation
emits nothing, and a subscriber's callouts run outside controller borrows with their own panic
containment: observation can lose data on failure but never owns an animation outcome.
