gestures library
The Flutter gesture recognizers.

To use, import package:flutter/gestures.dart.

Classes
BaseTapAndDragGestureRecognizer
A base class for gesture recognizers that recognize taps and movements.
BaseTapGestureRecognizer
A base class for gesture recognizers that recognize taps.
DelayedMultiDragGestureRecognizer
Recognizes movement both horizontally and vertically on a per-pointer basis after a delay.
DeviceGestureSettings
The device specific gesture settings scaled into logical pixels.
DiagnosticPropertiesBuilder
Builder to accumulate properties and configuration used to assemble a DiagnosticsNode from a Diagnosticable object.
DiagnosticsNode
Defines diagnostics data for a value.
DoubleTapGestureRecognizer
Recognizes when the user has tapped the screen at the same location twice in quick succession.
Drag
Interface for objects that receive updates about drags.
DragDownDetails
Details object for callbacks that use GestureDragDownCallback.
DragEndDetails
Details object for callbacks that use GestureDragEndCallback.
DragGestureRecognizer
Recognizes movement.
DragStartDetails
Details object for callbacks that use GestureDragStartCallback.
DragUpdateDetails
Details object for callbacks that use GestureDragUpdateCallback.
EagerGestureRecognizer
A gesture recognizer that eagerly claims victory in all gesture arenas.
FlutterErrorDetailsForPointerEventDispatcher
Variant of FlutterErrorDetails with extra fields for the gesture library's binding's pointer event dispatcher (GestureBinding.dispatchEvent).
FlutterView
A view into which a Flutter Scene is drawn.
ForcePressDetails
Details object for callbacks that use GestureForcePressStartCallback, GestureForcePressPeakCallback, GestureForcePressEndCallback or GestureForcePressUpdateCallback.
ForcePressGestureRecognizer
Recognizes a force press on devices that have force sensors.
GestureArenaEntry
An interface to pass information to an arena.
GestureArenaManager
Used for disambiguating the meaning of sequences of pointer events.
GestureArenaMember
Represents an object participating in an arena.
GestureArenaTeam
A group of GestureArenaMember objects that are competing as a unit in the GestureArenaManager.
GestureRecognizer
The base class that all gesture recognizers inherit from.
HitTestable
An object that can hit-test pointers.
HitTestDispatcher
An object that can dispatch events.
HitTestEntry<T extends HitTestTarget>
Data collected during a hit test about a specific HitTestTarget.
HitTestResult
The result of performing a hit test.
HitTestTarget
An object that can handle events.
HorizontalDragGestureRecognizer
Recognizes movement in the horizontal direction.
HorizontalMultiDragGestureRecognizer
Recognizes movement in the horizontal direction on a per-pointer basis.
ImmediateMultiDragGestureRecognizer
Recognizes movement both horizontally and vertically on a per-pointer basis.
IOSScrollViewFlingVelocityTracker
A VelocityTracker subclass that provides a close approximation of iOS scroll view's velocity estimation strategy.
LeastSquaresSolver
Uses the least-squares algorithm to fit a polynomial to a set of data.
LongPressDownDetails
Details for callbacks that use GestureLongPressDownCallback.
LongPressEndDetails
Details for callbacks that use GestureLongPressEndCallback.
LongPressGestureRecognizer
Recognizes when the user has pressed down at the same location for a long period of time.
LongPressMoveUpdateDetails
Details for callbacks that use GestureLongPressMoveUpdateCallback.
LongPressStartDetails
Details for callbacks that use GestureLongPressStartCallback.
MacOSScrollViewFlingVelocityTracker
A VelocityTracker subclass that provides a close approximation of macOS scroll view's velocity estimation strategy.
Matrix4
4D Matrix. Values are stored in column major order.
MultiDragGestureRecognizer
Recognizes movement on a per-pointer basis.
MultiDragPointerState
Per-pointer state for a MultiDragGestureRecognizer.
MultiTapGestureRecognizer
Recognizes taps on a per-pointer basis.
Offset
An immutable 2D floating-point offset.
OffsetPair
A container for a local and global Offset pair.
OneSequenceGestureRecognizer
Base class for gesture recognizers that can only recognize one gesture at a time. For example, a single TapGestureRecognizer can never recognize two taps happening simultaneously, even if multiple pointers are placed on the same widget.
PanGestureRecognizer
Recognizes movement both horizontally and vertically.
PointerAddedEvent
The device has started tracking the pointer.
PointerCancelEvent
The input from the pointer is no longer directed towards this receiver.
PointerData
Information about the state of a pointer.
PointerDownEvent
The pointer has made contact with the device.
PointerEnterEvent
The pointer has moved with respect to the device while the pointer is or is not in contact with the device, and it has entered a target object.
PointerEvent
Base class for touch, stylus, or mouse events.
PointerEventConverter
Converts from engine pointer data to framework pointer events.
PointerEventResampler
Class for pointer event resampling.
PointerExitEvent
The pointer has moved with respect to the device while the pointer is or is not in contact with the device, and exited a target object.
PointerHoverEvent
The pointer has moved with respect to the device while the pointer is not in contact with the device.
PointerMoveEvent
The pointer has moved with respect to the device while the pointer is in contact with the device.
PointerPanZoomEndEvent
The pan/zoom on this pointer has ended.
PointerPanZoomStartEvent
A pan/zoom has begun on this pointer.
PointerPanZoomUpdateEvent
The active pan/zoom on this pointer has updated.
PointerRemovedEvent
The device is no longer tracking the pointer.
PointerRouter
A routing table for PointerEvent events.
PointerScaleEvent
The pointer issued a scale event.
PointerScrollEvent
The pointer issued a scroll event.
PointerScrollInertiaCancelEvent
The pointer issued a scroll-inertia cancel event.
PointerSignalEvent
An event that corresponds to a discrete pointer signal.
PointerSignalResolver
Mediates disputes over which listener should handle pointer signal events when multiple listeners wish to handle those events.
PointerUpEvent
The pointer has stopped making contact with the device.
PolynomialFit
An nth degree polynomial fit to a dataset.
PositionedGestureDetails
An abstract interface representing gesture details that include positional information.
PrimaryPointerGestureRecognizer
A base class for gesture recognizers that track a single primary pointer.
SamplingClock
Class that implements clock used for sampling.
ScaleEndDetails
Details for GestureScaleEndCallback.
ScaleGestureRecognizer
Recognizes a scale gesture.
ScaleStartDetails
Details for GestureScaleStartCallback.
ScaleUpdateDetails
Details for GestureScaleUpdateCallback.
SerialTapCancelDetails
Details for GestureSerialTapCancelCallback, such as the tap count within the series.
SerialTapDownDetails
Details for GestureSerialTapDownCallback, such as the tap count within the series.
SerialTapGestureRecognizer
Recognizes serial taps (taps in a series).
SerialTapUpDetails
Details for GestureSerialTapUpCallback, such as the tap count within the series.
TapAndDragGestureRecognizer
Recognizes taps along with both horizontal and vertical movement.
TapAndHorizontalDragGestureRecognizer
Recognizes taps along with movement in the horizontal direction.
TapAndPanGestureRecognizer
Recognizes taps along with both horizontal and vertical movement.
TapDownDetails
Details for GestureTapDownCallback, such as position.
TapDragDownDetails
Details for GestureTapDragDownCallback, such as the number of consecutive taps.
TapDragEndDetails
Details for GestureTapDragEndCallback, such as the number of consecutive taps.
TapDragStartDetails
Details for GestureTapDragStartCallback, such as the number of consecutive taps.
TapDragUpdateDetails
Details for GestureTapDragUpdateCallback, such as the number of consecutive taps.
TapDragUpDetails
Details for GestureTapDragUpCallback, such as the number of consecutive taps.
TapGestureRecognizer
Recognizes taps.
TapMoveDetails
Details object for callbacks that use GestureTapMoveCallback.
TapUpDetails
Details for GestureTapUpCallback, such as position.
Velocity
A velocity in two dimensions.
VelocityEstimate
A two dimensional velocity estimate.
VelocityTracker
Computes a pointer's velocity based on data from PointerMoveEvents.
VerticalDragGestureRecognizer
Recognizes movement in the vertical direction.
VerticalMultiDragGestureRecognizer
Recognizes movement in the vertical direction on a per-pointer basis.
Enums
DragStartBehavior
Configuration of offset passed to DragStartDetails.
GestureDisposition
Whether the gesture was accepted or rejected.
GestureRecognizerState
The possible states of a PrimaryPointerGestureRecognizer.
MultitouchDragStrategy
Configuration of multi-finger drag strategy on multi-touch devices.
PointerDeviceKind
The kind of pointer device.
Mixins
GestureBinding
A binding for the gesture subsystem.
Constants
kBackMouseButton → const int
The bit of PointerEvent.buttons that corresponds to the back mouse button.
kDefaultMouseScrollToScaleFactor → const double
The default conversion factor when treating mouse scrolling as scaling.
kDefaultTrackpadScrollToScaleFactor → const Offset
The default conversion factor when treating trackpad scrolling as scaling.
kDoubleTapMinTime → const Duration
The minimum time from the end of the first tap to the start of the second tap in a double-tap gesture.
kDoubleTapSlop → const double
Distance between the initial position of the first touch and the start position of a potential second touch for the second touch to be considered the second touch of a double-tap gesture.
kDoubleTapTimeout → const Duration
The maximum time from the start of the first tap to the start of the second tap in a double-tap gesture.
kDoubleTapTouchSlop → const double
The maximum distance that the first touch in a double-tap gesture can travel before deciding that it is not part of a double-tap gesture. DoubleTapGestureRecognizer also restricts the second touch to this distance.
kForwardMouseButton → const int
The bit of PointerEvent.buttons that corresponds to the forward mouse button.
kHoverTapSlop → const double
Maximum distance between the down and up pointers for a tap. (Currently not honored by the TapGestureRecognizer; PrimaryPointerGestureRecognizer, which TapGestureRecognizer inherits from, uses kTouchSlop.)
kHoverTapTimeout → const Duration
Maximum length of time between a tap down and a tap up for the gesture to be considered a tap. (Currently not honored by the TapGestureRecognizer.)
kJumpTapTimeout → const Duration
The maximum time from the start of the first tap to the start of the second tap in a jump-tap gesture.
kLongPressTimeout → const Duration
The time before a long press gesture attempts to win.
kMaxFlingVelocity → const double
Drag gesture fling velocities are clipped to this value.
kMiddleMouseButton → const int
The bit of PointerEvent.buttons that corresponds to the middle mouse button.
kMinFlingVelocity → const double
The minimum velocity for a touch to consider that touch to trigger a fling gesture.
kPagingTouchSlop → const double
The distance a touch has to travel for the framework to be confident that the gesture is a paging gesture. (Currently not used, because paging uses a regular drag gesture, which uses kTouchSlop.)
kPanSlop → const double
The distance a touch has to travel for the framework to be confident that the gesture is a panning gesture.
kPrecisePointerHitSlop → const double
Like kTouchSlop, but for more precise pointers like mice and trackpads.
kPrecisePointerPanSlop → const double
Like kPanSlop, but for more precise pointers like mice and trackpads.
kPrecisePointerScaleSlop → const double
Like kScaleSlop, but for more precise pointers like mice and trackpads.
kPressTimeout → const Duration
The time that must elapse before a tap gesture sends onTapDown, if there's any doubt that the gesture is a tap.
kPrimaryButton → const int
The bit of PointerEvent.buttons that corresponds to a cross-device behavior of "primary operation".
kPrimaryMouseButton → const int
The bit of PointerEvent.buttons that corresponds to the primary mouse button.
kPrimaryStylusButton → const int
The bit of PointerEvent.buttons that corresponds to the primary stylus button.
kScaleSlop → const double
The distance a touch has to travel for the framework to be confident that the gesture is a scale gesture.
kSecondaryButton → const int
The bit of PointerEvent.buttons that corresponds to a cross-device behavior of "secondary operation".
kSecondaryMouseButton → const int
The bit of PointerEvent.buttons that corresponds to the secondary mouse button.
kSecondaryStylusButton → const int
The bit of PointerEvent.buttons that corresponds to the secondary stylus button.
kStylusContact → const int
The bit of PointerEvent.buttons that corresponds to when a stylus contacting the screen.
kTertiaryButton → const int
The bit of PointerEvent.buttons that corresponds to a cross-device behavior of "tertiary operation".
kTouchContact → const int
The bit of PointerEvent.buttons that corresponds to the pointer contacting a touch screen.
kTouchSlop → const double
The distance a touch has to travel for the framework to be confident that the gesture is a scroll gesture, or, inversely, the maximum distance that a touch can travel before the framework becomes confident that it is not a tap.
kWindowTouchSlop → const double
The margin around a dialog, popup menu, or other window-like widget inside which we do not consider a tap to dismiss the widget. (Not currently used.)
kZoomControlsTimeout → const Duration
The time for which zoom controls (e.g. in a map interface) are to be displayed on the screen, from the moment they were last requested.
Properties
debugPrintGestureArenaDiagnostics ↔ bool
Prints information about gesture recognizers and gesture arenas.
getter/setter pair
debugPrintHitTestResults ↔ bool
Whether to print the results of each hit test to the console.
getter/setter pair
debugPrintMouseHoverEvents ↔ bool
Whether to print the details of each mouse hover event to the console.
getter/setter pair
debugPrintRecognizerCallbacksTrace ↔ bool
Logs a message every time a gesture recognizer callback is invoked.
getter/setter pair
debugPrintResamplingMargin ↔ bool
Whether to print the resampling margin to the console.
getter/setter pair
Functions
computeHitSlop(PointerDeviceKind kind, DeviceGestureSettings? settings) → double
Determine the appropriate hit slop pixels based on the kind of pointer.
computePanSlop(PointerDeviceKind kind, DeviceGestureSettings? settings) → double
Determine the appropriate pan slop pixels based on the kind of pointer.
computeScaleSlop(PointerDeviceKind kind) → double
Determine the appropriate scale slop pixels based on the kind of pointer.
debugAssertAllGesturesVarsUnset(String reason) → bool
Returns true if none of the gestures library debug variables have been changed.
isSingleButton(int buttons) → bool
Returns whether buttons contains one and only one button.
nthMouseButton(int number) → int
The bit of PointerEvent.buttons that corresponds to the nth mouse button.
nthStylusButton(int number) → int
The bit of PointerEvent.buttons that corresponds to the nth stylus button.
smallestButton(int buttons) → int
Returns the button of buttons with the smallest integer.
Typedefs
AllowedButtonsFilter = bool Function(int buttons)
Signature for GestureRecognizer.allowedButtonsFilter.
DevicePixelRatioGetter = double? Function(int viewId)
Signature for a callback that returns the device pixel ratio of a FlutterView identified by the provided viewId.
GestureCancelCallback = void Function()
Signature for when the pointer that previously triggered a GestureTapDragDownCallback did not complete.
GestureDoubleTapCallback = void Function()
Signature for callback when the user has tapped the screen at the same location twice in quick succession.
GestureDragCancelCallback = void Function()
Signature for when the pointer that previously triggered a GestureDragDownCallback did not complete.
GestureDragDownCallback = void Function(DragDownDetails details)
Signature for when a pointer has contacted the screen and might begin to move.
GestureDragEndCallback = void Function(DragEndDetails details)
Signature for when a pointer that was previously in contact with the screen and moving is no longer in contact with the screen.
GestureDragStartCallback = void Function(DragStartDetails details)
Signature for when a pointer has contacted the screen and has begun to move.
GestureDragUpdateCallback = void Function(DragUpdateDetails details)
Signature for when a pointer that is in contact with the screen and moving has moved again.
GestureForceInterpolation = double Function(double pressureMin, double pressureMax, double pressure)
Signature used by ForcePressGestureRecognizer for interpolating the raw device pressure to a value in the range [0, 1] given the device's pressure min and pressure max.
GestureForcePressEndCallback = void Function(ForcePressDetails details)
Signature for when the pointer that previously triggered a ForcePressGestureRecognizer.onStart callback is no longer in contact with the screen.
GestureForcePressPeakCallback = void Function(ForcePressDetails details)
Signature used by ForcePressGestureRecognizer for when a pointer that has pressed with at least ForcePressGestureRecognizer.peakPressure.
GestureForcePressStartCallback = void Function(ForcePressDetails details)
Signature used by a ForcePressGestureRecognizer for when a pointer has pressed with at least ForcePressGestureRecognizer.startPressure.
GestureForcePressUpdateCallback = void Function(ForcePressDetails details)
Signature used by ForcePressGestureRecognizer during the frames after the triggering of a ForcePressGestureRecognizer.onStart callback.
GestureLongPressCallback = void Function()
Callback signature for LongPressGestureRecognizer.onLongPress.
GestureLongPressCancelCallback = void Function()
Callback signature for LongPressGestureRecognizer.onLongPressCancel.
GestureLongPressDownCallback = void Function(LongPressDownDetails details)
Callback signature for LongPressGestureRecognizer.onLongPressDown.
GestureLongPressEndCallback = void Function(LongPressEndDetails details)
Callback signature for LongPressGestureRecognizer.onLongPressEnd.
GestureLongPressMoveUpdateCallback = void Function(LongPressMoveUpdateDetails details)
Callback signature for LongPressGestureRecognizer.onLongPressMoveUpdate.
GestureLongPressStartCallback = void Function(LongPressStartDetails details)
Callback signature for LongPressGestureRecognizer.onLongPressStart.
GestureLongPressUpCallback = void Function()
Callback signature for LongPressGestureRecognizer.onLongPressUp.
GestureMultiDragStartCallback = Drag? Function(Offset position)
Signature for when MultiDragGestureRecognizer recognizes the start of a drag gesture.
GestureMultiTapCallback = void Function(int pointer)
Signature used by MultiTapGestureRecognizer for when a tap has occurred.
GestureMultiTapCancelCallback = void Function(int pointer)
Signature for when the pointer that previously triggered a GestureMultiTapDownCallback will not end up causing a tap.
GestureMultiTapDownCallback = void Function(int pointer, TapDownDetails details)
Signature used by MultiTapGestureRecognizer for when a pointer that might cause a tap has contacted the screen at a particular location.
GestureMultiTapUpCallback = void Function(int pointer, TapUpDetails details)
Signature used by MultiTapGestureRecognizer for when a pointer that will trigger a tap has stopped contacting the screen at a particular location.
GestureScaleEndCallback = void Function(ScaleEndDetails details)
Signature for when the pointers are no longer in contact with the screen.
GestureScaleStartCallback = void Function(ScaleStartDetails details)
Signature for when the pointers in contact with the screen have established a focal point and initial scale of 1.0.
GestureScaleUpdateCallback = void Function(ScaleUpdateDetails details)
Signature for when the pointers in contact with the screen have indicated a new focal point and/or scale.
GestureSerialTapCancelCallback = void Function(SerialTapCancelDetails details)
Signature used by SerialTapGestureRecognizer.onSerialTapCancel for when a pointer that previously triggered a GestureSerialTapDownCallback will not end up completing the serial tap.
GestureSerialTapDownCallback = void Function(SerialTapDownDetails details)
Signature used by SerialTapGestureRecognizer.onSerialTapDown for when a pointer that might cause a serial tap has contacted the screen at a particular location.
GestureSerialTapUpCallback = void Function(SerialTapUpDetails details)
Signature used by SerialTapGestureRecognizer.onSerialTapUp for when a pointer that will trigger a serial tap has stopped contacting the screen.
GestureTapCallback = void Function()
Signature for when a tap has occurred.
GestureTapCancelCallback = void Function()
Signature for when the pointer that previously triggered a GestureTapDownCallback will not end up causing a tap.
GestureTapDownCallback = void Function(TapDownDetails details)
Signature for when a pointer that might cause a tap has contacted the screen.
GestureTapDragDownCallback = void Function(TapDragDownDetails details)
Signature for when a pointer that might cause a tap has contacted the screen.
GestureTapDragEndCallback = void Function(TapDragEndDetails endDetails)
Signature for when a pointer that was previously in contact with the screen and moving is no longer in contact with the screen.
GestureTapDragStartCallback = void Function(TapDragStartDetails details)
Signature for when a pointer has contacted the screen and has begun to move.
GestureTapDragUpCallback = void Function(TapDragUpDetails details)
Signature for when a pointer that will trigger a tap has stopped contacting the screen.
GestureTapDragUpdateCallback = void Function(TapDragUpdateDetails details)
Signature for when a pointer that is in contact with the screen and moving has moved again.
GestureTapMoveCallback = void Function(TapMoveDetails details)
Signature for when a pointer that triggered a tap has moved.
GestureTapUpCallback = void Function(TapUpDetails details)
Signature for when a pointer that will trigger a tap has stopped contacting the screen.
GestureVelocityTrackerBuilder = VelocityTracker Function(PointerEvent event)
Signature for a function that builds a VelocityTracker.
HandleEventCallback = void Function(PointerEvent event)
A callback used by PointerEventResampler.sample and PointerEventResampler.stop to process a resampled event.
InformationCollector = Iterable<DiagnosticsNode> Function()
Signature for FlutterErrorDetails.informationCollector callback and other callbacks that collect information describing an error.
PointerRoute = void Function(PointerEvent event)
A callback that receives a PointerEvent
PointerSignalResolvedCallback = void Function(PointerSignalEvent event)
The callback to register with a PointerSignalResolver to express interest in a pointer signal event.
RecognizerCallback<T> = T Function()
Generic signature for callbacks passed to GestureRecognizer.invokeCallback. This allows the GestureRecognizer.invokeCallback mechanism to be generically used with anonymous functions that return objects of particular types.
RespondPointerEventCallback = void Function({required bool allowPlatformDefault})
A function that implements the PointerSignalEvent.respond method.