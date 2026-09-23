# ADR-0035: Lifecycle consolidation and the frames-enabled re-enable leg

- **Status:** Accepted
- **Date:** 2026-07-18
- **Related:** ADR-0027 (realm ownership), ADR-0058 (pacing; the backgrounded pump)

## Context

The workspace had three app-lifecycle representations at once: `flui_scheduler`'s
`AppLifecycleState` (a faithful port, the only one wired to `frames_enabled`), a duplicate enum
in `flui-view` that nothing fed, and a `flui-platform` state machine with different variants
(`Starting`/`Active`/`Background`/`Terminating`) that the runner drove and nothing read. Platform
quit and window focus updated a state machine that could not affect frame scheduling.

The canonical handler also missed a Flutter leg: it flipped `frames_enabled` back on for
`Resumed`/`Inactive` but never requested a frame, so an app returning from `Hidden`/`Paused`
looked frozen until unrelated input woke the loop. Flutter's `_setFramesEnabledState(true)`
calls `scheduleFrame()` on that edge.

## Decision

### 1. One lifecycle type: `flui_scheduler::AppLifecycleState`

It is canonical because it is the one tied to behavior. `flui-view` re-exports it; the
`flui-platform` lifecycle traits and the app-binding lifecycle state machine are deleted, not
deprecated. Two names for one concept had already drifted, and would again.
`should_schedule_frame()` is an alias of `frames_enabled()` — one source of truth.
`should_run_animations()` is deleted: it had no caller, and Flutter gates tickers through
`TickerMode` in the widget tree, not on lifecycle state in the scheduler.

### 2. The re-enable leg

`handle_app_lifecycle_state_change` swaps `frames_enabled` atomically and calls
`request_frame()` only on the false → true edge; repeated states, `Resumed` ↔ `Inactive` and the
enabled → disabled edge schedule nothing. FLUI keeps no retained scene to re-present on resume,
so the re-enable edge also re-dirties the presentation's root; the first frame after a long hide
paints fresh content instead of running idle.

### 3. Frames disabled still pump async work

The per-wake gate is `wake_action(frames_enabled, dirty, frame_scheduled) -> Render | PumpAsync
| Skip`. `PumpAsync` (frames disabled) clears the frame-scheduled latch with
`Scheduler::finish_async_pump()` and then runs `drive_async_tasks()` — no begin/draw frame, no
tickers, no pipeline, no present. The clear comes **first**, mirroring `handle_begin_frame`:
without it a later independent `Waker::wake()` finds the latch already set and never wakes the
loop; clearing *after* instead would erase a task's synchronous self-wake during the pump. How
the backgrounded pump is paced is ADR-0058's decision.

### 4. Lifecycle facts are per presentation; the realm aggregates

Each presentation owns its native visibility/focus snapshot and its last delivered local
state. A realm derives the scheduler state from its live presentations: any visible and focused
→ `Resumed`; otherwise any visible → `Inactive`; otherwise `Hidden`; an empty forest →
`Detached`. Keyboard routing history does not manufacture native focus.

- Local steps follow `lifecycle_ladder(old, new)`, a port of Flutter's
  `ServicesBinding._generateStateTransitions` walking `dart:ui`'s declaration order (`detached,
  resumed, inactive, hidden, paused`) — `Detached` is first, so leaving it is one forward step.
  An unobserved presentation is `None`, and its first notification goes straight to the target,
  as in Flutter's nullable-previous-state branch.
- Scheduler listeners see the realm aggregate, not the local ladder. A paused or hidden host
  cannot transiently enable frames or attach resources while a presentation synchronizes.
- Local facts and the next ladder step commit before user callbacks; input cancellation
  precedes lifecycle callbacks; binding observers see committed local and aggregate state.
  Restoring a presentation re-dirties its own root and wakes a frame. Focus transfer cancels
  the former presentation's active pointer sequences at once.
- Observed `Detached` is reversible and only rejects input. Explicit stopping is a separate
  terminal intent. Closing a presentation removes it from the aggregate, delivers `Detached`
  while its tree is alive, then disposes it; an observer panic cannot skip removal.
- Android maps its single active-status signal to the ladder: `false` walks to `Paused`, `true`
  back to `Resumed`.

### 5. Platform quit reaches every realm

A quit belongs to the application loop, not the primary window. The desktop quit callback visits
every installed realm once, laddering each live presentation to `Detached`. Quit closes
secondary-window admission immediately; notification waits until an active dispatch restores its
checked-out state; completions and callbacks carry their loop's identity, so one from an earlier
loop cannot affect a later one. A panicking observer cannot skip sibling realms: the first
panic resumes after restoration and notification. Window closure and application termination
stay distinct, as in AppKit's `applicationShouldTerminateAfterLastWindowClosed`.

### 6. Lifecycle subscriptions are weak and scoped

`LifecycleContext::lifecycle_handle()` (ADR-0078) returns an optional owner-local capability.
The presentation binding owns the source; build owners and contexts hold weak handles.
`subscribe` atomically returns the current observation and an RAII token, without invoking the
callback before the token can be stored; dropping the token cancels callbacks not yet started,
even within the same dispatch. Delivery is FIFO outside source borrows; a panicking callback
cannot skip siblings. Begin-close fences new subscriptions; the final `Detached` is delivered
before the source is invalidated and widgets are disposed. This follows GPUI's scoped
subscriptions rather than Flutter's `AppLifecycleListener.dispose`.

## Flutter divergences

- **Async work is polled by the frame loop**, not by an always-running event loop; the
  `PumpAsync` arm is what keeps futures advancing while frames are off.
- **No retained scene**: resume re-dirties the root instead of re-presenting the last frame.
- **Lifecycle is per presentation with a realm aggregate**, not one process-wide stream;
  scheduler listeners see the aggregate.

## Not implemented

A native-Windows visibility signal (only the winit backend reports occlusion); Windows minimize
mapped through `(visible, focused)`; a web `visibilitychange` signal; an Android transport that
separates pause/resume from focus; negotiated exit (`onExitRequested`). Wayland occlusion
depends on the compositor sending xdg-shell `suspended`; where it does not, a window counts as
always visible.

## Alternatives rejected

- **Make the platform state machine canonical.** A fourth representation, with names and a
  transition model that do not match Flutter's.
- **Gate `frames_enabled` inside `drive_frame`.** Would also stop the async driver, starving
  in-flight futures the moment frames disable.
- **Keep running frames and skip only presentation.** Burns build/layout/paint for a window
  nobody can see.
