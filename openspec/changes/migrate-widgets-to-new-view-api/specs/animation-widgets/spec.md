# Animation Widgets Specification

## Purpose

This specification references the detailed animation widget requirements documented in `crates/flui_widgets/guide/05_animation_widgets.md`.

## ADDED Requirements

### Requirement: Animation Widget Categories

Animation widgets SHALL be organized into implicit animations, explicit animations, and transition widgets, as documented in guide/05_animation_widgets.md.

#### Scenario: Implicit animation widgets animate property changes automatically

**GIVEN** a developer needs to animate widget properties automatically
**WHEN** using implicit animation widgets (AnimatedContainer, AnimatedPadding, AnimatedAlign, AnimatedPositioned, AnimatedOpacity, AnimatedRotation, AnimatedScale, AnimatedSlide, AnimatedDefaultTextStyle, AnimatedPhysicalModel)
**THEN** widget SHALL support bon builder pattern
**AND** widget SHALL support struct literal pattern
**AND** widget SHALL extend ImplicitlyAnimatedWidget
**AND** widget SHALL support duration, curve, onEnd parameters
**AND** widget SHALL automatically animate property changes between old and new values
**AND** widget SHALL compose appropriate RenderObjects with animation support
**AND** widget SHALL follow patterns documented in guide/05_animation_widgets.md

#### Scenario: AnimatedContainer animates all Container properties

**GIVEN** a developer needs to animate container properties
**WHEN** using AnimatedContainer widget
**THEN** widget SHALL support all Container parameters (padding, margin, color, decoration, width, height, constraints, alignment, transform)
**AND** widget SHALL automatically animate changes to any parameter
**AND** widget SHALL compose multiple RenderObjects (RenderPadding, RenderDecoratedBox, etc.) with animation
**AND** widget SHALL follow patterns documented in guide/05_animation_widgets.md

#### Scenario: AnimatedSwitcher cross-fades between children

**GIVEN** a developer needs to cross-fade between different child widgets
**WHEN** using AnimatedSwitcher widget
**THEN** widget SHALL use RenderStack + RenderAnimatedOpacity for cross-fade
**AND** widget SHALL support duration, reverseDuration, switchInCurve, switchOutCurve
**AND** widget SHALL support transitionBuilder for custom transitions
**AND** widget SHALL support layoutBuilder for custom layout during transition
**AND** widget SHALL detect child changes via Widget.key comparison
**AND** widget SHALL follow patterns documented in guide/05_animation_widgets.md

#### Scenario: AnimatedCrossFade cross-fades between two specific children

**GIVEN** a developer needs to cross-fade between exactly two children
**WHEN** using AnimatedCrossFade widget
**THEN** widget SHALL use RenderStack + RenderAnimatedOpacity
**AND** widget SHALL support firstChild, secondChild, crossFadeState parameters
**AND** widget SHALL support duration, reverseDuration, firstCurve, secondCurve, sizeCurve
**AND** widget SHALL support alignment, layoutBuilder parameters
**AND** widget SHALL follow patterns documented in guide/05_animation_widgets.md

#### Scenario: Hero enables shared element transitions

**GIVEN** a developer needs shared element transition between routes
**WHEN** using Hero widget with matching tag
**THEN** widget SHALL coordinate with Navigator for route transitions
**AND** widget SHALL support tag parameter (Object) for matching across routes
**AND** widget SHALL support createRectTween, flightShuttleBuilder, placeholderBuilder
**AND** widget SHALL support transitionOnUserGestures parameter
**AND** widget SHALL use child's RenderObject + overlay during transition
**AND** widget SHALL follow patterns documented in guide/05_animation_widgets.md

---

### Requirement: Explicit Animation Widgets

Explicit animation widgets SHALL use AnimationController for manual animation control, as documented in guide/05_animation_widgets.md.

#### Scenario: AnimatedBuilder rebuilds on animation changes

**GIVEN** a developer needs to rebuild widget tree on animation value changes
**WHEN** using AnimatedBuilder widget
**THEN** widget SHALL accept Listenable (typically Animation) parameter
**AND** widget SHALL accept builder callback receiving BuildContext and optional child
**AND** widget SHALL support child parameter for cached non-rebuilding subtree
**AND** widget SHALL rebuild only builder content on animation changes
**AND** widget SHALL follow patterns documented in guide/05_animation_widgets.md

#### Scenario: TweenAnimationBuilder simplifies tween-based animations

**GIVEN** a developer needs simple tween-based animation without AnimationController
**WHEN** using TweenAnimationBuilder widget
**THEN** widget SHALL accept Tween<T> parameter for value interpolation
**AND** widget SHALL support duration, curve, onEnd parameters
**AND** widget SHALL accept builder callback receiving value of type T
**AND** widget SHALL support child parameter for caching
**AND** widget SHALL automatically manage AnimationController lifecycle
**AND** widget SHALL follow patterns documented in guide/05_animation_widgets.md

#### Scenario: AnimatedWidget provides base for custom animated widgets

**GIVEN** a developer needs to create custom animated widget
**WHEN** extending AnimatedWidget abstract class
**THEN** widget SHALL accept Listenable parameter
**AND** widget SHALL implement build(BuildContext) method
**AND** widget SHALL automatically rebuild on listenable changes
**AND** subclass SHALL be used for reusable animated components
**AND** widget SHALL follow patterns documented in guide/05_animation_widgets.md

---

### Requirement: Transition Widgets

Transition widgets SHALL provide pre-built animations driven by Animation<T> controllers, as documented in guide/05_animation_widgets.md.

#### Scenario: Transition widgets provide common animation patterns

**GIVEN** a developer needs standard animation effects with explicit control
**WHEN** using transition widgets (FadeTransition, SlideTransition, ScaleTransition, RotationTransition, SizeTransition, PositionedTransition, DecoratedBoxTransition, AlignTransition, DefaultTextStyleTransition)
**THEN** widget SHALL extend AnimatedWidget
**AND** widget SHALL accept Animation<T> parameter for explicit control
**AND** FadeTransition SHALL use RenderAnimatedOpacity with Animation<double>
**AND** SlideTransition SHALL use RenderFractionalTranslation with Animation<Offset>
**AND** ScaleTransition SHALL use RenderTransform with Animation<double>
**AND** RotationTransition SHALL use RenderTransform with Animation<double> (turns)
**AND** SizeTransition SHALL use RenderAnimatedSize with Animation<double>
**AND** widget SHALL follow patterns documented in guide/05_animation_widgets.md

---

### Requirement: AnimationController Integration

Animation widgets SHALL integrate with AnimationController and Tween for explicit animation control.

#### Scenario: AnimationController provides animation state management

**GIVEN** an explicit animation widget needs controller
**WHEN** using AnimationController (not a widget, but critical for explicit animations)
**THEN** AnimationController SHALL support duration, reverseDuration, lowerBound, upperBound, value, vsync
**AND** AnimationController SHALL expose forward(), reverse(), repeat(), reset(), stop() methods
**AND** AnimationController SHALL expose animateTo(value), animateBack(value) methods
**AND** AnimationController SHALL require TickerProvider for vsync (typically StatefulWidget with TickerProviderStateMixin)
**AND** AnimationController SHALL be disposed in StatefulWidget.dispose()

#### Scenario: Tween defines value interpolation

**GIVEN** an animation needs value interpolation between begin and end
**WHEN** using Tween<T> (not a widget, but critical for animations)
**THEN** Tween SHALL support begin and end parameters of type T
**AND** Tween SHALL provide animate(Animation) method returning Animation<T>
**AND** Tween SHALL support chain(Animatable) for composition
**AND** built-in Tweens SHALL include ColorTween, SizeTween, RectTween, IntTween
**AND** custom Tweens SHALL override lerp(double t) method

#### Scenario: Curves define animation timing

**GIVEN** an animation needs custom timing curve
**WHEN** using Curve constants or custom Curve
**THEN** standard curves SHALL include linear, easeIn, easeOut, easeInOut, fastOutSlowIn, bounceIn, bounceOut, elasticIn, elasticOut
**AND** custom curves SHALL use Cubic(a, b, c, d) or Interval(begin, end)
**AND** CurvedAnimation SHALL apply curve to AnimationController

---

## Related Documentation

- **Guide:** `crates/flui_widgets/guide/05_animation_widgets.md` - Detailed widget reference
- **Architecture:** `crates/flui_widgets/guide/WIDGET_ARCHITECTURE.md` - Widget organization patterns
- **Implementation:** `crates/flui_widgets/guide/IMPLEMENTATION_GUIDE.md` - Code examples

## Widgets Covered

**Total:** 23 animation widgets

**Implicit Animations (10):**
- AnimatedContainer, AnimatedPadding, AnimatedAlign, AnimatedPositioned
- AnimatedOpacity, AnimatedRotation, AnimatedScale, AnimatedSlide
- AnimatedDefaultTextStyle, AnimatedPhysicalModel

**Cross-Fade and Hero (3):**
- AnimatedSwitcher, AnimatedCrossFade, Hero

**Explicit Animation Builders (3):**
- AnimatedBuilder, AnimatedWidget (abstract base), TweenAnimationBuilder

**Transition Widgets (9):**
- FadeTransition, SlideTransition, ScaleTransition, RotationTransition
- SizeTransition, PositionedTransition, DecoratedBoxTransition
- AlignTransition, DefaultTextStyleTransition

**Supporting Types (not widgets):**
- AnimationController
- Tween<T>, ColorTween, SizeTween, RectTween, IntTween
- Curve, Cubic, Interval
- CurvedAnimation
