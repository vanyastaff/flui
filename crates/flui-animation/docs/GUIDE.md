# Animation Guide

Practical guide to using `flui_animation`.

Standalone `rust` blocks are compiled as doctests. Blocks marked
`rust,ignore` are intentionally context-dependent continuations of the setup
or controller created by an earlier section.

## Setup

```rust
use flui_animation::{
    AnimationController, Animation, CurvedAnimation,
    Curves, FloatTween, Animatable,
};
use flui_scheduler::UpdateScheduler;
use std::sync::Arc;
use std::time::Duration;

let scheduler = Arc::new(UpdateScheduler::new());
```

## AnimationController

### Creating

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
// Simple
let controller = AnimationController::new(
    Duration::from_millis(300),
    &scheduler,
);

// With custom bounds
let controller = AnimationController::with_bounds(
    Duration::from_millis(300),
    &scheduler,
    0.0,
    100.0,
)?;

// With builder (full control)
let controller = AnimationController::builder(
    Duration::from_millis(300),
    &scheduler,
)
.bounds(0.0, 100.0)?
.initial_value(50.0)
.reverse_duration(Duration::from_millis(500))
.build()?;
# controller.dispose();
# Ok(())
# }
```

### Driving

```rust,ignore
// Forward (toward upper_bound)
controller.forward()?;

// Reverse (toward lower_bound)
controller.reverse()?;

// From specific value
controller.forward_from(0.5)?;
controller.reverse_from(0.8)?;

// Stop at current position
controller.stop();

// Jump to lower_bound, status = Dismissed
controller.reset();

// Set value directly (no animation)
controller.set_value(0.5);
```

### Repeating

```rust,ignore
// Loop: 0→1, 0→1, 0→1, ...
controller.repeat(false)?;

// Bounce: 0→1→0→1→0, ...
controller.repeat(true)?;

// Stop repeating
controller.stop();
```

### Physics

```rust,ignore
use flui_animation::{SpringDescription, SpringSimulation};

// Fling with velocity
controller.fling(1.0)?;   // toward upper_bound
controller.fling(-1.0)?;  // toward lower_bound

// Custom spring
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.7);
controller.fling_with(1.0, spring)?;

// Arbitrary simulation
let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
controller.animate_with(sim)?;
```

### Reading State

```rust,ignore
let value = controller.value();      // Current value
let status = controller.status();    // AnimationStatus

controller.is_animating();  // Forward or Reverse
controller.is_completed();  // At upper_bound
controller.is_dismissed();  // At lower_bound
```

### Listening

```rust,ignore
// Value changes
let id = controller.add_listener(|| {
    println!("value: {}", controller.value());
});
controller.remove_listener(id);

// Status changes
let id = controller.add_status_listener(|status| {
    match status {
        AnimationStatus::Completed => println!("done"),
        AnimationStatus::Dismissed => println!("reset"),
        _ => {}
    }
});
controller.remove_status_listener(id);
```

### Cleanup

```rust,ignore
controller.dispose();
// All operations now return Err(AlreadyDisposed)
```

---

## Curves

### Using Predefined Curves

```rust
use flui_animation::{Curve, Curves};

let t = 0.3;
let eased = Curves::EaseIn.transform(0.5);
let bounced = Curves::BounceOut.transform(t);
let springy = Curves::ElasticOut.transform(t);

// The input policy every curve follows (see the `Curve` trait docs):
assert_eq!(Curves::EaseIn.transform(0.0), 0.0); // exact ends
assert_eq!(Curves::EaseIn.transform(1.5), 1.0); // outside [0, 1] clamps
assert!(Curves::EaseIn.transform(f64::NAN).is_nan()); // NaN stays NaN
```

### Available Curves

| Category | Curves |
|----------|--------|
| Linear | `Linear` |
| Ease | `EaseIn`, `EaseOut`, `EaseInOut` |
| Material | `FastOutSlowIn`, `SlowOutFastIn` |
| Sine | `EaseInSine`, `EaseOutSine`, `EaseInOutSine` |
| Expo | `EaseInExpo`, `EaseOutExpo`, `EaseInOutExpo` |
| Circ | `EaseInCirc`, `EaseOutCirc`, `EaseInOutCirc` |
| Back | `EaseInBack`, `EaseOutBack`, `EaseInOutBack` |
| Elastic | `ElasticIn`, `ElasticOut`, `ElasticInOut` |
| Bounce | `BounceIn`, `BounceOut`, `BounceInOut` |
| Other | `Decelerate` |

Every constant is monotone except the `Back`, `Elastic` and `Bounce` families.
`Cubic` constants are solved to within `1e-7` of the exact bezier output.

### Custom Curves

```rust
use flui_animation::{Cubic, CurveError, Curves, ElasticOutCurve, Interval};

// Cubic bezier (CSS `cubic-bezier` control points; x1 and x2 in [0, 1])
let curve = Cubic::new(0.25, 0.1, 0.25, 1.0);

// Elastic with custom period
let elastic = ElasticOutCurve::new(0.3);

// Active only in [0.2, 0.8]
let interval = Interval::new(0.2, 0.8, Curves::EaseIn);

// Parameters from data: `try_new` reports what is wrong instead of panicking
let error = ElasticOutCurve::try_new(0.0).expect_err("period must be > 0");
assert!(matches!(error, CurveError::OutOfRange { parameter: "period", .. }));
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
use flui_animation::{Curve, Curves};

// The 180° rotation 1.0 - curve(1.0 - t): ease-in becomes ease-out.
let flipped = Curves::EaseIn.flipped();
assert_eq!(flipped.transform(1.0), 1.0);
```

To play a curve backwards in time, reverse the driving animation
(`ReverseAnimation`), not the curve.

---

## Tweens

### Basic Usage

```rust,ignore
use flui_animation::{FloatTween, Animatable};

let tween = FloatTween::new(0.0, 100.0);
let value = tween.transform(0.5);  // 50.0
```

### Available Tweens

```rust,ignore
use flui_animation::*;
use flui_foundation::geometry::{EdgeInsets, Offset, Rect, Size};
use flui_painting::Alignment;
use flui_painting::styling::{BorderRadius, BorderRadiusExt, Color};

// Numeric
FloatTween::new(0.0, 100.0)
IntTween::new(0, 255)
StepTween::new(0, 10)  // floors

// Color
ColorTween::new(Color::RED, Color::BLUE)

// Geometry
SizeTween::new(Size::ZERO, Size::new(100.0, 100.0))
OffsetTween::new(Offset::ZERO, Offset::new(50.0, 50.0))
RectTween::new(rect1, rect2)

// Layout
AlignmentTween::new(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT)
EdgeInsetsTween::new(EdgeInsets::ZERO, EdgeInsets::all(16.0))
BorderRadiusTween::new(BorderRadius::ZERO, BorderRadius::circular(8.0))

// Constant
ConstantTween::new(42.0)
```

### Keyframes

A `Keyframes<T>` track (`T: Lerp + TwoWayConverter`) is a value as a pure
function of elapsed time. Segments are timed by `Duration` and placed end to
end from zero:

| Segment | Moves | Velocity at a join with `cubic` |
|---|---|---|
| `to(value, over, curve)` | eases into `value`; the curve belongs to this segment | the curve's slope at that end |
| `cubic(value, over)` | Catmull-Rom spline through the keyframe *times* | the chord of the neighbouring keys |
| `hold(over)` | keeps the value | zero |
| `jump(value)` | changes the value instantly | zero |

At a boundary the track returns the keyframe value exactly; at a jump, the
value after it. A curve that returns NaN or infinity, or interpolation that
overflows, publishes the segment's start value. `build` reports a zero
total, a segment past the total, a `Duration` overflow or a non-finite
keyframe as a `KeyframesError`.

```rust
use std::time::Duration;
use flui_animation::{Animatable, Curves, Keyframes};

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
    .to(0.0, ms(1000), Curves::EaseIn)
    .build()
    .expect("fits");
assert_eq!(fade.transform(0.5), 1.0);
```

Repeat with a repeating controller (or `value_at_looped`). `Stagger` gives
element `i` of `n` the delay `step · |origin − i|` (`First`, `Last`,
`Center`, `Index`), so one controller drives every element:
`track.value_at_looped(elapsed + track.total() - stagger.delay(i, n))`.

### Chaining and Composition

```rust,ignore
// A curve, then a value tween
let eased = ChainedTween::new(CurveTween::new(Curves::EaseIn), tween);

// Reverse
let reversed = ReverseTween::new(tween);
```

---

## Composition

### CurvedAnimation

Apply curve to animation output:

```rust,ignore
use flui_animation::CurvedAnimation;

let curved = CurvedAnimation::new(
    controller.clone(),
    Curves::EaseInOut,
);
```

### TweenAnimation

Map 0–1 to any type:

```rust,ignore
use flui_animation::TweenAnimation;

let animated = TweenAnimation::new(
    controller.clone(),
    FloatTween::new(0.0, 300.0),
);

let pixels = animated.value();  // 0.0 to 300.0
```

### ReverseAnimation

```rust,ignore
use flui_animation::ReverseAnimation;

let reversed = ReverseAnimation::new(controller.clone());
// value = 1.0 - parent.value()
// Forward ↔ Reverse, Completed ↔ Dismissed
```

### CompoundAnimation

```rust,ignore
use flui_animation::{CompoundAnimation, AnimationOperator};

let sum = CompoundAnimation::new(a, b, AnimationOperator::Add);
let min = CompoundAnimation::new(a, b, AnimationOperator::Min);
let mean = CompoundAnimation::mean(a, b);

// With extensions
let sum = Arc::new(a).add(Arc::new(b));
let diff = Arc::new(a).subtract(Arc::new(b));
```

### ProxyAnimation

Hot-swap parent:

```rust,ignore
use flui_animation::ProxyAnimation;

let proxy = ProxyAnimation::new(controller1.clone());
// Later...
proxy.set_parent(controller2.clone());
```

### ConstantAnimation

Fixed value:

```rust,ignore
use flui_animation::{ConstantAnimation, ALWAYS_COMPLETE, ALWAYS_DISMISSED};

let stopped = ConstantAnimation::new(0.5, AnimationStatus::Completed);
let complete = ConstantAnimation::completed(1.0);

// Global constants
let _ = ALWAYS_COMPLETE.value();  // 1.0
let _ = ALWAYS_DISMISSED.value(); // 0.0
```

### AnimationSwitch

Switch at crossover:

```rust,ignore
use flui_animation::AnimationSwitch;

let switch = AnimationSwitch::new(anim1, Some(anim2));
// When values cross, switches from anim1 to anim2
```

---

## Physics Simulations

### SpringDescription

```rust,ignore
use flui_animation::SpringDescription;

// Explicit parameters
let spring = SpringDescription::new(
    1.0,    // mass
    500.0,  // stiffness
    10.0,   // damping
);

// From damping ratio (intuitive)
let spring = SpringDescription::with_damping_ratio(
    1.0,    // mass
    500.0,  // stiffness
    1.0,    // 1.0 = critical, <1 = bouncy, >1 = overdamped
);

// From feel
let spring = SpringDescription::with_duration_and_bounce(
    0.5,    // perceptual duration (seconds)
    0.3,    // bounce (0 = none, higher = more)
);
```

### SpringSimulation

```rust,ignore
use flui_animation::SpringSimulation;

let sim = SpringSimulation::new(spring, start, end, velocity);

sim.x(0.1);       // position at t=0.1
sim.dx(0.1);      // velocity at t=0.1
sim.is_done(0.1); // within tolerance?
```

### FrictionSimulation

```rust,ignore
use flui_animation::FrictionSimulation;

let sim = FrictionSimulation::new(
    0.05,   // drag (0 < drag < 1, drag ≠ 1)
    0.0,    // position
    100.0,  // velocity
);

sim.final_x();     // resting position
sim.time_at_x(x);  // time to reach x
```

### GravitySimulation

```rust,ignore
use flui_animation::GravitySimulation;

let sim = GravitySimulation::new(
    9.8,    // acceleration
    0.0,    // position
    10.0,   // velocity
    100.0,  // end position
);
```

---

## Error Handling

```rust,ignore
use flui_animation::AnimationError;

match controller.forward() {
    Ok(()) => { /* started */ }
    Err(AnimationError::AlreadyDisposed) => { /* disposed */ }
    Err(AnimationError::InvalidBounds) => { /* bad bounds */ }
    Err(e) => { /* other error */ }
}

// Propagation
fn animate() -> Result<(), AnimationError> {
    let controller = AnimationController::builder(d, s)
        .bounds(0.0, 100.0)?
        .build()?;
    controller.forward()?;
    Ok(())
}
```

---

## Best Practices

### Always Dispose

```rust,ignore
let controller = AnimationController::new(duration, &scheduler);
// ... use controller ...
controller.dispose();  // Required
```

### Use Arc for Sharing

```rust,ignore
let controller = Arc::new(AnimationController::new(...));
let curved1 = CurvedAnimation::new(controller.clone(), Curves::EaseIn);
let curved2 = CurvedAnimation::new(controller.clone(), Curves::EaseOut);
```


### Reuse Controllers

```rust,ignore
// Don't create new controller each time
controller.reset();
controller.forward()?;
```

### Use Status Listeners (Not Polling)

```rust,ignore
// Bad: check every frame
if controller.status() == AnimationStatus::Completed { ... }

// Good: react to changes
controller.add_status_listener(|status| {
    if status == AnimationStatus::Completed { ... }
});
```
