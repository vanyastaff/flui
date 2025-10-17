scheduler library
The Flutter Scheduler library.

To use, import package:flutter/scheduler.dart.

This library is responsible for scheduler frame callbacks, and tasks at given priorities.

The library makes sure that tasks are only run when appropriate. For example, an idle-task is only executed when no animation is running.

Classes
DiagnosticsNode
Defines diagnostics data for a value.
FrameTiming
Time-related performance metrics of a frame.
PerformanceModeRequestHandle
An opaque handle that keeps a request for DartPerformanceMode active until disposed.
Priority
A task priority, as passed to SchedulerBinding.scheduleTask.
Ticker
Calls its callback once per animation frame, when enabled.
TickerFuture
An object representing an ongoing Ticker sequence.
TickerProvider
An interface implemented by classes that can vend Ticker objects.
Enums
AppLifecycleState
States that an application can be in once it is running.
SchedulerPhase
The various phases that a SchedulerBinding goes through during SchedulerBinding.handleBeginFrame.
SchedulerServiceExtensions
Service extension constants for the scheduler library.
Mixins
SchedulerBinding
Scheduler for running the following:
Properties
debugPrintBeginFrameBanner ↔ bool
Print a banner at the beginning of each frame.
getter/setter pair
debugPrintEndFrameBanner ↔ bool
Print a banner at the end of each frame.
getter/setter pair
debugPrintScheduleFrameStacks ↔ bool
Log the call stacks that cause a frame to be scheduled.
getter/setter pair
debugTracePostFrameCallbacks ↔ bool
Record timeline trace events for post-frame callbacks.
getter/setter pair
timeDilation ↔ double
Slows down animations by this factor to help in development.
getter/setter pair
Functions
debugAssertAllSchedulerVarsUnset(String reason) → bool
Returns true if none of the scheduler library debug variables have been changed.
defaultSchedulingStrategy({required int priority, required SchedulerBinding scheduler}) → bool
The default SchedulingStrategy for SchedulerBinding.schedulingStrategy.
Typedefs
FrameCallback = void Function(Duration timeStamp)
Signature for frame-related callbacks from the scheduler.
SchedulingStrategy = bool Function({required int priority, required SchedulerBinding scheduler})
Signature for the SchedulerBinding.schedulingStrategy callback. Called whenever the system needs to decide whether a task at a given priority needs to be run.
TaskCallback<T> = FutureOr<T> Function()
Signature for SchedulerBinding.scheduleTask callbacks.
TickerCallback = void Function(Duration elapsed)
Signature for the callback passed to the Ticker class's constructor.
TimingsCallback = void Function(List<FrameTiming> timings)
Signature for PlatformDispatcher.onReportTimings.
VoidCallback = void Function()
Signature of callbacks that have no arguments and return no data.
Exceptions / Errors
TickerCanceled
Exception thrown by Ticker objects on the TickerFuture.orCancel future when the ticker is canceled.
