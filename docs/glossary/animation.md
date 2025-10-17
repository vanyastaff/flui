animation library
The Flutter animation system.

To use, import package:flutter/animation.dart.

This library provides basic building blocks for implementing animations in Flutter. Other layers of the framework use these building blocks to provide advanced animation support for applications. For example, the widget library includes ImplicitlyAnimatedWidgets and AnimatedWidgets that make it easy to animate certain properties of a Widget. If those animated widgets are not sufficient for a given use case, the basic building blocks provided by this library can be used to implement custom animated effects.

This library depends only on core Dart libraries and the physics.dart library.

Foundations: the Animation class
Flutter represents an animation as a value that changes over a given duration, and that value may be of any type. For example, it could be a double indicating the current opacity of a Widget as it fades out. Or, it could be the current background Color of a widget that transitions smoothly from one color to another. The current value of an animation is represented by an Animation object, which is the central class of the animation library. In addition to the current animation value, the Animation object also stores the current AnimationStatus. The status indicates whether the animation is currently conceptually running from the beginning to the end or the other way around. It may also indicate that the animation is currently stopped at the beginning or the end.

Other objects can register listeners on an Animation to be informed whenever the animation value and/or the animation status changes. A Widget may register such a value listener via Animation.addListener to rebuild itself with the current animation value whenever that value changes. For example, a widget might listen to an animation to update its opacity to the animation's value every time that value changes. Likewise, registering a status listener via Animation.addStatusListener may be useful to trigger another action when the current animation has ended.

As an example, the following video shows the changes over time in the current animation status and animation value for the opacity animation of a widget. This Animation is driven by an AnimationController (see next section). Before the animation triggers, the animation status is "dismissed" and the value is 0.0. As the value runs from 0.0 to 1.0 to fade in the widget, the status changes to "forward". When the widget is fully faded in at an animation value of 1.0 the status is "completed". When the animation triggers again to fade the widget back out, the animation status changes to "reverse" and the animation value runs back to 0.0. At that point the widget is fully faded out and the animation status switches back to "dismissed" until the animation is triggered again.

Although you can't instantiate Animation directly (it is an abstract class), you can create one using an AnimationController.

Powering animations: AnimationController
An AnimationController is a special kind of Animation that advances its animation value whenever the device running the application is ready to display a new frame (typically, this rate is around 60 values per second). An AnimationController can be used wherever an Animation is expected. As the name implies, an AnimationController also provides control over its Animation: It implements methods to stop the animation at any time and to run it forward as well as in the reverse direction.

By default, an AnimationController increases its animation value linearly over the given duration from 0.0 to 1.0 when run in the forward direction. For many use cases you might want the value to be of a different type, change the range of the animation values, or change how the animation moves between values. This is achieved by wrapping the animation: Wrapping it in an Animatable (see below) changes the range of animation values to a different range or type (for example to animate Colors or Rects). Furthermore, a Curve can be applied to the animation by wrapping it in a CurvedAnimation. Instead of linearly increasing the animation value, a curved animation changes its value according to the provided curve. The framework ships with many built-in curves (see Curves). As an example, Curves.easeOutCubic increases the animation value quickly at the beginning of the animation and then slows down until the target value is reached:

Animating different types: Animatable
An Animatable<T> is an object that takes an Animation<double> as input and produces a value of type T. Objects of these types can be used to translate the animation value range of an AnimationController (or any other Animation of type double) to a different range. That new range doesn't even have to be of type double anymore. With the help of an Animatable like a Tween or a TweenSequence (see sections below) an AnimationController can be used to smoothly transition Colors, Rects, Sizes and many more types from one value to another over a given duration.

Interpolating values: Tweens
A Tween is applied to an Animation of type double to change the range and type of the animation value. For example, to transition the background of a Widget smoothly between two Colors, a ColorTween can be used. Each Tween specifies a start and an end value. As the animation value of the Animation powering the Tween progresses from 0.0 to 1.0 it produces interpolated values between its start and end value. The values produced by the Tween usually move closer and closer to its end value as the animation value of the powering Animation approaches 1.0.

The following video shows example values produced by an IntTween, a Tween<double>, and a ColorTween as the animation value runs from 0.0 to 1.0 and back to 0.0:

An Animation or AnimationController can power multiple Tweens. For example, to animate the size and the color of a widget in parallel, create one AnimationController that powers a SizeTween and a ColorTween.

The framework ships with many Tween subclasses (IntTween, SizeTween, RectTween, etc.) to animate common properties.

Staggered animations: TweenSequences
A TweenSequence can help animate a given property smoothly in stages. Each Tween in the sequence is responsible for a different stage and has an associated weight. When the animation runs, the stages execute one after another. For example, let's say you want to animate the background of a widget from yellow to green and then, after a short pause, to red. For this you can specify three tweens within a tween sequence: One ColorTween animating from yellow to green, one ConstantTween that just holds the color green, and another ColorTween animating from green to red. For each tween you need to pick a weight indicating the ratio of time spent on that tween compared to all other tweens. If we assign a weight of 2 to both of the ColorTweens and a weight of 1 to the ConstantTween the transition described by the ColorTweens would take twice as long as the ConstantTween. A TweenSequence is driven by an Animation just like a regular Tween: As the powering Animation runs from 0.0 to 1.0 the TweenSequence runs through all of its stages.

The following video shows the animation described in the previous paragraph:

See also:

Introduction to animations on flutter.dev.
Animations tutorial on flutter.dev.
Sample app, which showcases Flutter's animation features.
ImplicitlyAnimatedWidget and its subclasses, which are Widgets that implicitly animate changes to their properties.
AnimatedWidget and its subclasses, which are Widgets that take an explicit Animation to animate their properties.
Classes
AlwaysStoppedAnimation<T>
An animation that is always stopped at a given value.
Animatable<T>
An object that can produce a value of type T given an Animation<double> as input.
Animation<T>
A value which might change over time, moving forward or backward.
AnimationController
A controller for an animation.
AnimationMax<T extends num>
An animation that tracks the maximum of two other animations.
AnimationMean
An animation of doubles that tracks the mean of two other animations.
AnimationMin<T extends num>
An animation that tracks the minimum of two other animations.
AnimationStyle
Used to override the default parameters of an animation.
CatmullRomCurve
An animation easing curve that passes smoothly through the given control points using a centripetal Catmull-Rom spline.
CatmullRomSpline
A 2D spline that passes smoothly through the given control points using a centripetal Catmull-Rom spline.
Color
An immutable color value in ARGB format.
ColorTween
An interpolation between two colors.
CompoundAnimation<T>
An interface for combining multiple Animations. Subclasses need only implement the value getter to control how the child animations are combined. Can be chained to combine more than 2 animations.
ConstantTween<T>
A tween with a constant value.
Cubic
A cubic polynomial mapping of the unit interval.
Curve
An parametric animation easing curve, i.e. a mapping of the unit interval to the unit interval.
Curve2D
Abstract class that defines an API for evaluating 2D parametric curves.
Curve2DSample
A class that holds a sample of a 2D parametric curve, containing the value (the X, Y coordinates) of the curve at the parametric value t.
CurvedAnimation
An animation that applies a curve to another animation.
Curves
A collection of common animation curves.
CurveTween
Transforms the value of the given animation by the given curve.
ElasticInCurve
An oscillating curve that grows in magnitude while overshooting its bounds.
ElasticInOutCurve
An oscillating curve that grows and then shrinks in magnitude while overshooting its bounds.
ElasticOutCurve
An oscillating curve that shrinks in magnitude while overshooting its bounds.
FlippedCurve
A curve that is the reversed inversion of its given curve.
FlippedTweenSequence
Enables creating a flipped Animation whose value is defined by a sequence of Tweens.
Interval
A curve that is 0.0 until begin, then curved (according to curve) from 0.0 at begin to 1.0 at end, then remains 1.0 past end.
IntTween
An interpolation between two integers that rounds.
Offset
An immutable 2D floating-point offset.
ParametricCurve<T>
An abstract class providing an interface for evaluating a parametric curve.
ProxyAnimation
An animation that is a proxy for another animation.
Rect
An immutable, 2D, axis-aligned, floating-point rectangle whose coordinates are relative to a given origin.
RectTween
An interpolation between two rectangles.
ReverseAnimation
An animation that is the reverse of another animation.
ReverseTween<T extends Object?>
A Tween that evaluates its parent in reverse.
SawTooth
A sawtooth curve that repeats a given number of times over the unit interval.
Simulation
The base class for all simulations.
Size
Holds a 2D floating-point size.
SizeTween
An interpolation between two sizes.
Split
A curve that progresses according to beginCurve until split, then according to endCurve.
SpringDescription
Structure that describes a spring's constants.
StepTween
An interpolation between two integers that floors.
ThreePointCubic
A cubic polynomial composed of two curves that share a common center point.
Threshold
A curve that is 0.0 until it hits the threshold, then it jumps to 1.0.
TickerFuture
An object representing an ongoing Ticker sequence.
TickerProvider
An interface implemented by classes that can vend Ticker objects.
TrainHoppingAnimation
This animation starts by proxying one animation, but when the value of that animation crosses the value of the second (either because the second is going in the opposite direction, or because the one overtakes the other), the animation hops over to proxying the second animation.
Tween<T extends Object?>
A linear interpolation between a beginning and ending value.
TweenSequence<T>
Enables creating an Animation whose value is defined by a sequence of Tweens.
TweenSequenceItem<T>
A simple holder for one element of a TweenSequence.
Enums
AnimationBehavior
Configures how an AnimationController behaves when animations are disabled.
AnimationStatus
The status of an animation.
Mixins
AnimationEagerListenerMixin
A mixin that replaces the didRegisterListener/didUnregisterListener contract with a dispose contract.
AnimationLazyListenerMixin
A mixin that helps listen to another object only when this object has registered listeners.
AnimationLocalListenersMixin
A mixin that implements the addListener/removeListener protocol and notifies all the registered listeners when notifyListeners is called.
AnimationLocalStatusListenersMixin
A mixin that implements the addStatusListener/removeStatusListener protocol and notifies all the registered listeners when notifyStatusListeners is called.
AnimationWithParentMixin<T>
Implements most of the Animation interface by deferring its behavior to a given parent Animation.
Constants
kAlwaysCompleteAnimation → const Animation<double>
An animation that is always complete.
kAlwaysDismissedAnimation → const Animation<double>
An animation that is always dismissed.
Typedefs
AnimatableCallback<T> = T Function(double value)
A typedef used by Animatable.fromCallback to create an Animatable from a callback.
AnimationStatusListener = void Function(AnimationStatus status)
Signature for listeners attached using Animation.addStatusListener.
ValueListenableTransformer<T> = T Function(T)
Signature for method used to transform values in Animation.fromValueListenable.
VoidCallback = void Function()
Signature of callbacks that have no arguments and return no data.
Exceptions / Errors
TickerCanceled
Exception thrown by Ticker objects on the TickerFuture.orCancel future when the ticker is canceled.