# flui_animation

Animation system for FLUI, based on Flutter's proven architecture.

## Core Concepts

### The Animation Model

Animations in FLUI follow Flutter's model: an `Animation<T>` produces values of type `T` over time. The animation itself doesn't know about time—it's driven externally by a ticker.

```
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

```rust
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
let controller = AnimationController::new(
    Duration::from_millis(300),
    scheduler.clone(),
);

// Or with builder for full control
let controller = AnimationController::builder(duration, scheduler)
    .bounds(0.0, 1.0)?
    .initial_value(0.5)?
    .reverse_duration(Duration::from_millis(200))
    .build()?;
```

### Driving Animations

```rust
controller.forward()?;           // Animate to upper_bound
controller.reverse()?;           // Animate to lower_bound
controller.forward_from(0.5)?;   // Jump to 0.5, then animate forward
controller.reverse_from(0.8)?;   // Jump to 0.8, then animate backward
controller.stop();               // Stop at current value
controller.reset();              // Jump to lower_bound, status = Dismissed
```

### Repeating

```rust
controller.repeat()?;                    // Loop: 0→1→0→1→...
controller.repeat_with_reverse(true)?;   // Bounce: 0→1→0→1→...
```

### Physics-Based Animation

```rust
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

```rust
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

```rust
controller.dispose();  // Stop animation, release ticker
// Controller is unusable after dispose
```

---

## Curves

A `Curve` maps `t ∈ [0, 1]` to an output in `[0, 1]`. Used for easing.

**Contract**: `transform(0.0) == 0.0` and `transform(1.0) == 1.0`.

### Predefined Curves

```rust
use flui_animation::Curves;

Curves::Linear        // Identity
Curves::EaseIn        // Slow start
Curves::EaseOut       // Slow end
Curves::EaseInOut     // Slow start and end
Curves::FastOutSlowIn // Material Design standard

Curves::BounceIn      // Bounce at start
Curves::BounceOut     // Bounce at end
Curves::BounceInOut   // Bounce both

Curves::ElasticIn     // Overshoot at start
Curves::ElasticOut    // Overshoot at end
Curves::ElasticInOut  // Overshoot both

Curves::Decelerate    // Fast start, gradual stop
```

### Custom Curves

```rust
// Cubic bezier (CSS-style)
let curve = Cubic::new(0.25, 0.1, 0.25, 1.0);

// Elastic with custom period
let elastic = ElasticOutCurve::new(0.3);

// Interval: active only in [0.2, 0.8]
let interval = Interval::new(0.2, 0.8, Curves::EaseIn);

// Threshold: step function at t=0.5
let step = Threshold::new(0.5);

// Catmull-Rom spline through points
let spline = CatmullRomCurve::with_points(vec![
    (0.0, 0.0),
    (0.3, 0.8),
    (0.7, 0.2),
    (1.0, 1.0),
]);
```

### Curve Modifiers

```rust
let flipped = curve.flipped();   // Output: 1.0 - curve(t)
let reversed = curve.reversed(); // Input: curve(1.0 - t)
```

---

## Tweens

An `Animatable<T>` transforms `t ∈ [0, 1]` into a value of type `T`.

A `Tween<T>` is an `Animatable` with explicit `begin` and `end` values.

### Built-in Tweens

```rust
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

```rust
let tween = FloatTween::new(0.0, 100.0);
let value = tween.transform(0.5);  // 50.0

// With animation
let position = tween.transform(controller.value());
```

### Tween Sequences

Chain tweens with weights:

```rust
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

```rust
use flui_animation::TweenAnimatableExt;

// Chain: first tween, then second
let chained = tween1.chain(tween2);

// Apply curve to tween output
let curved = tween.with_curve(Curves::EaseIn);

// Reverse direction
let reversed = tween.reversed();
```

### CurveTween

Apply a curve as an Animatable:

```rust
let curve_tween = CurveTween::new(Curves::EaseIn);
let eased = curve_tween.transform(0.5);  // EaseIn applied to 0.5
```

---

## Animation Composition

### CurvedAnimation

Apply a curve to an animation's output:

```rust
let curved = CurvedAnimation::new(controller.clone(), Curves::EaseInOut);

// Value is: curve.transform(controller.value())
let value = curved.value();
```

### TweenAnimation

Map animation output through a tween:

```rust
let tween = FloatTween::new(0.0, 300.0);
let animated = TweenAnimation::new(controller.clone(), tween);

// Value is: tween.transform(controller.value())
let pixels = animated.value();  // 0.0 to 300.0
```

### ReverseAnimation

Invert an animation:

```rust
let reversed = ReverseAnimation::new(controller.clone());

// value = 1.0 - parent.value()
// Forward becomes Reverse, Completed becomes Dismissed
```

### ProxyAnimation

Hot-swap the parent animation:

```rust
let proxy = ProxyAnimation::new(controller1.clone());

// Later, switch to different animation
proxy.set_parent(controller2.clone());
```

### CompoundAnimation

Combine two animations with an operator:

```rust
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

```rust
let stopped = ConstantAnimation::new(0.5, AnimationStatus::Completed);
let complete = ConstantAnimation::completed(1.0);
let dismissed = ConstantAnimation::dismissed(0.0);

// Predefined constants
use flui_animation::{ALWAYS_COMPLETE, ALWAYS_DISMISSED};
```

### AnimationSwitch

Switch between animations when they cross:

```rust
let switch = AnimationSwitch::new(anim1.clone(), Some(anim2.clone()));

// When anim1 and anim2 values cross, switches to anim2
// Useful for "train hopping" between overlapping animations
```

---

## Extension Traits

### AnimationExt

```rust
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

```rust
use flui_animation::TweenAnimatableExt;

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

```rust
use flui_animation::CurveExt;

// Convert curve to CurveTween
let tween = Curves::EaseIn.into_tween();

// Chain curves
let combined = Curves::EaseIn.then(Curves::EaseOut);
```

---

## Physics Simulations

### SpringDescription

Defines spring physics parameters:

```rust
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

```rust
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

Deceleration with drag:

```rust
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

```rust
let sim = GravitySimulation::new(
    9.8,    // acceleration
    0.0,    // initial position
    10.0,   // initial velocity
    100.0,  // end position (simulation ends here)
);
```

### Tolerance

All simulations use tolerance for `is_done()`:

```rust
let tolerance = Tolerance {
    distance: 0.01,   // Position tolerance
    velocity: 0.01,   // Velocity tolerance  
    time: 0.001,      // Time tolerance
};

let sim = SpringSimulation::with_tolerance(spring, 0.0, 1.0, 0.0, tolerance);
```

---

## Error Handling

```rust
pub enum AnimationError {
    InvalidBounds,      // lower_bound >= upper_bound
    InvalidValue,       // Value outside bounds
    InvalidDuration,    // Duration is zero or negative
    AlreadyDisposed,    // Operation on disposed controller
    AlreadyAnimating,   // Conflicting animation command
    TickerError,        // Scheduler/ticker failure
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
| `TweenSequenceItem::new` | weight ≤ 0, weight is infinite |
| `Interval::new` | begin/end outside [0,1], end < begin |
| `Threshold::new` | threshold outside [0,1] |
