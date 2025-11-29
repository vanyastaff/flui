# Animation Guide

Complete guide to using the `flui_animation` crate.

## Table of Contents

- [Getting Started](#getting-started)
- [AnimationController](#animationcontroller)
- [Curves](#curves)
- [Tweens](#tweens)
- [Composition](#composition)
- [Listening to Changes](#listening-to-changes)
- [Error Handling](#error-handling)
- [Best Practices](#best-practices)

## Getting Started

### Basic Setup

Every animation needs a `Scheduler` for frame timing:

```rust
use flui_animation::{AnimationController, Animation};
use flui_scheduler::Scheduler;
use std::sync::Arc;
use std::time::Duration;

let scheduler = Arc::new(Scheduler::new());
let controller = AnimationController::new(
    Duration::from_millis(300),
    scheduler,
);
```

### Animation Lifecycle

```rust
// Start forward animation (0.0 → 1.0)
controller.forward()?;

// Reverse animation (1.0 → 0.0)
controller.reverse()?;

// Stop at current value
controller.stop()?;

// Reset to beginning (0.0)
controller.reset()?;

// IMPORTANT: Always dispose when done
controller.dispose();
```

## AnimationController

### Using the Builder

The builder pattern provides a fluent API with validation:

```rust
use flui_animation::{AnimationController, AnimationControllerBuilder};

let controller = AnimationController::builder(
    Duration::from_millis(300),
    scheduler,
)
.bounds(0.0, 100.0)?           // Custom range (default: 0.0 to 1.0)
.reverse_duration(Duration::from_millis(500))  // Slower reverse
.initial_value(50.0)           // Start in middle
.build()?;
```

### Direct Construction

For simple cases:

```rust
// Default bounds (0.0 to 1.0)
let controller = AnimationController::new(duration, scheduler);

// Custom bounds
let controller = AnimationController::with_bounds(
    duration,
    scheduler,
    0.0,
    100.0,
)?;
```

### Reading Values

```rust
// Current animation value
let value = controller.value();

// Current status
let status = controller.status();

// Check states
if controller.is_animating() { /* running */ }
if controller.is_completed() { /* at end */ }
if controller.is_dismissed() { /* at start */ }
```

### Setting Values Directly

```rust
// Jump to specific value (clamped to bounds)
controller.set_value(0.5);

// Animate from current position
controller.forward()?;
```

## Curves

Curves modify the animation's progression. Apply them with `CurvedAnimation`:

```rust
use flui_animation::CurvedAnimation;
use flui_types::animation::Curves;

let curved = CurvedAnimation::new(
    Arc::new(controller),
    Curves::EaseInOut,
);
```

### Using Extension Traits

More ergonomic with `AnimationExt`:

```rust
use flui_animation::AnimationExt;

let curved = Arc::new(controller).curved(Curves::EaseInOut);
```

### Different Forward/Reverse Curves

```rust
let curved = CurvedAnimation::new(controller, Curves::EaseIn)
    .with_reverse_curve(Curves::EaseOut);
```

### Available Curves

From `flui_types::animation::Curves`:

| Curve | Description |
|-------|-------------|
| `Linear` | Constant speed |
| `EaseIn` | Slow start, fast end |
| `EaseOut` | Fast start, slow end |
| `EaseInOut` | Slow start and end |
| `FastOutSlowIn` | Material Design standard |
| `EaseInSine` | Sine-based ease in |
| `EaseOutSine` | Sine-based ease out |
| `EaseInOutSine` | Sine-based ease in/out |
| `EaseInExpo` | Exponential ease in |
| `EaseOutExpo` | Exponential ease out |
| `EaseInBack` | Overshoot at start |
| `EaseOutBack` | Overshoot at end |
| `ElasticIn` | Elastic effect at start |
| `ElasticOut` | Elastic effect at end |

## Tweens

Tweens map the 0.0-1.0 animation value to any type:

```rust
use flui_animation::TweenAnimation;
use flui_types::animation::FloatTween;

let tween = FloatTween::new(0.0, 100.0);
let animation = TweenAnimation::new(tween, Arc::new(controller));

// When controller.value() = 0.5, animation.value() = 50.0
```

### Using Extension Traits

```rust
use flui_animation::AnimatableExt;

let animation = FloatTween::new(0.0, 100.0)
    .animate(Arc::new(controller) as Arc<dyn Animation<f32>>);
```

### Available Tweens

From `flui_types::animation`:

| Tween | Maps to |
|-------|---------|
| `FloatTween` | `f32` |
| `IntTween` | `i32` |
| `SizeTween` | `Size` |
| `OffsetTween` | `Offset` |
| `RectTween` | `Rect` |
| `ColorTween` | `Color` |
| `AlignmentTween` | `Alignment` |

### Custom Tweens

Implement `Animatable<T>`:

```rust
use flui_types::animation::Animatable;

#[derive(Debug, Clone)]
pub struct MyTween {
    begin: MyType,
    end: MyType,
}

impl Animatable<MyType> for MyTween {
    fn transform(&self, t: f32) -> MyType {
        // Interpolate between begin and end
        self.begin.lerp(&self.end, t)
    }
}
```

## Composition

### Reverse Animation

Inverts values (0.0 becomes 1.0):

```rust
use flui_animation::ReverseAnimation;

let reversed = ReverseAnimation::new(Arc::new(controller));

// Or with extension trait
let reversed = Arc::new(controller).reversed();
```

### Compound Animation

Combine two animations with operators:

```rust
use flui_animation::{CompoundAnimation, AnimationOperator};

// Addition
let sum = CompoundAnimation::new(
    Arc::new(controller1),
    Arc::new(controller2),
    AnimationOperator::Add,
);

// Or with extension traits
let sum = Arc::new(c1).add(Arc::new(c2) as Arc<dyn Animation<f32>>);
let product = Arc::new(c1).multiply(Arc::new(c2) as Arc<dyn Animation<f32>>);
```

### Proxy Animation

Hot-swap animations at runtime:

```rust
use flui_animation::ProxyAnimation;

let proxy = ProxyAnimation::new(Arc::new(controller1));

// Later, switch to different animation
proxy.set_parent(Arc::new(controller2));
```

### Chaining Transformations

```rust
let animation = Arc::new(controller)
    .curved(Curves::EaseInOut)  // Apply curve
    .reversed();                 // Then reverse

let color = ColorTween::new(RED, BLUE)
    .animate(animation as Arc<dyn Animation<f32>>);
```

## Listening to Changes

### Value Changes

Use the `Listenable` trait:

```rust
use flui_foundation::Listenable;

let listener_id = controller.add_listener(Arc::new(|| {
    println!("Value changed: {}", controller.value());
}));

// Remove when done
controller.remove_listener(listener_id);
```

### Status Changes

```rust
use flui_types::animation::AnimationStatus;

let listener_id = controller.add_status_listener(Arc::new(|status| {
    match status {
        AnimationStatus::Forward => println!("Started forward"),
        AnimationStatus::Reverse => println!("Started reverse"),
        AnimationStatus::Completed => println!("Reached end"),
        AnimationStatus::Dismissed => println!("Reached start"),
    }
}));

controller.remove_status_listener(listener_id);
```

## Error Handling

All fallible operations return `Result<T, AnimationError>`:

```rust
use flui_animation::AnimationError;

match controller.forward() {
    Ok(()) => println!("Animation started"),
    Err(AnimationError::Disposed) => {
        println!("Controller was disposed");
    }
    Err(AnimationError::InvalidBounds(msg)) => {
        println!("Invalid bounds: {}", msg);
    }
    Err(AnimationError::TickerNotAvailable) => {
        println!("No ticker available");
    }
}
```

### Propagating Errors

```rust
fn animate_widget() -> Result<(), AnimationError> {
    let controller = AnimationController::builder(duration, scheduler)
        .bounds(0.0, 100.0)?
        .build()?;
    
    controller.forward()?;
    Ok(())
}
```

## Best Practices

### Always Dispose Controllers

```rust
let controller = AnimationController::new(duration, scheduler);

// Use the controller...

// IMPORTANT: Prevent resource leaks
controller.dispose();
```

### Use Arc for Sharing

```rust
let controller = Arc::new(AnimationController::new(duration, scheduler));

// Share between multiple animations
let curved1 = CurvedAnimation::new(controller.clone(), Curves::EaseIn);
let curved2 = CurvedAnimation::new(controller.clone(), Curves::EaseOut);
```

### Prefer Extension Traits

```rust
// Instead of
let curved = CurvedAnimation::new(Arc::new(controller), curve);

// Prefer
let curved = Arc::new(controller).curved(curve);
```

### Use Builder for Complex Configuration

```rust
// Instead of multiple method calls
let controller = AnimationController::with_bounds(d, s, 0.0, 100.0)?;
controller.set_reverse_duration(Duration::from_millis(500));
controller.set_value(50.0);

// Prefer builder
let controller = AnimationController::builder(d, s)
    .bounds(0.0, 100.0)?
    .reverse_duration(Duration::from_millis(500))
    .initial_value(50.0)
    .build()?;
```

### Handle Errors Appropriately

```rust
// For animations that must succeed
controller.forward().expect("Animation should start");

// For recoverable situations
if let Err(e) = controller.forward() {
    tracing::warn!("Could not start animation: {}", e);
}

// For propagation
controller.forward()?;
```

### Clean Up Listeners

```rust
let id = controller.add_status_listener(callback);

// When no longer needed
controller.remove_status_listener(id);
```
