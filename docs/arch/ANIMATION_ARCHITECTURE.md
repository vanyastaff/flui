# FLUI Animation Architecture

**Version:** 0.1.0
**Date:** 2025-11-10
**Author:** Claude (Anthropic)
**Status:** Design Proposal

---

## Executive Summary

This document defines the architecture for FLUI's animation system, based on Flutter's proven animation framework. The system follows the **persistent object pattern** with clear separation of concerns:

- **Persistent objects** (`Animation<T>`, `AnimationController`, `Tween<T>`) in `flui_animation` crate - Arc-based, extend Listenable
- **Animation widgets** (`AnimatedWidget`, `AnimatedBuilder`, implicit animated widgets) in `flui_widgets` - manage animation lifecycle
- **Ticker system** (`Ticker`, `TickerProvider`) in `flui_core/foundation` - provide frame callbacks

**Key Design Principles:**
1. **Type-safe animations**: Generic `Animation<T>` for any value type
2. **Composable**: Chain Tweens, apply Curves, merge multiple animations
3. **Listenable-based**: All animations implement Listenable for reactive updates
4. **Explicit vs Implicit**: Both fine-grained control (AnimatedWidget) and simple APIs (ImplicitlyAnimatedWidget)
5. **Efficient disposal**: All animation objects require proper cleanup

**Total Work Estimate:** ~2,000 LOC in animation crate + ~1,200 LOC in widgets

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Core Animation Types](#core-animation-types)
3. [Ticker System](#ticker-system)
4. [Tween System](#tween-system)
5. [Curve System](#curve-system)
6. [Animation Widgets](#animation-widgets)
7. [Implementation Plan](#implementation-plan)
8. [Usage Examples](#usage-examples)
9. [Testing Strategy](#testing-strategy)

---

## Architecture Overview

### Three-Layer Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                       flui_widgets                          │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  AnimatedWidget, AnimatedBuilder                     │   │
│  │  ImplicitlyAnimatedWidget (AnimatedContainer, etc.)  │   │
│  │  TweenAnimationBuilder                               │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                          ↓ uses
┌─────────────────────────────────────────────────────────────┐
│                     flui_animation                          │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Animation<T>, AnimationController                   │   │
│  │  Tween<T>, Curve, CurvedAnimation                    │   │
│  │  AnimationStatus, AnimationDirection                 │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                          ↓ uses
┌─────────────────────────────────────────────────────────────┐
│                  flui_core/foundation                       │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Ticker, TickerProvider, TickerCallback              │   │
│  │  ChangeNotifier, Listenable (already exists)         │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### Persistent Object Pattern

Following the same pattern as FocusNode, ScrollController, and OverlayEntry:

```rust
// Animation objects are PERSISTENT (like RenderObject)
let controller = AnimationController::new(
    duration: Duration::from_millis(300),
    vsync: ticker_provider,
);

// They survive widget rebuilds
let animation = Tween::new(0.0, 1.0)
    .animate(CurvedAnimation::new(
        parent: controller.clone(),
        curve: Curves::EASE_IN_OUT,
    ));

// Widgets manage their lifecycle
AnimatedBuilder::new(
    animation: animation.clone(),
    builder: move |ctx, child| {
        Opacity::new(animation.value(), child)
    },
)

// CRITICAL: Must dispose when done
controller.dispose();
```

---

## Core Animation Types

### 1. Animation\<T\> Trait (Base Trait)

The foundation of all animations - implements Listenable:

```rust
// In flui_animation/src/animation.rs

/// Represents a value that changes over time
///
/// All animations implement Listenable, allowing widgets to rebuild
/// when the animation value changes.
pub trait Animation<T>: Listenable + Send + Sync + fmt::Debug
where
    T: Clone + Send + Sync + 'static,
{
    /// Current value of the animation
    fn value(&self) -> T;

    /// Current status of the animation
    fn status(&self) -> AnimationStatus;

    /// Add a status listener (called when animation starts, completes, etc.)
    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId;

    /// Remove a status listener
    fn remove_status_listener(&self, id: ListenerId);

    /// Whether animation is currently running
    fn is_animating(&self) -> bool {
        matches!(self.status(), AnimationStatus::Forward | AnimationStatus::Reverse)
    }

    /// Whether animation is completed
    fn is_completed(&self) -> bool {
        self.status() == AnimationStatus::Completed
    }

    /// Whether animation is dismissed (at beginning)
    fn is_dismissed(&self) -> bool {
        self.status() == AnimationStatus::Dismissed
    }
}

/// Animation lifecycle status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationStatus {
    /// Animation is stopped at the beginning
    Dismissed,
    /// Animation is running forward
    Forward,
    /// Animation is running in reverse
    Reverse,
    /// Animation is stopped at the end
    Completed,
}

/// Callback for status changes
pub type StatusCallback = Arc<dyn Fn(AnimationStatus) + Send + Sync>;

/// Direction of animation playback
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationDirection {
    Forward,
    Reverse,
}
```

**Why Trait?** Multiple types need to be animations:
- `AnimationController` (generates values 0.0..1.0)
- `CurvedAnimation` (applies curves)
- `Tween<T>` (maps to any type T)
- `ProxyAnimation` (wraps another animation)

**Listenable Integration**: Uses existing `ChangeNotifier` infrastructure from `flui_core/foundation`.

### 2. AnimationController (Persistent Object)

The primary animation driver - extends `Animation<f64>`:

```rust
// In flui_animation/src/animation_controller.rs

/// Controls an animation, driving it forward/backward
///
/// AnimationController is a PERSISTENT OBJECT that survives widget rebuilds.
/// It must be disposed when no longer needed.
#[derive(Clone)]
pub struct AnimationController {
    inner: Arc<Mutex<AnimationControllerInner>>,
    notifier: Arc<ChangeNotifier>,
}

struct AnimationControllerInner {
    /// Current value (typically 0.0 to 1.0)
    value: f64,

    /// Animation status
    status: AnimationStatus,

    /// Duration of forward animation
    duration: Duration,

    /// Duration of reverse animation (defaults to duration)
    reverse_duration: Option<Duration>,

    /// Lower bound (default 0.0)
    lower_bound: f64,

    /// Upper bound (default 1.0)
    upper_bound: f64,

    /// Ticker for frame callbacks
    ticker: Option<Ticker>,

    /// Status listeners
    status_listeners: Vec<(ListenerId, StatusCallback)>,

    /// Animation direction
    direction: AnimationDirection,

    /// Start time of current animation
    start_time: Option<Instant>,

    /// Is disposed?
    disposed: bool,
}

impl AnimationController {
    /// Create a new animation controller
    pub fn new(
        duration: Duration,
        vsync: Arc<dyn TickerProvider>,
    ) -> Self {
        Self::with_bounds(duration, vsync, 0.0, 1.0)
    }

    /// Create with custom bounds
    pub fn with_bounds(
        duration: Duration,
        vsync: Arc<dyn TickerProvider>,
        lower_bound: f64,
        upper_bound: f64,
    ) -> Self {
        let notifier = Arc::new(ChangeNotifier::new());
        let ticker = vsync.create_ticker({
            let notifier = notifier.clone();
            Arc::new(move |elapsed| {
                notifier.notify_listeners();
            })
        });

        Self {
            inner: Arc::new(Mutex::new(AnimationControllerInner {
                value: lower_bound,
                status: AnimationStatus::Dismissed,
                duration,
                reverse_duration: None,
                lower_bound,
                upper_bound,
                ticker: Some(ticker),
                status_listeners: Vec::new(),
                direction: AnimationDirection::Forward,
                start_time: None,
                disposed: false,
            })),
            notifier,
        }
    }

    /// Start animation forward
    pub fn forward(&self) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        self.check_disposed(&inner)?;

        inner.direction = AnimationDirection::Forward;
        inner.status = AnimationStatus::Forward;
        inner.start_time = Some(Instant::now());

        if let Some(ticker) = &inner.ticker {
            ticker.start();
        }

        self.notify_status_listeners(AnimationStatus::Forward, &inner);
        Ok(())
    }

    /// Start animation in reverse
    pub fn reverse(&self) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        self.check_disposed(&inner)?;

        inner.direction = AnimationDirection::Reverse;
        inner.status = AnimationStatus::Reverse;
        inner.start_time = Some(Instant::now());

        if let Some(ticker) = &inner.ticker {
            ticker.start();
        }

        self.notify_status_listeners(AnimationStatus::Reverse, &inner);
        Ok(())
    }

    /// Stop animation
    pub fn stop(&self) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        self.check_disposed(&inner)?;

        if let Some(ticker) = &inner.ticker {
            ticker.stop();
        }

        inner.status = if inner.value >= inner.upper_bound {
            AnimationStatus::Completed
        } else if inner.value <= inner.lower_bound {
            AnimationStatus::Dismissed
        } else {
            // Stopped in middle, keep previous status
            inner.status
        };

        Ok(())
    }

    /// Reset to beginning
    pub fn reset(&self) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        self.check_disposed(&inner)?;

        inner.value = inner.lower_bound;
        inner.status = AnimationStatus::Dismissed;

        if let Some(ticker) = &inner.ticker {
            ticker.stop();
        }

        self.notifier.notify_listeners();
        self.notify_status_listeners(AnimationStatus::Dismissed, &inner);
        Ok(())
    }

    /// Animate to specific value
    pub fn animate_to(&self, target: f64) -> Result<(), AnimationError> {
        // Implementation similar to forward/reverse but with custom target
        todo!()
    }

    /// Repeat animation forever
    pub fn repeat(&self, reverse: bool) -> Result<(), AnimationError> {
        // Implementation that restarts on completion
        todo!()
    }

    /// CRITICAL: Dispose when done to prevent leaks
    pub fn dispose(&self) {
        let mut inner = self.inner.lock();

        if inner.disposed {
            return;
        }

        if let Some(ticker) = inner.ticker.take() {
            ticker.dispose();
        }

        inner.status_listeners.clear();
        inner.disposed = true;
    }

    fn check_disposed(&self, inner: &AnimationControllerInner) -> Result<(), AnimationError> {
        if inner.disposed {
            Err(AnimationError::Disposed)
        } else {
            Ok(())
        }
    }

    fn notify_status_listeners(&self, status: AnimationStatus, inner: &AnimationControllerInner) {
        for (_, callback) in &inner.status_listeners {
            callback(status);
        }
    }
}

impl Animation<f64> for AnimationController {
    fn value(&self) -> f64 {
        self.inner.lock().value
    }

    fn status(&self) -> AnimationStatus {
        self.inner.lock().status
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        let mut inner = self.inner.lock();
        let id = ListenerId::new();
        inner.status_listeners.push((id, callback));
        id
    }

    fn remove_status_listener(&self, id: ListenerId) {
        let mut inner = self.inner.lock();
        inner.status_listeners.retain(|(listener_id, _)| *listener_id != id);
    }
}

impl Listenable for AnimationController {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AnimationError {
    #[error("AnimationController has been disposed")]
    Disposed,

    #[error("Invalid animation bounds: {0}")]
    InvalidBounds(String),
}
```

**Key Properties:**
- **Persistent Object**: Survives widget rebuilds
- **Listenable**: Implements both Animation\<f64\> and Listenable
- **Ticker-driven**: Uses Ticker for frame callbacks
- **Must Dispose**: Required cleanup via `dispose()`

---

## Ticker System

### In flui_core/foundation/ticker.rs

```rust
// In flui_core/src/foundation/ticker.rs

/// Provides frame callbacks for animations
///
/// A Ticker ticks once per frame, calling a callback with the elapsed time.
#[derive(Clone)]
pub struct Ticker {
    inner: Arc<Mutex<TickerInner>>,
}

struct TickerInner {
    callback: TickerCallback,
    is_active: bool,
    start_time: Option<Instant>,
}

pub type TickerCallback = Arc<dyn Fn(Duration) + Send + Sync>;

impl Ticker {
    pub fn new(callback: TickerCallback) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TickerInner {
                callback,
                is_active: false,
                start_time: None,
            })),
        }
    }

    /// Start ticking
    pub fn start(&self) {
        let mut inner = self.inner.lock();
        inner.is_active = true;
        inner.start_time = Some(Instant::now());
    }

    /// Stop ticking
    pub fn stop(&self) {
        let mut inner = self.inner.lock();
        inner.is_active = false;
    }

    /// Called each frame by the scheduler
    pub fn tick(&self, now: Instant) {
        let inner = self.inner.lock();

        if !inner.is_active {
            return;
        }

        if let Some(start) = inner.start_time {
            let elapsed = now.duration_since(start);
            (inner.callback)(elapsed);
        }
    }

    pub fn is_active(&self) -> bool {
        self.inner.lock().is_active
    }

    pub fn dispose(&self) {
        self.stop();
    }
}

/// Provides Ticker instances
///
/// Typically implemented by State objects that need animations.
pub trait TickerProvider: Send + Sync {
    /// Create a new ticker with the given callback
    fn create_ticker(&self, callback: TickerCallback) -> Ticker;
}

/// Single ticker provider (for single animation)
pub trait SingleTickerProviderMixin: TickerProvider {
    // Ensures only one ticker is created
}

/// Multiple ticker provider (for multiple animations)
pub trait TickerProviderMixin: TickerProvider {
    // Allows multiple tickers
}
```

**Integration with FrameScheduler:**

The frame scheduler (in `flui_core`) needs to tick all active tickers:

```rust
// In flui_core/src/pipeline/frame_scheduler.rs

impl FrameScheduler {
    pub fn schedule_frame(&self) {
        // ... existing frame logic ...

        // Tick all active tickers
        let now = Instant::now();
        for ticker in &self.active_tickers {
            ticker.tick(now);
        }
    }

    pub fn register_ticker(&self, ticker: Ticker) {
        // Add to active_tickers list
    }

    pub fn unregister_ticker(&self, ticker: &Ticker) {
        // Remove from active_tickers list
    }
}
```

---

## Tween System

### Tween\<T\> - Type-Safe Value Mapping

```rust
// In flui_animation/src/tween.rs

/// Maps animation values to typed values
///
/// A Tween defines a mapping from a double (0.0 to 1.0) to a value of type T.
///
/// Tweens are mutable, but can be stored as `static final` if values never change.
#[derive(Debug, Clone)]
pub struct Tween<T> {
    /// Begin value
    pub begin: T,

    /// End value
    pub end: T,

    _marker: PhantomData<T>,
}

impl<T> Tween<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn new(begin: T, end: T) -> Self {
        Self {
            begin,
            end,
            _marker: PhantomData,
        }
    }

    /// Create an Animation<T> from Animation<f64>
    pub fn animate(&self, parent: Arc<dyn Animation<f64>>) -> TweenAnimation<T> {
        TweenAnimation::new(self.clone(), parent)
    }
}

/// Trait for types that can be interpolated
pub trait Lerp: Clone + Send + Sync + 'static {
    /// Linear interpolation between self and other
    fn lerp(&self, other: &Self, t: f64) -> Self;
}

impl Lerp for f64 {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        self + (other - self) * t
    }
}

impl Lerp for Size {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        Size::new(
            self.width.lerp(&other.width, t),
            self.height.lerp(&other.height, t),
        )
    }
}

impl Lerp for Color {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        // RGBA interpolation
        Color::from_rgba(
            (self.r() as f64).lerp(&(other.r() as f64), t) as u8,
            (self.g() as f64).lerp(&(other.g() as f64), t) as u8,
            (self.b() as f64).lerp(&(other.b() as f64), t) as u8,
            (self.a() as f64).lerp(&(other.a() as f64), t) as u8,
        )
    }
}

// Add more Lerp implementations for Offset, Rect, EdgeInsets, etc.

/// Animation that applies a Tween to a parent animation
#[derive(Clone)]
pub struct TweenAnimation<T> {
    tween: Tween<T>,
    parent: Arc<dyn Animation<f64>>,
    notifier: Arc<ChangeNotifier>,
}

impl<T> TweenAnimation<T>
where
    T: Lerp,
{
    pub fn new(tween: Tween<T>, parent: Arc<dyn Animation<f64>>) -> Self {
        let notifier = Arc::new(ChangeNotifier::new());

        // Forward parent's notifications
        let notifier_clone = notifier.clone();
        parent.add_listener(Arc::new(move || {
            notifier_clone.notify_listeners();
        }));

        Self {
            tween,
            parent,
            notifier,
        }
    }
}

impl<T> Animation<T> for TweenAnimation<T>
where
    T: Lerp,
{
    fn value(&self) -> T {
        let t = self.parent.value();
        self.tween.begin.lerp(&self.tween.end, t)
    }

    fn status(&self) -> AnimationStatus {
        self.parent.status()
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        self.parent.add_status_listener(callback)
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.parent.remove_status_listener(id)
    }
}

impl<T> Listenable for TweenAnimation<T>
where
    T: Lerp,
{
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id)
    }
}
```

**Common Tween Types:**

```rust
// Convenience constructors
impl Tween<f64> {
    pub fn float(begin: f64, end: f64) -> Self {
        Self::new(begin, end)
    }
}

impl Tween<Color> {
    pub fn color(begin: Color, end: Color) -> Self {
        Self::new(begin, end)
    }
}

impl Tween<Size> {
    pub fn size(begin: Size, end: Size) -> Self {
        Self::new(begin, end)
    }
}

impl Tween<Offset> {
    pub fn offset(begin: Offset, end: Offset) -> Self {
        Self::new(begin, end)
    }
}
```

---

## Curve System

### Curves - Non-Linear Interpolation

```rust
// In flui_animation/src/curve.rs

/// Defines the rate of change of an animation over time
pub trait Curve: Send + Sync + fmt::Debug {
    /// Transform a value from 0.0-1.0 to a curved value
    fn transform(&self, t: f64) -> f64;

    /// Optional: The curve for the reverse direction
    fn flipped(&self) -> Arc<dyn Curve> {
        Arc::new(FlippedCurve::new(Arc::new(self.clone())))
    }
}

/// Common easing curves
pub struct Curves;

impl Curves {
    pub const LINEAR: Linear = Linear;
    pub const EASE_IN: CubicBezier = CubicBezier::new(0.42, 0.0, 1.0, 1.0);
    pub const EASE_OUT: CubicBezier = CubicBezier::new(0.0, 0.0, 0.58, 1.0);
    pub const EASE_IN_OUT: CubicBezier = CubicBezier::new(0.42, 0.0, 0.58, 1.0);
    pub const FAST_OUT_SLOW_IN: CubicBezier = CubicBezier::new(0.4, 0.0, 0.2, 1.0);
    pub const BOUNCE_IN: BounceInCurve = BounceInCurve;
    pub const BOUNCE_OUT: BounceOutCurve = BounceOutCurve;
    pub const ELASTIC_IN: ElasticInCurve = ElasticInCurve::new(0.4);
    pub const ELASTIC_OUT: ElasticOutCurve = ElasticOutCurve::new(0.4);
}

/// Linear curve (no easing)
#[derive(Debug, Clone, Copy)]
pub struct Linear;

impl Curve for Linear {
    fn transform(&self, t: f64) -> f64 {
        t
    }
}

/// Cubic bezier curve
#[derive(Debug, Clone, Copy)]
pub struct CubicBezier {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl CubicBezier {
    pub const fn new(a: f64, b: f64, c: f64, d: f64) -> Self {
        Self { a, b, c, d }
    }
}

impl Curve for CubicBezier {
    fn transform(&self, t: f64) -> f64 {
        // Cubic bezier math
        let t2 = t * t;
        let t3 = t2 * t;
        3.0 * self.a * t + 3.0 * self.b * t2 + self.c * t3
    }
}

/// Bounce curve
#[derive(Debug, Clone, Copy)]
pub struct BounceOutCurve;

impl Curve for BounceOutCurve {
    fn transform(&self, t: f64) -> f64 {
        if t < 1.0 / 2.75 {
            7.5625 * t * t
        } else if t < 2.0 / 2.75 {
            let t = t - 1.5 / 2.75;
            7.5625 * t * t + 0.75
        } else if t < 2.5 / 2.75 {
            let t = t - 2.25 / 2.75;
            7.5625 * t * t + 0.9375
        } else {
            let t = t - 2.625 / 2.75;
            7.5625 * t * t + 0.984375
        }
    }
}

// Add more curve implementations...

/// Animation with curve applied
#[derive(Clone)]
pub struct CurvedAnimation {
    parent: Arc<dyn Animation<f64>>,
    curve: Arc<dyn Curve>,
    reverse_curve: Option<Arc<dyn Curve>>,
    notifier: Arc<ChangeNotifier>,
}

impl CurvedAnimation {
    pub fn new(parent: Arc<dyn Animation<f64>>, curve: Arc<dyn Curve>) -> Self {
        let notifier = Arc::new(ChangeNotifier::new());

        // Forward parent's notifications
        let notifier_clone = notifier.clone();
        parent.add_listener(Arc::new(move || {
            notifier_clone.notify_listeners();
        }));

        Self {
            parent,
            curve,
            reverse_curve: None,
            notifier,
        }
    }

    pub fn with_reverse_curve(mut self, reverse_curve: Arc<dyn Curve>) -> Self {
        self.reverse_curve = Some(reverse_curve);
        self
    }
}

impl Animation<f64> for CurvedAnimation {
    fn value(&self) -> f64 {
        let t = self.parent.value();

        // Apply curve based on direction
        let curve = match self.parent.status() {
            AnimationStatus::Reverse => {
                self.reverse_curve.as_ref().unwrap_or(&self.curve)
            }
            _ => &self.curve,
        };

        curve.transform(t)
    }

    fn status(&self) -> AnimationStatus {
        self.parent.status()
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        self.parent.add_status_listener(callback)
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.parent.remove_status_listener(id)
    }
}

impl Listenable for CurvedAnimation {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id)
    }
}
```

---

## Animation Widgets

### 1. AnimatedWidget (Explicit Animations)

```rust
// In flui_widgets/src/animated/animated_widget.rs

/// Base class for widgets that rebuild when an animation changes
///
/// AnimatedWidget requires explicit AnimationController management.
/// Subclasses should implement `build_with_child` to create their widget tree.
pub trait AnimatedWidget: View {
    /// The animation this widget listens to
    fn listenable(&self) -> Arc<dyn Listenable>;
}

// Example: FadeTransition
#[derive(Debug)]
pub struct FadeTransition {
    opacity: Arc<dyn Animation<f64>>,
    child: Option<AnyElement>,
}

impl FadeTransition {
    pub fn new(opacity: Arc<dyn Animation<f64>>, child: Option<AnyElement>) -> Self {
        Self { opacity, child }
    }
}

impl View for FadeTransition {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Subscribe to animation changes
        let opacity = self.opacity.clone();
        ctx.subscribe_listenable(self.opacity.clone());

        Opacity::new(opacity.value(), self.child)
    }
}

impl AnimatedWidget for FadeTransition {
    fn listenable(&self) -> Arc<dyn Listenable> {
        self.opacity.clone()
    }
}
```

**Other Transition Widgets:**

```rust
pub struct ScaleTransition {
    scale: Arc<dyn Animation<f64>>,
    alignment: Alignment,
    child: Option<AnyElement>,
}

pub struct SlideTransition {
    position: Arc<dyn Animation<Offset>>,
    child: Option<AnyElement>,
}

pub struct RotationTransition {
    turns: Arc<dyn Animation<f64>>,
    alignment: Alignment,
    child: Option<AnyElement>,
}

pub struct SizeTransition {
    size_factor: Arc<dyn Animation<f64>>,
    axis: Axis,
    child: Option<AnyElement>,
}
```

### 2. AnimatedBuilder (Generic Builder)

```rust
// In flui_widgets/src/animated/animated_builder.rs

/// Builds a widget tree based on an animation's current value
///
/// More flexible than AnimatedWidget - you provide the builder function.
#[derive(Debug)]
pub struct AnimatedBuilder<F> {
    animation: Arc<dyn Listenable>,
    builder: Arc<F>,
    child: Option<AnyElement>,
}

impl<F> AnimatedBuilder<F>
where
    F: Fn(&BuildContext, Option<AnyElement>) -> AnyElement + Send + Sync + 'static,
{
    pub fn new(
        animation: Arc<dyn Listenable>,
        builder: F,
    ) -> Self {
        Self {
            animation,
            builder: Arc::new(builder),
            child: None,
        }
    }

    pub fn child(mut self, child: AnyElement) -> Self {
        self.child = Some(child);
        self
    }
}

impl<F> View for AnimatedBuilder<F>
where
    F: Fn(&BuildContext, Option<AnyElement>) -> AnyElement + Send + Sync + 'static,
{
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Subscribe to animation
        ctx.subscribe_listenable(self.animation.clone());

        // Build with current animation value
        (self.builder)(ctx, self.child)
    }
}
```

**Usage:**

```rust
AnimatedBuilder::new(
    animation.clone(),
    move |ctx, child| {
        Transform::rotate(
            animation.value() * 2.0 * PI,
            child,
        ).into()
    },
)
.child(Icon::new("rotate"))
```

### 3. ImplicitlyAnimatedWidget (Simple Animations)

```rust
// In flui_widgets/src/animated/implicitly_animated_widget.rs

/// Base for widgets that implicitly animate property changes
///
/// ImplicitlyAnimatedWidget creates and manages its own AnimationController internally.
pub trait ImplicitlyAnimatedWidget: View {
    /// Duration of the animation
    fn duration(&self) -> Duration;

    /// Animation curve
    fn curve(&self) -> Arc<dyn Curve>;
}

/// State for implicitly animated widgets
pub struct ImplicitlyAnimatedWidgetState<T>
where
    T: Lerp,
{
    controller: Option<AnimationController>,
    animation: Option<Arc<TweenAnimation<T>>>,
    current_value: T,
}

// Example: AnimatedContainer
#[derive(Debug)]
pub struct AnimatedContainer {
    width: Option<f64>,
    height: Option<f64>,
    color: Option<Color>,
    padding: Option<EdgeInsets>,
    duration: Duration,
    curve: Arc<dyn Curve>,
    child: Option<AnyElement>,
}

impl AnimatedContainer {
    pub fn new(duration: Duration) -> Self {
        Self {
            width: None,
            height: None,
            color: None,
            padding: None,
            duration,
            curve: Arc::new(Curves::LINEAR),
            child: None,
        }
    }

    pub fn width(mut self, width: f64) -> Self {
        self.width = Some(width);
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn curve(mut self, curve: Arc<dyn Curve>) -> Self {
        self.curve = curve;
        self
    }
}

impl View for AnimatedContainer {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Get or create animation controller
        let controller = use_animation_controller(ctx, self.duration);

        // Create tweens for each property
        let width_anim = self.width.map(|target| {
            let current = ctx.get_previous_value("width").unwrap_or(target);
            Tween::new(current, target).animate(controller.clone())
        });

        let color_anim = self.color.map(|target| {
            let current = ctx.get_previous_value("color").unwrap_or(target);
            Tween::new(current, target).animate(controller.clone())
        });

        // Start animation
        let _ = controller.forward();

        // Build container with animated values
        Container::new()
            .width(width_anim.as_ref().map(|a| a.value()))
            .color(color_anim.as_ref().map(|a| a.value()))
            .child(self.child)
    }
}
```

**Other Implicit Animated Widgets:**

```rust
pub struct AnimatedOpacity {
    opacity: f64,
    duration: Duration,
    curve: Arc<dyn Curve>,
    child: Option<AnyElement>,
}

pub struct AnimatedAlign {
    alignment: Alignment,
    duration: Duration,
    curve: Arc<dyn Curve>,
    child: Option<AnyElement>,
}

pub struct AnimatedPadding {
    padding: EdgeInsets,
    duration: Duration,
    curve: Arc<dyn Curve>,
    child: Option<AnyElement>,
}

pub struct AnimatedPositioned {
    left: Option<f64>,
    top: Option<f64>,
    right: Option<f64>,
    bottom: Option<f64>,
    duration: Duration,
    curve: Arc<dyn Curve>,
    child: AnyElement,
}
```

### 4. TweenAnimationBuilder (Generic Implicit Animation)

```rust
// In flui_widgets/src/animated/tween_animation_builder.rs

/// Generic builder for implicitly animating any value
///
/// More flexible than specific implicit animated widgets.
#[derive(Debug)]
pub struct TweenAnimationBuilder<T, F>
where
    T: Lerp,
    F: Fn(&BuildContext, T, Option<AnyElement>) -> AnyElement + Send + Sync,
{
    tween: Tween<T>,
    duration: Duration,
    curve: Arc<dyn Curve>,
    builder: Arc<F>,
    child: Option<AnyElement>,
}

impl<T, F> TweenAnimationBuilder<T, F>
where
    T: Lerp,
    F: Fn(&BuildContext, T, Option<AnyElement>) -> AnyElement + Send + Sync + 'static,
{
    pub fn new(tween: Tween<T>, duration: Duration, builder: F) -> Self {
        Self {
            tween,
            duration,
            curve: Arc::new(Curves::LINEAR),
            builder: Arc::new(builder),
            child: None,
        }
    }

    pub fn curve(mut self, curve: Arc<dyn Curve>) -> Self {
        self.curve = curve;
        self
    }

    pub fn child(mut self, child: AnyElement) -> Self {
        self.child = Some(child);
        self
    }
}

impl<T, F> View for TweenAnimationBuilder<T, F>
where
    T: Lerp,
    F: Fn(&BuildContext, T, Option<AnyElement>) -> AnyElement + Send + Sync + 'static,
{
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let controller = use_animation_controller(ctx, self.duration);
        let animation = self.tween.animate(controller.clone());

        let _ = controller.forward();

        (self.builder)(ctx, animation.value(), self.child)
    }
}
```

---

## Implementation Plan

### Phase 1: Core Animation Infrastructure (~800 LOC)

**Location:** `crates/flui_animation/src/`

1. **animation.rs** (~150 LOC)
   - `Animation<T>` trait
   - `AnimationStatus` enum
   - `AnimationDirection` enum
   - Status callback types

2. **animation_controller.rs** (~250 LOC)
   - `AnimationController` struct
   - Implementation of `Animation<f64>`
   - Methods: `forward()`, `reverse()`, `stop()`, `reset()`, `dispose()`
   - Ticker integration

3. **tween.rs** (~200 LOC)
   - `Tween<T>` struct
   - `Lerp` trait
   - `TweenAnimation<T>` wrapper
   - Lerp implementations for common types (f64, Size, Color, Offset, Rect, EdgeInsets)

4. **curve.rs** (~200 LOC)
   - `Curve` trait
   - `Curves` constants
   - Common curve implementations (Linear, CubicBezier, Bounce, Elastic, etc.)
   - `CurvedAnimation` wrapper

**Total Phase 1:** ~800 LOC

### Phase 2: Ticker System (~200 LOC)

**Location:** `crates/flui_core/src/foundation/`

5. **ticker.rs** (~150 LOC)
   - `Ticker` struct
   - `TickerProvider` trait
   - `SingleTickerProviderMixin` trait
   - `TickerProviderMixin` trait

6. **frame_scheduler.rs** (modifications ~50 LOC)
   - Register/unregister tickers
   - Tick all active tickers each frame

**Total Phase 2:** ~200 LOC

### Phase 3: Animation Widgets (~1,200 LOC)

**Location:** `crates/flui_widgets/src/animated/`

7. **animated_widget.rs** (~100 LOC)
   - `AnimatedWidget` trait
   - Listenable subscription in BuildContext

8. **transitions.rs** (~300 LOC)
   - `FadeTransition`
   - `ScaleTransition`
   - `SlideTransition`
   - `RotationTransition`
   - `SizeTransition`
   - `DecoratedBoxTransition`

9. **animated_builder.rs** (~100 LOC)
   - `AnimatedBuilder` widget

10. **implicitly_animated_widget.rs** (~200 LOC)
    - `ImplicitlyAnimatedWidget` trait
    - `ImplicitlyAnimatedWidgetState` helper
    - Hook: `use_animation_controller()`

11. **implicit_animations.rs** (~400 LOC)
    - `AnimatedContainer`
    - `AnimatedOpacity`
    - `AnimatedAlign`
    - `AnimatedPadding`
    - `AnimatedPositioned`
    - `AnimatedDefaultTextStyle`

12. **tween_animation_builder.rs** (~100 LOC)
    - `TweenAnimationBuilder` widget

**Total Phase 3:** ~1,200 LOC

### Phase 4: Testing & Documentation (~400 LOC)

13. **tests/** (~300 LOC)
    - Animation lifecycle tests
    - Tween interpolation tests
    - Curve transformation tests
    - Widget rebuild tests

14. **examples/** (~100 LOC)
    - Basic animation example
    - Multiple animations example
    - Implicit animations example
    - Custom transition example

**Total Phase 4:** ~400 LOC

---

## Usage Examples

### Example 1: Explicit Animation with AnimationController

```rust
use flui_animation::*;
use flui_widgets::*;

#[derive(Debug)]
struct FadeInDemo {
    ticker_provider: Arc<dyn TickerProvider>,
}

impl View for FadeInDemo {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create controller (persists across rebuilds via hook)
        let controller = use_animation_controller(
            ctx,
            Duration::from_millis(500),
            self.ticker_provider,
        );

        // Create opacity animation
        let animation = Tween::new(0.0, 1.0)
            .animate(CurvedAnimation::new(
                controller.clone(),
                Arc::new(Curves::EASE_IN),
            ));

        // Start on first build
        use_effect(ctx, {
            let controller = controller.clone();
            move || {
                let _ = controller.forward();
                None
            }
        });

        // Use FadeTransition widget
        FadeTransition::new(
            animation,
            Some(Box::new(Text::new("Hello, FLUI!"))),
        )
    }
}
```

### Example 2: AnimatedBuilder for Custom Animation

```rust
use flui_animation::*;
use flui_widgets::*;

#[derive(Debug)]
struct SpinningBox {
    ticker_provider: Arc<dyn TickerProvider>,
}

impl View for SpinningBox {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let controller = use_animation_controller(
            ctx,
            Duration::from_secs(2),
            self.ticker_provider,
        );

        // Repeat forever
        use_effect(ctx, {
            let controller = controller.clone();
            move || {
                let _ = controller.repeat(false);
                Some(Box::new(move || {
                    controller.dispose();
                }))
            }
        });

        AnimatedBuilder::new(
            controller.clone(),
            move |ctx, child| {
                Transform::rotate(
                    controller.value() * 2.0 * std::f64::consts::PI,
                    child,
                ).into()
            },
        )
        .child(Box::new(Container::new()
            .width(100.0)
            .height(100.0)
            .color(Color::BLUE)))
    }
}
```

### Example 3: Implicit Animation with AnimatedContainer

```rust
use flui_animation::*;
use flui_widgets::*;

#[derive(Debug)]
struct GrowingBox;

impl View for GrowingBox {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let is_large = use_signal(ctx, false);

        Column::new()
            .children(vec![
                Box::new(AnimatedContainer::new(Duration::from_millis(300))
                    .width(if is_large.get() { 200.0 } else { 100.0 })
                    .height(if is_large.get() { 200.0 } else { 100.0 })
                    .color(if is_large.get() { Color::RED } else { Color::BLUE })
                    .curve(Arc::new(Curves::EASE_IN_OUT))),

                Box::new(Button::new("Toggle Size")
                    .on_pressed({
                        let is_large = is_large.clone();
                        move || is_large.update(|v| *v = !*v)
                    })),
            ])
    }
}
```

### Example 4: TweenAnimationBuilder for Custom Type

```rust
use flui_animation::*;
use flui_widgets::*;

#[derive(Debug)]
struct CounterAnimation;

impl View for CounterAnimation {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        TweenAnimationBuilder::new(
            Tween::new(0, 100),
            Duration::from_secs(2),
            |ctx, value, child| {
                Text::new(format!("Count: {}", value)).into()
            },
        )
        .curve(Arc::new(Curves::EASE_OUT))
    }
}
```

### Example 5: Multiple Animations with Listenable.merge

```rust
use flui_animation::*;
use flui_widgets::*;

#[derive(Debug)]
struct MultiAnimation {
    ticker_provider: Arc<dyn TickerProvider>,
}

impl View for MultiAnimation {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let controller1 = use_animation_controller(
            ctx,
            Duration::from_millis(500),
            self.ticker_provider.clone(),
        );

        let controller2 = use_animation_controller(
            ctx,
            Duration::from_millis(700),
            self.ticker_provider,
        );

        // Merge both animations
        let merged = Listenable::merge(vec![
            controller1.clone(),
            controller2.clone(),
        ]);

        AnimatedBuilder::new(
            merged,
            move |ctx, child| {
                Transform::translate(
                    Offset::new(controller1.value() * 100.0, 0.0),
                    Some(Box::new(
                        Transform::scale(
                            controller2.value(),
                            child,
                        )
                    )),
                ).into()
            },
        )
    }
}
```

---

## Testing Strategy

### Unit Tests

1. **Animation Lifecycle:**
   - Test status transitions (Dismissed → Forward → Completed)
   - Test reverse playback
   - Test repeat functionality
   - Test dispose behavior

2. **Tween Interpolation:**
   - Test linear interpolation
   - Test color interpolation (RGBA)
   - Test size interpolation
   - Test offset interpolation
   - Test custom Lerp implementations

3. **Curve Transformations:**
   - Test linear curve (identity)
   - Test cubic bezier curves
   - Test bounce curves
   - Test elastic curves
   - Test flipped curves

4. **Widget Rebuilds:**
   - Test AnimatedWidget rebuilds on animation change
   - Test AnimatedBuilder rebuilds
   - Test ImplicitlyAnimatedWidget creates controller
   - Test disposal on widget removal

### Integration Tests

1. **Full Animation Pipeline:**
   - Create controller → Apply tween → Apply curve → Use in widget
   - Verify frame callbacks trigger
   - Verify listeners notified

2. **Multiple Animations:**
   - Test Listenable.merge
   - Test multiple controllers in same widget
   - Test staggered animations

3. **Performance:**
   - Benchmark animation overhead
   - Test 100+ simultaneous animations
   - Measure frame time impact

---

## Crate Dependencies

```toml
# crates/flui_animation/Cargo.toml

[package]
name = "flui_animation"
version = "0.1.0"
edition = "2021"

[dependencies]
flui_core = { path = "../flui_core" }
flui_types = { path = "../flui_types" }
parking_lot = "0.12"
thiserror = "1.0"

[dev-dependencies]
tokio = { version = "1.43", features = ["full"] }
```

```toml
# crates/flui_widgets/Cargo.toml (add animation dependency)

[dependencies]
flui_animation = { path = "../flui_animation" }
# ... existing dependencies ...
```

---

## Migration from Existing Code

If FLUI already has animation code in `flui_types`, migrate to new architecture:

### Before (flui_types)

```rust
// Old: Animation primitives in types crate
use flui_types::animation::{Curve, Tween};
```

### After (flui_animation)

```rust
// New: Persistent animation objects in dedicated crate
use flui_animation::{AnimationController, Tween, Curve};
use flui_widgets::{AnimatedBuilder, FadeTransition};
```

**Migration Steps:**
1. Move `Curve` and `Tween` traits from `flui_types` to `flui_animation`
2. Keep simple curve implementations (Linear, CubicBezier) in `flui_types` for backwards compatibility
3. Add new persistent objects (`AnimationController`, `CurvedAnimation`) in `flui_animation`
4. Add animation widgets in `flui_widgets`
5. Update existing code to use new APIs

---

## Open Questions

1. **TickerProvider Implementation:**
   - Should State automatically implement TickerProvider?
   - How do we ensure ticker disposal on widget removal?
   - Should we track all tickers globally for frame callbacks?

2. **Performance Optimization:**
   - Should we pool AnimationController instances?
   - Should we batch animation updates to minimize rebuilds?
   - Should we use a separate animation thread?

3. **Web Support:**
   - How do we integrate with `requestAnimationFrame` on web?
   - Should we use `wasm-timer` for web ticker?

4. **Advanced Features:**
   - Should we support animation sequences (TweenSequence)?
   - Should we support animation tracks (AnimationTrack)?
   - Should we support staggered animations (StaggeredAnimation)?

---

## Version History

| Version | Date       | Author | Changes                          |
|---------|------------|--------|----------------------------------|
| 0.1.0   | 2025-11-10 | Claude | Initial animation architecture   |

---

## References

- [Flutter Animation Documentation](https://docs.flutter.dev/ui/animations)
- [Flutter AnimationController API](https://api.flutter.dev/flutter/animation/AnimationController-class.html)
- [Flutter Tween API](https://api.flutter.dev/flutter/animation/Tween-class.html)
- [Flutter Curve API](https://api.flutter.dev/flutter/animation/Curve-class.html)
- [Flutter AnimatedWidget API](https://api.flutter.dev/flutter/widgets/AnimatedWidget-class.html)
- [Flutter ImplicitlyAnimatedWidget API](https://api.flutter.dev/flutter/widgets/ImplicitlyAnimatedWidget-class.html)

---

## Conclusion

This architecture provides a **complete, Flutter-accurate animation system** for FLUI:

✅ **Persistent objects** (AnimationController, Tween, Curve) in `flui_animation`
✅ **Lifecycle widgets** (AnimatedWidget, AnimatedBuilder, implicit widgets) in `flui_widgets`
✅ **Ticker infrastructure** in `flui_core/foundation`
✅ **Type-safe** with generic Animation\<T\> trait
✅ **Composable** with tween chaining and curve application
✅ **Efficient** with Listenable-based reactive updates
✅ **Complete** with both explicit and implicit animation APIs

**Estimated Total Work:** ~2,600 LOC (800 core + 200 ticker + 1,200 widgets + 400 tests/examples)

This should provide a solid foundation for FLUI's animation system! 🎨
