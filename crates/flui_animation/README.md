# flui-animation

Persistent animation objects for the FLUI framework, following Flutter's proven animation architecture.

## ✨ Features

- **Type-Safe Animations**: Generic `Animation<T>` trait for any value type
- **Composable**: Chain animations with tweens, curves, and operators
- **Thread-Safe**: Full `Send + Sync` support with `Arc` and `parking_lot`
- **Persistent Objects**: Animations survive widget rebuilds
- **Flutter-Inspired**: Matches Flutter's animation API and patterns
- **Zero-Cost Abstractions**: Efficient trait-based design

## 📦 What's Included

### Core Types

- **`Animation<T>`** - Base trait for all animations
- **`AnimationController`** - Primary animation driver (0.0 → 1.0)
- **`CurvedAnimation`** - Apply easing curves to animations
- **`TweenAnimation<T>`** - Map f32 values to any type T
- **`ReverseAnimation`** - Invert an animation's values
- **`ProxyAnimation`** - Hot-swap animations at runtime
- **`CompoundAnimation`** - Combine animations with operators (+, *, min, max)

### Integration

- **`Ticker`** - Frame-based callback system (in `flui_core`)
- **`TickerProvider`** - Provides tickers to animation controllers
- Full integration with `flui_types::animation` (Curves, Tweens)

## 🚀 Quick Start

```rust
use flui_animation::prelude::*;
use flui_core::foundation::SimpleTickerProvider;
use std::sync::Arc;
use std::time::Duration;

// Create a ticker provider
let ticker_provider = Arc::new(SimpleTickerProvider);

// Create an animation controller
let controller = AnimationController::new(
    Duration::from_millis(300),
    ticker_provider,
);

// Apply an easing curve
let curved = CurvedAnimation::new(
    Arc::new(controller.clone()),
    Curves::EaseInOut,
);

// Map to a color
let tween = ColorTween::new(Color::RED, Color::BLUE);
let color_animation = TweenAnimation::new(tween, Arc::new(curved));

// Start the animation
controller.forward().unwrap();

// Get the current value
let color = color_animation.value();

// Clean up when done
controller.dispose();
```

## 📊 Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   flui_widgets                          │
│  AnimatedWidget, AnimatedBuilder, ImplicitAnimations    │
└──────────────────────┬──────────────────────────────────┘
                       │ uses
┌──────────────────────▼──────────────────────────────────┐
│                 flui_animation                          │
│  Animation<T>, AnimationController, CurvedAnimation     │
└──────────────────────┬──────────────────────────────────┘
                       │ uses
┌──────────────────────▼──────────────────────────────────┐
│             flui_types/animation                        │
│  Curve, Tween<T>, AnimationStatus (data only)          │
└──────────────────────┬──────────────────────────────────┘
                       │ uses
┌──────────────────────▼──────────────────────────────────┐
│            flui_core/foundation                         │
│  Ticker, TickerProvider, Listenable                     │
└─────────────────────────────────────────────────────────┘
```

## 🎯 Examples

### Basic Controller

```rust
let controller = AnimationController::new(
    Duration::from_millis(300),
    ticker_provider,
);

controller.forward().unwrap();  // Animate forward
controller.reverse().unwrap();  // Animate backward
controller.reset().unwrap();    // Reset to beginning
controller.stop().unwrap();     // Stop at current value
```

### Curved Animation

```rust
let curved = CurvedAnimation::new(
    Arc::new(controller),
    Curves::EaseInOut,
);

// With different reverse curve
let curved = CurvedAnimation::new(
    Arc::new(controller),
    Curves::EaseIn,
).with_reverse_curve(Curves::EaseOut);
```

### Tween Animation

```rust
// Float tween
let tween = FloatTween::new(0.0, 100.0);
let animation = TweenAnimation::new(tween, Arc::new(controller));

// Color tween
let tween = ColorTween::new(Color::RED, Color::BLUE);
let animation = TweenAnimation::new(tween, Arc::new(controller));

// Size tween
let tween = SizeTween::new(
    Size::new(100.0, 100.0),
    Size::new(200.0, 200.0),
);
let animation = TweenAnimation::new(tween, Arc::new(controller));
```

### Reverse Animation

```rust
let reversed = ReverseAnimation::new(Arc::new(controller));

controller.set_value(0.25);
assert_eq!(reversed.value(), 0.75);  // 1.0 - 0.25
```

### Compound Animation

```rust
// Addition
let compound = CompoundAnimation::add(
    Arc::new(controller1),
    Arc::new(controller2),
);

// Multiplication
let compound = CompoundAnimation::multiply(
    Arc::new(controller1),
    Arc::new(controller2),
);

// Min/Max
let compound = CompoundAnimation::min(
    Arc::new(controller1),
    Arc::new(controller2),
);
```

### Proxy Animation

```rust
let proxy = ProxyAnimation::new(Arc::new(controller1));

// Later, swap to a different animation
proxy.set_parent(Arc::new(controller2));
```

## 🧪 Testing

```bash
# Run all tests
cargo test -p flui_animation

# Run with output
cargo test -p flui_animation -- --nocapture

# Run examples
cargo run --example basic_animation
```

**Test Coverage**: 22 tests, all passing ✅

## 📚 Documentation

- [Animation Architecture](../../docs/arch/ANIMATION_ARCHITECTURE.md) - Detailed architecture document
- [API Guide](../../docs/API_GUIDE.md) - Comprehensive API reference
- [Examples](./examples/) - Working examples

Generate local documentation:

```bash
cargo doc -p flui_animation --open
```

## 🎨 Available Curves

From `flui_types::animation::Curves`:

- **Linear**: `Linear`
- **Ease**: `EaseIn`, `EaseOut`, `EaseInOut`
- **Fast/Slow**: `FastOutSlowIn`, `SlowOutFastIn`
- **Sine**: `EaseInSine`, `EaseOutSine`, `EaseInOutSine`
- **Expo**: `EaseInExpo`, `EaseOutExpo`, `EaseInOutExpo`
- **Circ**: `EaseInCirc`, `EaseOutCirc`, `EaseInOutCirc`
- **Back**: `EaseInBack`, `EaseOutBack`, `EaseInOutBack`
- **Elastic**: `ElasticIn`, `ElasticOut`, `ElasticInOut`

## 🔧 Available Tweens

From `flui_types::animation`:

- **Numeric**: `FloatTween`, `IntTween`, `StepTween`
- **Geometric**: `SizeTween`, `OffsetTween`, `RectTween`
- **Styling**: `ColorTween`, `BorderRadiusTween`, `EdgeInsetsTween`
- **Layout**: `AlignmentTween`
- **Special**: `ConstantTween`, `ReverseTween`, `TweenSequence`

## 🔐 Thread Safety

All animation types are fully thread-safe:

- Uses `Arc` for shared ownership
- Uses `parking_lot::Mutex` for interior mutability (2-3x faster than std)
- All callbacks are `Send + Sync`
- No `Rc` or `RefCell` - safe for multi-threaded UI

## ⚠️ Important Notes

### Disposing Controllers

**Always dispose animation controllers when done:**

```rust
let controller = AnimationController::new(duration, ticker_provider);

// Use the controller...

controller.dispose();  // ⚠️ Required to prevent leaks!
```

### Hook Integration

When using with hooks (future):

```rust
// In a widget's build method
let controller = use_animation_controller(ctx, duration);
// Automatically disposed when widget is unmounted
```

## 📈 Performance

- **~1,939 lines of code** (well-commented)
- **22 comprehensive tests**
- Zero runtime overhead for type erasure
- Efficient `Arc`-based sharing
- Lock-free where possible

## 🚦 Status

**✅ Phase 1 Complete**: Core animation infrastructure
- Animation trait system ✅
- AnimationController ✅
- Tween system ✅
- Curve system ✅
- Ticker system ✅

**⏳ Phase 2 In Progress**: Widget integration
- AnimatedWidget (future)
- Transition widgets (future)
- Implicit animations (future)

## 📝 License

MIT OR Apache-2.0
