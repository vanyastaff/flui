# flui_animation

Persistent animation objects for the FLUI framework, following Flutter's proven animation architecture with idiomatic Rust patterns.

## Overview

This crate provides the animation infrastructure for FLUI:

| Type | Description |
|------|-------------|
| `Animation<T>` | Base trait for all animations |
| `AnimationController` | Primary animation driver (0.0 to 1.0) |
| `AnimationControllerBuilder` | Builder for configuring controllers |
| `CurvedAnimation` | Apply easing curves to animations |
| `TweenAnimation<T>` | Map f32 values to any type T |
| `ReverseAnimation` | Invert an animation's values |
| `ProxyAnimation` | Hot-swap animations at runtime |
| `CompoundAnimation` | Combine animations with operators |
| `AnimatableExt` | Extension trait for tweens |
| `AnimationExt` | Extension trait for composition |

## Architecture

```
flui_widgets (AnimatedWidget, Transitions)
       │
       ▼
flui_animation (this crate)
       │
       ├── flui_types/animation (Curve, Tween, AnimationStatus)
       └── flui-scheduler (Scheduler, Ticker)
```

## Quick Start

```rust
use flui_animation::{AnimationController, Animation, AnimationExt};
use flui_scheduler::Scheduler;
use flui_types::animation::Curves;
use std::sync::Arc;
use std::time::Duration;

fn example() -> Result<(), flui_animation::AnimationError> {
    let scheduler = Arc::new(Scheduler::new());

    // Create controller with builder
    let controller = AnimationController::builder(
        Duration::from_millis(300),
        scheduler,
    )
    .bounds(0.0, 1.0)?
    .build()?;

    // Apply curve using extension trait
    let curved = Arc::new(controller.clone()).curved(Curves::EaseInOut);

    // Start animation
    controller.forward()?;

    // Get value
    let value = curved.value();

    // Cleanup
    controller.dispose();
    Ok(())
}
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `serde` | Enable serialization support for animation types |

## Documentation

| Document | Description |
|----------|-------------|
| [GUIDE.md](docs/GUIDE.md) | Complete usage guide |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | System internals and design |
| [PATTERNS.md](docs/PATTERNS.md) | Design patterns and rationale |
| [PERFORMANCE.md](docs/PERFORMANCE.md) | Performance characteristics |

## Testing

```bash
cargo test -p flui_animation
```

## Related Crates

- `flui_types` - Core types including Curve, Tween, AnimationStatus
- `flui-scheduler` - Frame scheduling and ticker system
- `flui-foundation` - Listenable trait and change notification
