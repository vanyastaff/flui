# ImplicitlyAnimatedWidget Pattern

This document analyzes Flutter's ImplicitlyAnimatedWidget system - widgets that automatically animate property changes without explicit AnimationController management.

## Source Files
- `packages/flutter/lib/src/widgets/implicit_animations.dart`

## Core Architecture

### ImplicitlyAnimatedWidget Base Class

```dart
abstract class ImplicitlyAnimatedWidget extends StatefulWidget {
  const ImplicitlyAnimatedWidget({
    super.key,
    this.curve = Curves.linear,
    required this.duration,
    this.onEnd,
  });

  /// The curve to apply when animating
  final Curve curve;

  /// The duration over which to animate
  final Duration duration;

  /// Called every time an animation completes
  final VoidCallback? onEnd;

  @override
  ImplicitlyAnimatedWidgetState<ImplicitlyAnimatedWidget> createState();
}
```

**Key Design Points:**
- Extends `StatefulWidget` - needs state to manage animation
- Takes `duration` and `curve` as configuration
- Optional `onEnd` callback for animation completion

### ImplicitlyAnimatedWidgetState

```dart
abstract class ImplicitlyAnimatedWidgetState<T extends ImplicitlyAnimatedWidget> 
    extends State<T>
    with SingleTickerProviderStateMixin<T> {
  
  /// The animation controller driving this widget's implicit animations
  @protected
  late final AnimationController controller = AnimationController(
    duration: widget.duration,
    debugLabel: kDebugMode ? widget.toStringShort() : null,
    vsync: this,
  );

  /// The animation driving this widget's implicit animations
  Animation<double> get animation => _animation;
  late CurvedAnimation _animation = _createCurve();

  @protected
  @override
  void initState() {
    super.initState();
    controller.addStatusListener((AnimationStatus status) {
      if (status.isCompleted) {
        widget.onEnd?.call();
      }
    });
    _constructTweens();
    didUpdateTweens();
  }

  @protected
  @override
  void didUpdateWidget(T oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.curve != oldWidget.curve) {
      _animation.dispose();
      _animation = _createCurve();
    }
    controller.duration = widget.duration;
    if (_constructTweens()) {
      forEachTween((tween, targetValue, constructor) {
        return tween
          ?..begin = tween.evaluate(_animation)
          ..end = targetValue;
      });
      controller.forward(from: 0.0);
      didUpdateTweens();
    }
  }

  /// Visits each tween controlled by this state
  @protected
  void forEachTween(TweenVisitor<dynamic> visitor);

  /// Hook called after tweens are updated
  @protected
  void didUpdateTweens() {}
}
```

**Lifecycle:**
1. `initState()` - Create controller, construct initial tweens
2. `didUpdateWidget()` - Compare old/new values, start animation if changed
3. `forEachTween()` - Subclass provides tween visitation
4. `didUpdateTweens()` - Hook for dependent property updates

### Tween Visitor Pattern

```dart
typedef TweenConstructor<T extends Object> = Tween<T> Function(T targetValue);

typedef TweenVisitor<T extends Object> = Tween<T>? Function(
  Tween<T>? tween, 
  T targetValue, 
  TweenConstructor<T> constructor
);
```

The visitor pattern allows declarative tween management:

```dart
@override
void forEachTween(TweenVisitor<dynamic> visitor) {
  _colorTween = visitor(
    _colorTween,           // Current tween (null initially)
    widget.targetColor,    // Target value
    (value) => ColorTween(begin: value as Color?),  // Constructor
  ) as ColorTween?;
}
```

### AnimatedWidgetBaseState

```dart
abstract class AnimatedWidgetBaseState<T extends ImplicitlyAnimatedWidget>
    extends ImplicitlyAnimatedWidgetState<T> {
  
  @protected
  @override
  void initState() {
    super.initState();
    controller.addListener(_handleAnimationChanged);
  }

  void _handleAnimationChanged() {
    setState(() {
      /* Rebuild with new animation value */
    });
  }
}
```

**Difference from ImplicitlyAnimatedWidgetState:**
- `AnimatedWidgetBaseState` calls `setState()` on every animation tick
- `ImplicitlyAnimatedWidgetState` requires manual rebuild handling

## Tween Types

Flutter provides specialized tweens with proper lerp implementations:

```dart
class BoxConstraintsTween extends Tween<BoxConstraints> {
  @override
  BoxConstraints lerp(double t) => BoxConstraints.lerp(begin, end, t)!;
}

class DecorationTween extends Tween<Decoration> {
  @override
  Decoration lerp(double t) => Decoration.lerp(begin, end, t)!;
}

class EdgeInsetsTween extends Tween<EdgeInsets> {
  @override
  EdgeInsets lerp(double t) => EdgeInsets.lerp(begin, end, t)!;
}

class BorderRadiusTween extends Tween<BorderRadius?> {
  @override
  BorderRadius? lerp(double t) => BorderRadius.lerp(begin, end, t);
}

class Matrix4Tween extends Tween<Matrix4> {
  @override
  Matrix4 lerp(double t) {
    // Decompose matrices into translation, rotation, scale
    // Lerp each component separately
    // Recompose into result matrix
  }
}

class TextStyleTween extends Tween<TextStyle> {
  @override
  TextStyle lerp(double t) => TextStyle.lerp(begin, end, t)!;
}
```

## Concrete Implementations

### AnimatedContainer

```dart
class AnimatedContainer extends ImplicitlyAnimatedWidget {
  final Widget? child;
  final AlignmentGeometry? alignment;
  final EdgeInsetsGeometry? padding;
  final Decoration? decoration;
  final BoxConstraints? constraints;
  final EdgeInsetsGeometry? margin;
  final Matrix4? transform;
  // ...
}

class _AnimatedContainerState extends AnimatedWidgetBaseState<AnimatedContainer> {
  AlignmentGeometryTween? _alignment;
  EdgeInsetsGeometryTween? _padding;
  DecorationTween? _decoration;
  BoxConstraintsTween? _constraints;
  EdgeInsetsGeometryTween? _margin;
  Matrix4Tween? _transform;

  @override
  void forEachTween(TweenVisitor<dynamic> visitor) {
    _alignment = visitor(_alignment, widget.alignment, 
        (value) => AlignmentGeometryTween(begin: value)) as AlignmentGeometryTween?;
    _padding = visitor(_padding, widget.padding,
        (value) => EdgeInsetsGeometryTween(begin: value)) as EdgeInsetsGeometryTween?;
    // ... for each animatable property
  }

  @override
  Widget build(BuildContext context) {
    return Container(
      alignment: _alignment?.evaluate(animation),
      padding: _padding?.evaluate(animation),
      decoration: _decoration?.evaluate(animation),
      constraints: _constraints?.evaluate(animation),
      margin: _margin?.evaluate(animation),
      transform: _transform?.evaluate(animation),
      child: widget.child,
    );
  }
}
```

### AnimatedPadding, AnimatedAlign, AnimatedPositioned

Similar pattern - each animates specific subset of properties:

- `AnimatedPadding` - only `padding`
- `AnimatedAlign` - `alignment`, `widthFactor`, `heightFactor`  
- `AnimatedPositioned` - `left`, `top`, `right`, `bottom`, `width`, `height`

---

## FLUI Design

### Rust Equivalent: ImplicitlyAnimatedView

```rust
/// Configuration for implicit animations
pub struct AnimationConfig {
    pub duration: Duration,
    pub curve: Box<dyn Curve>,
}

/// Trait for views that animate property changes automatically
pub trait ImplicitlyAnimatedView: View {
    /// Animation configuration
    fn animation_config(&self) -> &AnimationConfig;
    
    /// Visit all animatable properties
    fn for_each_tween<F>(&self, visitor: F)
    where
        F: FnMut(&mut dyn AnyTween, &dyn Any);
}
```

### Tween System

```rust
/// Type-erased tween for visitor pattern
pub trait AnyTween: Send + Sync {
    fn set_target(&mut self, value: &dyn Any);
    fn evaluate(&self, t: f32) -> Box<dyn Any>;
    fn needs_animation(&self) -> bool;
}

/// Typed tween with lerp capability
pub struct Tween<T: Lerp> {
    begin: Option<T>,
    end: Option<T>,
}

impl<T: Lerp + Clone> Tween<T> {
    pub fn evaluate(&self, t: f32) -> T {
        match (&self.begin, &self.end) {
            (Some(begin), Some(end)) => T::lerp(begin, end, t),
            (Some(begin), None) => begin.clone(),
            (None, Some(end)) => end.clone(),
            (None, None) => panic!("Tween has no value"),
        }
    }
}

/// Lerp trait for interpolatable types
pub trait Lerp {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self;
}

// Implementations for common types
impl Lerp for f32 {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        a + (b - a) * t
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

impl Lerp for EdgeInsets {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        EdgeInsets {
            left: f32::lerp(&a.left, &b.left, t),
            top: f32::lerp(&a.top, &b.top, t),
            right: f32::lerp(&a.right, &b.right, t),
            bottom: f32::lerp(&a.bottom, &b.bottom, t),
        }
    }
}
```

### State with Animation Support

```rust
/// State that manages implicit animation
pub struct ImplicitAnimationState<V: ImplicitlyAnimatedView> {
    controller: AnimationController,
    tweens: V::Tweens,
    view_state: V::State,
}

impl<V: ImplicitlyAnimatedView> ImplicitAnimationState<V> {
    fn update(&mut self, old_view: &V, new_view: &V, ctx: &mut BuildContext) {
        let mut needs_animation = false;
        
        new_view.for_each_tween(|tween, target| {
            if tween.needs_animation() {
                needs_animation = true;
            }
            tween.set_target(target);
        });
        
        if needs_animation {
            self.controller.forward_from(0.0);
        }
    }
}
```

### Declarative API with Derive Macro

```rust
/// Derive macro generates forEachTween implementation
#[derive(ImplicitlyAnimated)]
pub struct AnimatedContainer {
    #[animate]
    pub padding: EdgeInsets,
    #[animate]
    pub color: Color,
    #[animate]  
    pub border_radius: BorderRadius,
    
    // Non-animated properties
    pub child: Box<dyn View>,
    
    // Animation config
    #[animation]
    pub animation: AnimationConfig,
}

// Generated code:
impl ImplicitlyAnimatedView for AnimatedContainer {
    type Tweens = AnimatedContainerTweens;
    
    fn animation_config(&self) -> &AnimationConfig {
        &self.animation
    }
    
    fn for_each_tween<F>(&self, mut visitor: F)
    where F: FnMut(&mut dyn AnyTween, &dyn Any)
    {
        visitor(&mut self.tweens.padding, &self.padding);
        visitor(&mut self.tweens.color, &self.color);
        visitor(&mut self.tweens.border_radius, &self.border_radius);
    }
}

struct AnimatedContainerTweens {
    padding: Tween<EdgeInsets>,
    color: Tween<Color>,
    border_radius: Tween<BorderRadius>,
}
```

### Usage Example

```rust
// Simple usage - just change properties, animation happens automatically
AnimatedContainer {
    padding: if expanded { 
        EdgeInsets::all(20.0) 
    } else { 
        EdgeInsets::all(8.0) 
    },
    color: if selected { Color::BLUE } else { Color::GRAY },
    animation: AnimationConfig {
        duration: Duration::from_millis(300),
        curve: Box::new(Curves::ease_in_out()),
    },
    child: text("Hello"),
}
```

### Integration with Signals

```rust
// With reactive state
fn animated_box(ctx: &mut BuildContext) -> impl View {
    let expanded = use_signal(ctx, false);
    
    AnimatedContainer {
        padding: if expanded.get() { 
            EdgeInsets::all(20.0) 
        } else { 
            EdgeInsets::all(8.0) 
        },
        animation: AnimationConfig::default(),
        child: Button::new("Toggle")
            .on_click(move || expanded.set(!expanded.get())),
    }
}
```

## Key Differences from Flutter

| Aspect | Flutter | FLUI |
|--------|---------|------|
| Tween storage | In State object | In generated Tweens struct |
| Lerp dispatch | Virtual method | Trait impl |
| Animation tick | setState() rebuild | Signal-based reactivity |
| Visitor pattern | Dynamic casting | Generic with Any trait |
| Derive support | None | Macro-generated |

## Summary

ImplicitlyAnimatedWidget provides:
1. **Automatic animation** - No manual AnimationController management
2. **Declarative** - Just change properties, animation happens
3. **Composable** - Tween visitor pattern handles any number of properties
4. **Efficient** - Only animates when values actually change

FLUI can leverage Rust's type system with derive macros to make this even more ergonomic while maintaining the same user-friendly declarative API.
