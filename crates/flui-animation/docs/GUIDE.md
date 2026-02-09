# Animation Guide

Practical guide to using `flui_animation`.

## Setup

```rust
use flui_animation::{
    AnimationController, Animation, AnimationExt,
    Curves, FloatTween, Animatable,
};
use flui_scheduler::Scheduler;
use std::sync::Arc;
use std::time::Duration;

let scheduler = Arc::new(Scheduler::new());
```

## AnimationController

### Creating

```rust
// Simple
let controller = AnimationController::new(
    Duration::from_millis(300),
    scheduler.clone(),
);

// With custom bounds
let controller = AnimationController::with_bounds(
    Duration::from_millis(300),
    scheduler.clone(),
    0.0,
    100.0,
)?;

// With builder (full control)
let controller = AnimationController::builder(
    Duration::from_millis(300),
    scheduler.clone(),
)
.bounds(0.0, 100.0)?
.initial_value(50.0)?
.reverse_duration(Duration::from_millis(500))
.build()?;
```

### Driving

```rust
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

```rust
// Loop: 0→1, 0→1, 0→1, ...
controller.repeat()?;

// Bounce: 0→1→0→1→0, ...
controller.repeat_with_reverse(true)?;

// Stop repeating
controller.stop();
```

### Physics

```rust
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

```rust
let value = controller.value();      // Current value
let status = controller.status();    // AnimationStatus

controller.is_animating();  // Forward or Reverse
controller.is_completed();  // At upper_bound
controller.is_dismissed();  // At lower_bound
```

### Listening

```rust
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

```rust
controller.dispose();
// All operations now return Err(AlreadyDisposed)
```

---

## Curves

### Using Predefined Curves

```rust
use flui_animation::Curves;

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
use flui_animation::{Cubic, ElasticOutCurve, Interval, Threshold};

// Cubic bezier (CSS-style control points)
let curve = Cubic::new(0.25, 0.1, 0.25, 1.0);

// Elastic with custom period
let elastic = ElasticOutCurve::new(0.3);

// Active only in [0.2, 0.8]
let interval = Interval::new(0.2, 0.8, Curves::EaseIn);

// Step at threshold
let step = Threshold::new(0.5);
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
let flipped = curve.flipped();   // 1.0 - curve(t)
let reversed = curve.reversed(); // curve(1.0 - t)
```

---

## Tweens

### Basic Usage

```rust
use flui_animation::{FloatTween, Animatable};

let tween = FloatTween::new(0.0, 100.0);
let value = tween.transform(0.5);  // 50.0
```

### Available Tweens

```rust
use flui_animation::*;
use flui_types::styling::Color;
use flui_types::geometry::{Size, Offset, Rect};
use flui_types::layout::{Alignment, EdgeInsets};

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

### Tween Sequences

```rust
use flui_animation::{TweenSequence, TweenSequenceItem, FloatTween};

let sequence = TweenSequence::new(vec![
    TweenSequenceItem::new(FloatTween::new(0.0, 100.0), 1.0),
    TweenSequenceItem::new(FloatTween::new(100.0, 100.0), 2.0), // hold
    TweenSequenceItem::new(FloatTween::new(100.0, 0.0), 1.0),
]);

// Weights: 1 + 2 + 1 = 4
// t ∈ [0.00, 0.25] → first tween
// t ∈ [0.25, 0.75] → second tween (hold at 100)
// t ∈ [0.75, 1.00] → third tween
```

### Chaining and Composition

```rust
use flui_animation::TweenAnimatableExt;

// Apply curve
let eased = tween.with_curve(Curves::EaseIn);

// Chain tweens
let chained = tween1.chain(tween2);

// Reverse
let reversed = tween.reversed();
```

---

## Composition

### CurvedAnimation

Apply curve to animation output:

```rust
use flui_animation::CurvedAnimation;

let curved = CurvedAnimation::new(
    controller.clone(),
    Curves::EaseInOut,
);

// Or with extension
let curved = Arc::new(controller).curved(Curves::EaseInOut);
```

### TweenAnimation

Map 0–1 to any type:

```rust
use flui_animation::TweenAnimation;

let animated = TweenAnimation::new(
    controller.clone(),
    FloatTween::new(0.0, 300.0),
);

let pixels = animated.value();  // 0.0 to 300.0
```

### ReverseAnimation

```rust
use flui_animation::ReverseAnimation;

let reversed = ReverseAnimation::new(controller.clone());
// value = 1.0 - parent.value()
// Forward ↔ Reverse, Completed ↔ Dismissed

// Or with extension
let reversed = Arc::new(controller).reversed();
```

### CompoundAnimation

```rust
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

```rust
use flui_animation::ProxyAnimation;

let proxy = ProxyAnimation::new(controller1.clone());
// Later...
proxy.set_parent(controller2.clone());
```

### ConstantAnimation

Fixed value:

```rust
use flui_animation::{ConstantAnimation, ALWAYS_COMPLETE, ALWAYS_DISMISSED};

let stopped = ConstantAnimation::new(0.5, AnimationStatus::Completed);
let complete = ConstantAnimation::completed(1.0);

// Global constants
let _ = ALWAYS_COMPLETE.value();  // 1.0
let _ = ALWAYS_DISMISSED.value(); // 0.0
```

### AnimationSwitch

Switch at crossover:

```rust
use flui_animation::AnimationSwitch;

let switch = AnimationSwitch::new(anim1, Some(anim2));
// When values cross, switches from anim1 to anim2
```

---

## Physics Simulations

### SpringDescription

```rust
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

```rust
use flui_animation::SpringSimulation;

let sim = SpringSimulation::new(spring, start, end, velocity);

sim.x(0.1);       // position at t=0.1
sim.dx(0.1);      // velocity at t=0.1
sim.is_done(0.1); // within tolerance?
```

### FrictionSimulation

```rust
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

```rust
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

```rust
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

```rust
let controller = AnimationController::new(duration, scheduler);
// ... use controller ...
controller.dispose();  // Required
```

### Use Arc for Sharing

```rust
let controller = Arc::new(AnimationController::new(...));
let curved1 = CurvedAnimation::new(controller.clone(), Curves::EaseIn);
let curved2 = CurvedAnimation::new(controller.clone(), Curves::EaseOut);
```

### Prefer Extension Traits

```rust
// Verbose
let curved = CurvedAnimation::new(Arc::new(controller), curve);

// Fluent
let curved = Arc::new(controller).curved(curve);
```

### Reuse Controllers

```rust
// Don't create new controller each time
controller.reset();
controller.forward()?;
```

### Use Status Listeners (Not Polling)

```rust
// Bad: check every frame
if controller.status() == AnimationStatus::Completed { ... }

// Good: react to changes
controller.add_status_listener(|status| {
    if status == AnimationStatus::Completed { ... }
});
```
