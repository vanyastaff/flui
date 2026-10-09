# Animation Guide

Practical guide to using `flui_animation`.

Every `rust` block is compiled as a doctest against the current API. Lines
starting with `#` are hidden setup (a scheduler, a controller, a
`Result`-returning `main`) that the rendered page leaves out.

## Setup

```rust
use flui_animation::{
    AnimationController, Animation,
    Curves, FloatTween, Animatable,
};
use flui_scheduler::UpdateScheduler;
use std::rc::Rc;
use std::time::Duration;

let scheduler = Rc::new(UpdateScheduler::new());
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
# use flui_scheduler::UpdateScheduler;
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
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

// Jump to lower_bound, status = Dismissed
controller.reset()?;

// Set value directly (no animation)
controller.set_value(0.5);
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
// Loop: 0→1, 0→1, 0→1, ...
controller.repeat(false)?;

// Bounce: 0→1→0→1→0, ...
controller.repeat(true)?;

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
# use flui_scheduler::UpdateScheduler;
use flui_animation::{SpringDescription, SpringSimulation};
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

// Fling with velocity
controller.fling(1.0)?;   // toward upper_bound
controller.fling(-1.0)?;  // toward lower_bound

// Custom spring (fling refuses an oscillating spring)
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);
controller.fling_with(1.0, Some(spring))?;

// Arbitrary simulation
let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
controller.animate_with(sim)?;
# drop(controller);
# Ok(())
# }
```

### Reading State

```rust
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
use flui_animation::Animation;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

let value = controller.value();   // Current value
let status = controller.status(); // AnimationStatus

controller.is_animating(); // Forward or Reverse
controller.is_completed(); // At upper_bound
controller.is_dismissed(); // At lower_bound
# drop(controller);
```

### Listening

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
use flui_animation::{Animation, AnimationStatus};
use flui_foundation::Listenable;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

// Value changes (the callback owns its own handle to the controller)
let observed = controller.clone();
let id = controller.add_listener(Rc::new(move || {
    println!("value: {}", observed.value());
}));
controller.remove_listener(id);

// Status changes
let id = controller.add_status_listener(Rc::new(|status| {
    match status {
        AnimationStatus::Completed => println!("done"),
        AnimationStatus::Dismissed => println!("reset"),
        _ => {}
    }
}));
controller.remove_status_listener(id);
# drop(controller);
```

### Cleanup

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let mut owner = AnimationController::builder(Duration::from_millis(300)).build_on(None);
# let controller = owner.controller().clone();
owner.dispose();
// Driving operations now return Err(AnimationError::Disposed)
assert!(matches!(controller.forward(), Err(AnimationError::Disposed)));
```

---

## Curves

### Using Predefined Curves

```rust
use flui_animation::{Curve, Curves};
# let t = 0.25;

let value = Curves::EaseIn.transform(0.5);
let value = Curves::BounceOut.transform(t);
let value = Curves::ElasticOut.transform(t);
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

### Custom Curves

```rust
use flui_animation::{Cubic, Curves, ElasticOutCurve, Interval};

// Cubic bezier (CSS-style control points)
let curve = Cubic::new(0.25, 0.1, 0.25, 1.0);

// Elastic with custom period
let elastic = ElasticOutCurve::new(0.3);

// Active only in [0.2, 0.8]
let interval = Interval::new(0.2, 0.8, Curves::EaseIn);

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
use flui_animation::{Animatable, FloatTween};

let tween = FloatTween::new(0.0, 100.0);
let value = tween.transform(0.5); // 50.0
assert_eq!(value, 50.0);
```

### Available Tweens

```rust
use flui_animation::{
    AlignmentTween, BorderRadiusTween, ColorTween, ConstantTween, EdgeInsetsTween,
    FloatTween, IntTween, OffsetTween, RectTween, SizeTween, StepTween,
};
use flui_foundation::geometry::{Edges, Offset, Rect, Size};
use flui_painting::Alignment;
use flui_painting::styling::{BorderRadius, BorderRadiusExt, Color};
# let rect1 = Rect::new(0.0, 0.0, 10.0, 10.0);
# let rect2 = Rect::new(0.0, 0.0, 50.0, 50.0);

// Numeric
let _ = FloatTween::new(0.0, 100.0);
let _ = IntTween::new(0, 255);
let _ = StepTween::new(0, 10); // floors

// Color
let _ = ColorTween::new(Color::RED, Color::BLUE);

// Geometry
let _ = SizeTween::new(Size::ZERO, Size::new(100.0, 100.0));
let _ = OffsetTween::new(Offset::ZERO, Offset::new(50.0, 50.0));
let _ = RectTween::new(rect1, rect2);

// Layout
let _ = AlignmentTween::new(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT);
let _ = EdgeInsetsTween::new(Edges::ZERO, Edges::all(16.0));
let _ = BorderRadiusTween::new(BorderRadius::ZERO, BorderRadius::circular(8.0));

// Constant
let _ = ConstantTween::new(42.0);
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

```rust
use flui_animation::{ChainedTween, CurveTween, Curves, FloatTween, ReverseTween};
let tween = FloatTween::new(0.0, 100.0);
// A curve, then a value tween
let eased = ChainedTween::new(CurveTween::new(Curves::EaseIn), tween);

// Reverse
let reversed = ReverseTween::new(tween);
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
# use flui_animation::{AnimationController, Curves};
# use flui_scheduler::UpdateScheduler;
use flui_animation::CurvedAnimation;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

let curved = CurvedAnimation::new(
    Rc::new(controller.clone()),
    Curves::EaseInOut,
);

# drop(controller);
```

### TweenAnimation

Map 0–1 to any type:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, FloatTween};
# use flui_scheduler::UpdateScheduler;
use flui_animation::TweenAnimation;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

let animated = TweenAnimation::new(
    FloatTween::new(0.0, 300.0),
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
# use flui_scheduler::UpdateScheduler;
use flui_animation::ReverseAnimation;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

let reversed = ReverseAnimation::new(Rc::new(controller.clone()));
// value = 1.0 - parent.value()
// Forward ↔ Reverse, Completed ↔ Dismissed

# drop(controller);
```

### ProxyAnimation

Hot-swap parent:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
use flui_animation::ProxyAnimation;
# let scheduler = UpdateScheduler::new();
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
use flui_animation::{ALWAYS_COMPLETE, ALWAYS_DISMISSED, Animation, AnimationStatus, ConstantAnimation};

let stopped = ConstantAnimation::with_status(0.5, AnimationStatus::Completed);
let complete = ConstantAnimation::completed(1.0);

// Global statics
assert_eq!(ALWAYS_COMPLETE.value(), 1.0);
assert_eq!(ALWAYS_DISMISSED.value(), 0.0);
```

### AnimationSwitch

Switch at crossover:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
use flui_animation::AnimationSwitch;
# let scheduler = UpdateScheduler::new();
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
// For constants: panics on invalid input.
let critical = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);
# let _ = (physical, perceptual, response, critical);
# Ok(())
# }
```

### SpringSimulation

```rust
use flui_animation::simulation::{Simulation, SpringDescription, SpringSimulation, Tolerance};

# fn main() -> Result<(), flui_animation::simulation::SimulationError> {
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.7);
let sim = SpringSimulation::try_new(spring, 0.0, 1.0, 0.0, Tolerance::DEFAULT)?;
assert!(sim.dx(0.1).is_finite()); // velocity at t = 0.1 s
assert!(sim.is_done(5.0));
assert_eq!(sim.x(5.0), 1.0);
# Ok(())
# }
```

### FrictionSimulation

```rust
use flui_animation::simulation::{FrictionSimulation, Tolerance};

# fn main() -> Result<(), flui_animation::simulation::SimulationError> {
// drag in (0, 1), position, velocity
let sim = FrictionSimulation::new(0.05, 0.0, 100.0, Tolerance::DEFAULT)?;
let resting = sim.final_x();
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
# use flui_scheduler::UpdateScheduler;
use flui_animation::{AnimationController, AnimationError};
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();

match controller.forward() {
    Ok(_future) => { /* started; the future resolves when the run ends */ }
    Err(AnimationError::Disposed) => { /* disposed */ }
    Err(AnimationError::NonFiniteTarget(why)) => { /* refused input */ }
    Err(e) => { /* other error */ }
}

// Propagation
fn animate(d: Duration) -> Result<AnimationController, AnimationError> {
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
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
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
# use flui_animation::{AnimationController, CurvedAnimation, Curves};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
let controller = AnimationController::builder(Duration::from_millis(300)).build();
let curved1 = CurvedAnimation::new(Rc::new(controller.clone()), Curves::EaseIn);
let curved2 = CurvedAnimation::new(Rc::new(controller.clone()), Curves::EaseOut);
# drop(controller);
```

### Reuse Controllers

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
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
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
// Bad: check every frame
if controller.status() == AnimationStatus::Completed { /* ... */ }

// Good: react to changes
controller.add_status_listener(Rc::new(|status| {
    if status == AnimationStatus::Completed { /* ... */ }
}));
# drop(controller);
```
