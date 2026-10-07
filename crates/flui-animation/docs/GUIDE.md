# Animation Guide

Practical guide to using `flui_animation`.

Every `rust` block is compiled as a doctest against the current API. Lines
starting with `#` are hidden setup (a scheduler, a controller, a
`Result`-returning `main`) that the rendered page leaves out.

## Setup

```rust
use flui_animation::{
    AnimationController, Animation, AnimationExt,
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

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
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
// Loop: 0→1, 0→1, 0→1, ...
controller.repeat(false)?;

// Bounce: 0→1→0→1→0, ...
controller.repeat(true)?;

// Stop repeating
controller.stop()?;
# controller.dispose();
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
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);

// Fling with velocity
controller.fling(1.0)?;   // toward upper_bound
controller.fling(-1.0)?;  // toward lower_bound

// Custom spring (fling refuses an oscillating spring)
let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);
controller.fling_with(1.0, Some(spring))?;

// Arbitrary simulation
let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
controller.animate_with(sim)?;
# controller.dispose();
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
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);

let value = controller.value();   // Current value
let status = controller.status(); // AnimationStatus

controller.is_animating(); // Forward or Reverse
controller.is_completed(); // At upper_bound
controller.is_dismissed(); // At lower_bound
# controller.dispose();
```

### Listening

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
use flui_animation::{Animation, AnimationStatus};
use flui_foundation::Listenable;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);

// Value changes (the callback owns its own handle to the controller)
let observed = controller.clone();
let id = controller.add_listener(Arc::new(move || {
    println!("value: {}", observed.value());
}));
controller.remove_listener(id);

// Status changes
let id = controller.add_status_listener(Arc::new(|status| {
    match status {
        AnimationStatus::Completed => println!("done"),
        AnimationStatus::Dismissed => println!("reset"),
        _ => {}
    }
}));
controller.remove_status_listener(id);
# controller.dispose();
```

### Cleanup

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
controller.dispose();
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

### Splines

```rust
use flui_animation::CatmullRomCurve;

let spline = CatmullRomCurve::with_points(vec![
    (0.0, 0.0),
    (0.3, 0.8),
    (0.7, 0.2),
    (1.0, 1.0),
]);
```

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

### Tween Sequences

```rust
use flui_animation::{Animatable, FloatTween, TweenSequence, TweenSequenceItem};

let sequence = TweenSequence::new(vec![
    TweenSequenceItem::new(FloatTween::new(0.0, 100.0), 1.0),
    TweenSequenceItem::new(FloatTween::new(100.0, 100.0), 2.0), // hold
    TweenSequenceItem::new(FloatTween::new(100.0, 0.0), 1.0),
]);

// Weights: 1 + 2 + 1 = 4
// t ∈ [0.00, 0.25] → first tween
// t ∈ [0.25, 0.75] → second tween (hold at 100)
// t ∈ [0.75, 1.00] → third tween
assert!((sequence.transform(0.5) - 100.0).abs() < 1e-9);
```

### Chaining and Composition

```rust
use flui_animation::{AnimatableExt, Curves, FloatTween};
# let tween = FloatTween::new(0.0, 100.0);
# let tween1 = FloatTween::new(0.0, 1.0);
# let tween2 = FloatTween::new(0.0, 100.0);

// Apply curve
let eased = tween.with_curve(Curves::EaseIn);

// Chain tweens: tween1's output is tween2's `t`
let chained = tween1.chain(tween2);

// Reverse
let reversed = tween.reversed();
```

---

## Composition

Every composition type takes its parent as `Arc<dyn Animation<f64>>`;
`AnimationController` is a cheap handle, so `Arc::new(controller.clone())`
shares the same controller.

### CurvedAnimation

Apply curve to animation output:

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationExt, Curves};
# use flui_scheduler::UpdateScheduler;
use flui_animation::CurvedAnimation;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);

let curved = CurvedAnimation::new(
    Arc::new(controller.clone()),
    Curves::EaseInOut,
);

// Or with extension
let curved = Arc::new(controller.clone()).curved(Curves::EaseInOut);
# controller.dispose();
```

### TweenAnimation

Map 0–1 to any type:

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, FloatTween};
# use flui_scheduler::UpdateScheduler;
use flui_animation::TweenAnimation;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);

let animated = TweenAnimation::new(
    FloatTween::new(0.0, 300.0),
    Arc::new(controller.clone()),
);

let pixels = animated.value(); // 0.0 to 300.0
# controller.dispose();
```

### ReverseAnimation

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationExt};
# use flui_scheduler::UpdateScheduler;
use flui_animation::ReverseAnimation;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);

let reversed = ReverseAnimation::new(Arc::new(controller.clone()));
// value = 1.0 - parent.value()
// Forward ↔ Reverse, Completed ↔ Dismissed

// Or with extension
let reversed = Arc::new(controller.clone()).reversed();
# controller.dispose();
```

### CompoundAnimation

```rust
# use std::sync::Arc;
# use flui_animation::{Animation, AnimationExt, ConstantAnimation};
use flui_animation::{AnimationOperator, CompoundAnimation};
# let a = ConstantAnimation::new(0.25);
# let b = ConstantAnimation::new(0.75);
# let (a_dyn, b_dyn): (Arc<dyn Animation<f64>>, Arc<dyn Animation<f64>>) =
#     (Arc::new(a.clone()), Arc::new(b.clone()));

let sum = CompoundAnimation::new(a_dyn.clone(), b_dyn.clone(), AnimationOperator::Add);
let min = CompoundAnimation::new(a_dyn.clone(), b_dyn.clone(), AnimationOperator::Min);
let mean = CompoundAnimation::mean(a_dyn, b_dyn);

// With extensions
let sum = Arc::new(a.clone()).add(Arc::new(b.clone()));
let diff = Arc::new(a).subtract(Arc::new(b));
```

### ProxyAnimation

Hot-swap parent:

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
use flui_animation::ProxyAnimation;
# let scheduler = UpdateScheduler::new();
# let controller1 = AnimationController::new(Duration::from_millis(300), &scheduler);
# let controller2 = AnimationController::new(Duration::from_millis(300), &scheduler);

let proxy = ProxyAnimation::new(Arc::new(controller1.clone()));
// Later...
proxy.set_parent(Arc::new(controller2.clone()));
# controller1.dispose();
# controller2.dispose();
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
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
use flui_animation::AnimationSwitch;
# let scheduler = UpdateScheduler::new();
# let anim1 = AnimationController::new(Duration::from_millis(300), &scheduler);
# let anim2 = AnimationController::new(Duration::from_millis(300), &scheduler);

let switch = AnimationSwitch::new(Arc::new(anim1.clone()), Some(Arc::new(anim2.clone())));
// When values cross, switches from anim1 to anim2
# switch.dispose();
# anim1.dispose();
# anim2.dispose();
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
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);

match controller.forward() {
    Ok(_future) => { /* started; the future resolves when the run ends */ }
    Err(AnimationError::Disposed) => { /* disposed */ }
    Err(AnimationError::NonFiniteTarget(why)) => { /* refused input */ }
    Err(e) => { /* other error */ }
}

// Propagation
fn animate(d: Duration, s: &UpdateScheduler) -> Result<(), AnimationError> {
    let controller = AnimationController::builder(d, s)
        .bounds(0.0, 100.0)?
        .build()?;
    controller.forward()?;
    controller.dispose();
    Ok(())
}
# animate(Duration::from_millis(300), &scheduler).unwrap();
# controller.dispose();
```

---

## Best Practices

### Always Dispose

```rust
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let duration = Duration::from_millis(300);
let controller = AnimationController::new(duration, &scheduler);
// ... use controller ...
controller.dispose(); // Required
```

### Share One Controller

`AnimationController` is a handle over shared state: `clone()` shares the
controller, it does not copy it.

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{AnimationController, CurvedAnimation, Curves};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
let curved1 = CurvedAnimation::new(Arc::new(controller.clone()), Curves::EaseIn);
let curved2 = CurvedAnimation::new(Arc::new(controller.clone()), Curves::EaseOut);
# controller.dispose();
```

### Prefer Extension Traits

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationExt, CurvedAnimation, Curves};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
# let curve = Curves::EaseIn;
// Verbose
let curved = CurvedAnimation::new(Arc::new(controller.clone()), curve);

// Fluent
let curved = Arc::new(controller.clone()).curved(curve);
# controller.dispose();
```

### Reuse Controllers

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
// Don't create new controller each time
controller.reset()?;
controller.forward()?;
# controller.dispose();
# Ok(())
# }
```

### Use Status Listeners (Not Polling)

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, AnimationStatus};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
// Bad: check every frame
if controller.status() == AnimationStatus::Completed { /* ... */ }

// Good: react to changes
controller.add_status_listener(Arc::new(|status| {
    if status == AnimationStatus::Completed { /* ... */ }
}));
# controller.dispose();
```
