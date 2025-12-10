# AnimatedWidget and Explicit Animations

This document analyzes Flutter's AnimatedWidget system - widgets that rebuild in response to Listenable/Animation changes, providing explicit control over animations.

## Source Files
- `packages/flutter/lib/src/widgets/transitions.dart` (AnimatedWidget, transitions)
- `packages/flutter/lib/src/animation/animation.dart` (Animation base)
- `packages/flutter/lib/src/animation/animation_controller.dart` (AnimationController)
- `packages/flutter/lib/src/animation/tween.dart` (Tween, Animatable)

## Two Animation Approaches in Flutter

| Aspect | ImplicitlyAnimatedWidget | AnimatedWidget |
|--------|-------------------------|----------------|
| Control | Automatic | Manual |
| AnimationController | Internal (managed) | External (you manage) |
| Naming | `AnimatedFoo` | `FooTransition` |
| Use case | Simple "animate to value" | Complex, coordinated animations |
| Examples | `AnimatedContainer`, `AnimatedOpacity` | `FadeTransition`, `SlideTransition` |

---

## Core Architecture

### Animation<T>

```dart
/// A value which might change over time, moving forward or backward.
abstract class Animation<T> extends Listenable implements ValueListenable<T> {
  const Animation();

  /// Current value of the animation
  @override
  T get value;

  /// Current status (dismissed, forward, reverse, completed)
  AnimationStatus get status;

  /// Listeners for value changes
  void addListener(VoidCallback listener);
  void removeListener(VoidCallback listener);

  /// Listeners for status changes
  void addStatusListener(AnimationStatusListener listener);
  void removeStatusListener(AnimationStatusListener listener);

  /// Convenience getters
  bool get isDismissed => status.isDismissed;
  bool get isCompleted => status.isCompleted;
  bool get isAnimating => status.isAnimating;

  /// Chain a Tween to this animation
  Animation<U> drive<U>(Animatable<U> child);
}
```

### AnimationStatus

```dart
enum AnimationStatus {
  dismissed,  // Stopped at beginning (value = 0.0)
  forward,    // Running from beginning to end
  reverse,    // Running from end to beginning  
  completed;  // Stopped at end (value = 1.0)

  bool get isDismissed => this == dismissed;
  bool get isCompleted => this == completed;
  bool get isAnimating => this == forward || this == reverse;
  bool get isForwardOrCompleted => this == forward || this == completed;
}
```

### AnimationController

```dart
/// A controller for an animation.
class AnimationController extends Animation<double>
    with AnimationEagerListenerMixin,
         AnimationLocalListenersMixin,
         AnimationLocalStatusListenersMixin {

  AnimationController({
    double? value,
    this.duration,
    this.reverseDuration,
    this.debugLabel,
    this.lowerBound = 0.0,
    this.upperBound = 1.0,
    this.animationBehavior = AnimationBehavior.normal,
    required TickerProvider vsync,  // Required!
  });

  /// Bounds
  final double lowerBound;  // Default: 0.0
  final double upperBound;  // Default: 1.0

  /// Durations
  Duration? duration;
  Duration? reverseDuration;

  /// Current value (0.0 to 1.0 by default)
  @override
  double get value => _value;
  set value(double newValue);  // Stops animation and sets value

  /// Animation control methods
  TickerFuture forward({double? from});
  TickerFuture reverse({double? from});
  TickerFuture animateTo(double target, {Duration? duration, Curve curve = Curves.linear});
  TickerFuture animateBack(double target, {Duration? duration, Curve curve = Curves.linear});
  TickerFuture repeat({double? min, double? max, bool reverse = false, Duration? period});
  TickerFuture fling({double velocity = 1.0, SpringDescription? springDescription});
  void stop({bool canceled = true});
  void reset();  // Sets value to lowerBound

  /// Lifecycle
  void dispose();
  void resync(TickerProvider vsync);

  /// Read-only view (can't mutate)
  Animation<double> get view => this;
}
```

**Key Points:**
- Requires `TickerProvider` (usually via `SingleTickerProviderStateMixin`)
- Default range is 0.0 to 1.0
- Must call `dispose()` in `State.dispose()`
- Returns `TickerFuture` that completes when animation finishes

---

## AnimatedWidget

```dart
/// A widget that rebuilds when the given Listenable changes value.
abstract class AnimatedWidget extends StatefulWidget {
  const AnimatedWidget({super.key, required this.listenable});

  /// The Listenable to listen to (usually an Animation)
  final Listenable listenable;

  /// Override to build widgets that depend on the animation
  @protected
  Widget build(BuildContext context);

  @override
  State<AnimatedWidget> createState() => _AnimatedState();
}

class _AnimatedState extends State<AnimatedWidget> {
  @override
  void initState() {
    super.initState();
    widget.listenable.addListener(_handleChange);
  }

  @override
  void didUpdateWidget(AnimatedWidget oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.listenable != oldWidget.listenable) {
      oldWidget.listenable.removeListener(_handleChange);
      widget.listenable.addListener(_handleChange);
    }
  }

  @override
  void dispose() {
    widget.listenable.removeListener(_handleChange);
    super.dispose();
  }

  void _handleChange() {
    if (!mounted) return;
    setState(() {
      // The listenable's state changed - rebuild
    });
  }

  @override
  Widget build(BuildContext context) => widget.build(context);
}
```

**Pattern:**
1. Widget takes `Listenable` (usually `Animation<T>`)
2. State subscribes to listenable in `initState`
3. On change, calls `setState()` to rebuild
4. Widget's `build()` reads current animation value
5. State unsubscribes in `dispose`

---

## Transition Widgets

### SlideTransition

```dart
class SlideTransition extends AnimatedWidget {
  const SlideTransition({
    super.key,
    required Animation<Offset> position,
    this.transformHitTests = true,
    this.textDirection,
    this.child,
  }) : super(listenable: position);

  Animation<Offset> get position => listenable as Animation<Offset>;
  final bool transformHitTests;
  final TextDirection? textDirection;
  final Widget? child;

  @override
  Widget build(BuildContext context) {
    Offset offset = position.value;
    if (textDirection == TextDirection.rtl) {
      offset = Offset(-offset.dx, offset.dy);
    }
    return FractionalTranslation(
      translation: offset,
      transformHitTests: transformHitTests,
      child: child,
    );
  }
}
```

### ScaleTransition & RotationTransition

```dart
class ScaleTransition extends MatrixTransition {
  const ScaleTransition({
    super.key,
    required Animation<double> scale,
    super.alignment = Alignment.center,
    super.filterQuality,
    super.child,
  }) : super(animation: scale, onTransform: _handleScaleMatrix);

  Animation<double> get scale => animation;

  static Matrix4 _handleScaleMatrix(double value) => 
      Matrix4.diagonal3Values(value, value, 1.0);
}

class RotationTransition extends MatrixTransition {
  const RotationTransition({
    super.key,
    required Animation<double> turns,
    super.alignment = Alignment.center,
    super.filterQuality,
    super.child,
  }) : super(animation: turns, onTransform: _handleTurnsMatrix);

  Animation<double> get turns => animation;

  static Matrix4 _handleTurnsMatrix(double value) => 
      Matrix4.rotationZ(value * math.pi * 2.0);
}
```

### FadeTransition (Special Case)

```dart
/// FadeTransition extends SingleChildRenderObjectWidget, not AnimatedWidget!
/// This is for performance - opacity changes don't need widget rebuild.
class FadeTransition extends SingleChildRenderObjectWidget {
  const FadeTransition({
    super.key,
    required this.opacity,
    this.alwaysIncludeSemantics = false,
    super.child,
  });

  final Animation<double> opacity;
  final bool alwaysIncludeSemantics;

  @override
  RenderAnimatedOpacity createRenderObject(BuildContext context) {
    return RenderAnimatedOpacity(
      opacity: opacity,
      alwaysIncludeSemantics: alwaysIncludeSemantics,
    );
  }

  @override
  void updateRenderObject(BuildContext context, RenderAnimatedOpacity renderObject) {
    renderObject
      ..opacity = opacity
      ..alwaysIncludeSemantics = alwaysIncludeSemantics;
  }
}
```

**Why?** `FadeTransition` passes the `Animation` directly to the `RenderObject`, which listens to it. This avoids widget rebuilds entirely - only repaint happens.

### PositionedTransition

```dart
class PositionedTransition extends AnimatedWidget {
  const PositionedTransition({
    super.key,
    required Animation<RelativeRect> rect,
    required this.child,
  }) : super(listenable: rect);

  Animation<RelativeRect> get rect => listenable as Animation<RelativeRect>;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Positioned.fromRelativeRect(rect: rect.value, child: child);
  }
}
```

### DecoratedBoxTransition

```dart
class DecoratedBoxTransition extends AnimatedWidget {
  const DecoratedBoxTransition({
    super.key,
    required this.decoration,
    this.position = DecorationPosition.background,
    required this.child,
  }) : super(listenable: decoration);

  final Animation<Decoration> decoration;
  final DecorationPosition position;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: decoration.value,
      position: position,
      child: child,
    );
  }
}
```

---

## Builder Widgets

### ListenableBuilder / AnimatedBuilder

```dart
/// Generic builder that rebuilds when Listenable changes
class ListenableBuilder extends AnimatedWidget {
  const ListenableBuilder({
    super.key,
    required super.listenable,
    required this.builder,
    this.child,
  });

  final TransitionBuilder builder;  // Widget Function(BuildContext, Widget?)
  final Widget? child;  // Pre-built child for optimization

  @override
  Widget build(BuildContext context) => builder(context, child);
}

/// AnimatedBuilder is just ListenableBuilder with different naming
class AnimatedBuilder extends ListenableBuilder {
  const AnimatedBuilder({
    super.key,
    required Listenable animation,
    required super.builder,
    super.child,
  }) : super(listenable: animation);

  Listenable get animation => listenable;
}
```

**Performance Optimization:**
```dart
// BAD - rebuilds Container on every frame
AnimatedBuilder(
  animation: _controller,
  builder: (context, child) {
    return Transform.rotate(
      angle: _controller.value * 2 * pi,
      child: Container(  // Rebuilt every frame!
        width: 100,
        height: 100,
        color: Colors.blue,
      ),
    );
  },
)

// GOOD - Container is built once, passed through
AnimatedBuilder(
  animation: _controller,
  child: Container(  // Built once
    width: 100,
    height: 100,
    color: Colors.blue,
  ),
  builder: (context, child) {
    return Transform.rotate(
      angle: _controller.value * 2 * pi,
      child: child,  // Reused each frame
    );
  },
)
```

---

## Tween System

### Animatable<T>

```dart
/// Can produce a value of type T given an Animation<double>
abstract class Animatable<T> {
  const Animatable();

  /// Get value at time t (0.0 to 1.0)
  T transform(double t);

  /// Get value for current animation position
  T evaluate(Animation<double> animation) => transform(animation.value);

  /// Create Animation<T> driven by parent Animation<double>
  Animation<T> animate(Animation<double> parent);

  /// Chain: apply parent first, then this
  Animatable<T> chain(Animatable<double> parent);
}
```

### Tween<T>

```dart
/// Linear interpolation between begin and end values
class Tween<T extends Object?> extends Animatable<T> {
  Tween({this.begin, this.end});

  T? begin;
  T? end;

  /// Default implementation uses +, -, * operators
  @protected
  T lerp(double t) {
    return begin + (end - begin) * t;  // Simplified
  }

  @override
  T transform(double t) {
    if (t == 0.0) return begin!;
    if (t == 1.0) return end!;
    return lerp(t);
  }
}
```

### Specialized Tweens

```dart
class ColorTween extends Tween<Color?> {
  @override
  Color? lerp(double t) => Color.lerp(begin, end, t);
}

class SizeTween extends Tween<Size?> {
  @override
  Size? lerp(double t) => Size.lerp(begin, end, t);
}

class RectTween extends Tween<Rect?> {
  @override
  Rect? lerp(double t) => Rect.lerp(begin, end, t);
}

class IntTween extends Tween<int> {
  @override
  int lerp(double t) => (begin! + (end! - begin!) * t).round();
}

class StepTween extends Tween<int> {
  @override
  int lerp(double t) => (begin! + (end! - begin!) * t).floor();
}

class AlignmentTween extends Tween<Alignment> {
  @override
  Alignment lerp(double t) => Alignment.lerp(begin, end, t)!;
}
```

### CurvedAnimation

```dart
/// Applies a curve to the parent animation
class CurvedAnimation extends Animation<double>
    with AnimationWithParentMixin<double> {
  
  CurvedAnimation({
    required this.parent,
    required this.curve,
    this.reverseCurve,
  });

  @override
  final Animation<double> parent;
  Curve curve;
  Curve? reverseCurve;

  @override
  double get value {
    final Curve? activeCurve = (parent.status == AnimationStatus.reverse)
        ? (reverseCurve ?? curve)
        : curve;
    return activeCurve!.transform(parent.value);
  }
}
```

### Chaining Example

```dart
// Create animation chain
final Animation<Color?> colorAnimation = _controller
    .drive(CurveTween(curve: Curves.easeInOut))  // Apply curve
    .drive(ColorTween(begin: Colors.red, end: Colors.blue));  // Apply tween

// Equivalent to:
final curved = CurvedAnimation(parent: _controller, curve: Curves.easeInOut);
final colorAnimation = ColorTween(begin: Colors.red, end: Colors.blue).animate(curved);
```

---

## FLUI Design

### Animation Trait

```rust
/// Status of an animation
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnimationStatus {
    Dismissed,  // At start
    Forward,    // Moving toward end
    Reverse,    // Moving toward start
    Completed,  // At end
}

impl AnimationStatus {
    pub fn is_animating(&self) -> bool {
        matches!(self, Self::Forward | Self::Reverse)
    }

    pub fn is_completed(&self) -> bool {
        *self == Self::Completed
    }

    pub fn is_dismissed(&self) -> bool {
        *self == Self::Dismissed
    }
}

/// Animation that produces values of type T
pub trait Animation<T>: Signal<T> {
    fn status(&self) -> AnimationStatus;
    
    fn is_animating(&self) -> bool {
        self.status().is_animating()
    }
    
    fn is_completed(&self) -> bool {
        self.status().is_completed()
    }
    
    fn is_dismissed(&self) -> bool {
        self.status().is_dismissed()
    }
    
    /// Chain a tween to this animation
    fn drive<U, A: Animatable<T, U>>(&self, animatable: A) -> DrivenAnimation<T, U, Self, A>
    where
        Self: Sized;
}
```

### AnimationController

```rust
pub struct AnimationController {
    value: Signal<f64>,
    status: Signal<AnimationStatus>,
    duration: Duration,
    reverse_duration: Option<Duration>,
    lower_bound: f64,
    upper_bound: f64,
    ticker: Option<TickerHandle>,
}

impl AnimationController {
    pub fn new(
        duration: Duration,
        vsync: &dyn TickerProvider,
    ) -> Self {
        Self {
            value: Signal::new(0.0),
            status: Signal::new(AnimationStatus::Dismissed),
            duration,
            reverse_duration: None,
            lower_bound: 0.0,
            upper_bound: 1.0,
            ticker: None,
        }
    }

    pub fn with_bounds(mut self, lower: f64, upper: f64) -> Self {
        self.lower_bound = lower;
        self.upper_bound = upper;
        self
    }

    pub fn value(&self) -> f64 {
        self.value.get()
    }

    pub fn set_value(&mut self, value: f64) {
        self.stop();
        self.value.set(value.clamp(self.lower_bound, self.upper_bound));
    }

    pub fn forward(&mut self) -> AnimationFuture {
        self.animate_to(self.upper_bound, self.duration)
    }

    pub fn reverse(&mut self) -> AnimationFuture {
        let duration = self.reverse_duration.unwrap_or(self.duration);
        self.animate_to(self.lower_bound, duration)
    }

    pub fn animate_to(&mut self, target: f64, duration: Duration) -> AnimationFuture {
        // Start ticker, update value each frame
        // Return future that completes when done
        todo!()
    }

    pub fn stop(&mut self) {
        if let Some(ticker) = self.ticker.take() {
            ticker.cancel();
        }
    }

    pub fn reset(&mut self) {
        self.set_value(self.lower_bound);
        self.status.set(AnimationStatus::Dismissed);
    }

    pub fn dispose(&mut self) {
        self.stop();
    }
}

impl Animation<f64> for AnimationController {
    fn status(&self) -> AnimationStatus {
        self.status.get()
    }
}
```

### Animatable Trait

```rust
/// Can transform a double (0.0-1.0) into a value of type T
pub trait Animatable<T> {
    fn transform(&self, t: f64) -> T;
    
    fn evaluate<A: Animation<f64>>(&self, animation: &A) -> T {
        self.transform(animation.value())
    }
    
    fn chain<U, A: Animatable<U>>(self, other: A) -> ChainedAnimatable<Self, A>
    where
        Self: Sized,
        T: Into<f64>,
    {
        ChainedAnimatable { parent: self, child: other }
    }
}
```

### Tween

```rust
/// Linear interpolation between two values
pub struct Tween<T: Lerp> {
    pub begin: T,
    pub end: T,
}

impl<T: Lerp> Tween<T> {
    pub fn new(begin: T, end: T) -> Self {
        Self { begin, end }
    }
}

impl<T: Lerp + Clone> Animatable<T> for Tween<T> {
    fn transform(&self, t: f64) -> T {
        if t == 0.0 {
            return self.begin.clone();
        }
        if t == 1.0 {
            return self.end.clone();
        }
        T::lerp(&self.begin, &self.end, t as f32)
    }
}

/// Lerp trait for interpolatable types
pub trait Lerp {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self;
}

// Implementations
impl Lerp for f32 {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        a + (b - a) * t
    }
}

impl Lerp for f64 {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        a + (b - a) * t as f64
    }
}

impl Lerp for Color {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        Color::rgba(
            f32::lerp(&a.r, &b.r, t),
            f32::lerp(&a.g, &b.g, t),
            f32::lerp(&a.b, &b.b, t),
            f32::lerp(&a.a, &b.a, t),
        )
    }
}
```

### CurveTween

```rust
pub struct CurveTween {
    pub curve: Box<dyn Curve>,
}

impl Animatable<f64> for CurveTween {
    fn transform(&self, t: f64) -> f64 {
        self.curve.transform(t)
    }
}

pub trait Curve: Send + Sync {
    fn transform(&self, t: f64) -> f64;
}

// Standard curves
pub struct Linear;
impl Curve for Linear {
    fn transform(&self, t: f64) -> f64 { t }
}

pub struct EaseInOut;
impl Curve for EaseInOut {
    fn transform(&self, t: f64) -> f64 {
        if t < 0.5 {
            2.0 * t * t
        } else {
            -1.0 + (4.0 - 2.0 * t) * t
        }
    }
}
```

### AnimatedView Trait

```rust
/// View that rebuilds when animation changes
pub trait AnimatedView: View {
    type AnimationType;
    
    fn animation(&self) -> &dyn Animation<Self::AnimationType>;
    
    fn build_animated(&self, ctx: &mut BuildContext, value: Self::AnimationType) -> impl View;
}

// Implementation subscribes to animation in element
```

### Transition Views

```rust
/// Fade transition - animates opacity
pub struct FadeTransition {
    pub opacity: Box<dyn Animation<f64>>,
    pub child: Box<dyn View>,
}

impl View for FadeTransition {
    fn build(&self, ctx: &mut BuildContext) -> impl IntoElement {
        // Returns RenderObject that subscribes to animation directly
        // No widget rebuild on animation tick - just repaint
    }
}

/// Slide transition - animates position
pub struct SlideTransition {
    pub position: Box<dyn Animation<Offset>>,
    pub child: Box<dyn View>,
}

impl View for SlideTransition {
    fn build(&self, ctx: &mut BuildContext) -> impl IntoElement {
        FractionalTranslation {
            translation: self.position.get(),
            child: self.child.clone(),
        }
    }
}

/// Scale transition
pub struct ScaleTransition {
    pub scale: Box<dyn Animation<f64>>,
    pub alignment: Alignment,
    pub child: Box<dyn View>,
}

/// Rotation transition
pub struct RotationTransition {
    pub turns: Box<dyn Animation<f64>>,
    pub alignment: Alignment,
    pub child: Box<dyn View>,
}
```

### AnimatedBuilder

```rust
/// Generic builder for animations
pub struct AnimatedBuilder<F>
where
    F: Fn(&mut BuildContext, Option<Box<dyn View>>) -> Box<dyn View> + 'static,
{
    pub animation: Box<dyn Listenable>,
    pub builder: F,
    pub child: Option<Box<dyn View>>,
}

impl<F> View for AnimatedBuilder<F>
where
    F: Fn(&mut BuildContext, Option<Box<dyn View>>) -> Box<dyn View> + 'static,
{
    fn build(&self, ctx: &mut BuildContext) -> impl IntoElement {
        // Element subscribes to animation
        // On change, calls builder with cached child
        (self.builder)(ctx, self.child.clone())
    }
}
```

### Usage Example

```rust
fn animated_logo(ctx: &mut BuildContext) -> impl View {
    let controller = use_animation_controller(ctx, Duration::from_millis(500));
    
    // Start animation on mount
    use_effect(ctx, || {
        controller.forward();
        move || controller.dispose()
    }, []);
    
    // Chain curve and tween
    let scale_animation = controller
        .drive(CurveTween { curve: Box::new(EaseInOut) })
        .drive(Tween::new(0.5, 1.0));
    
    ScaleTransition {
        scale: Box::new(scale_animation),
        alignment: Alignment::CENTER,
        child: Box::new(FlutterLogo { size: 100.0 }),
    }
}
```

---

## Summary

| Flutter | FLUI |
|---------|------|
| `Animation<T>` | `Animation<T>` trait extending `Signal` |
| `AnimationController` | `AnimationController` struct |
| `Listenable` | `Signal` / `Listenable` trait |
| `Tween<T>` | `Tween<T>` with `Lerp` trait |
| `CurvedAnimation` | `CurveTween` + chain |
| `AnimatedWidget` | `AnimatedView` trait |
| `FadeTransition` | `FadeTransition` (render-level) |
| `SlideTransition` | `SlideTransition` |
| `AnimatedBuilder` | `AnimatedBuilder<F>` |

**Key Design Decisions:**
1. `Animation` extends `Signal` for reactive integration
2. `Lerp` trait for type-safe interpolation
3. Render-level animation for performance (FadeTransition)
4. Builder pattern with cached child optimization
5. `use_animation_controller` hook for lifecycle management
