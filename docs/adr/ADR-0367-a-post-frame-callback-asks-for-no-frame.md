# ADR-0367: A post-frame callback asks for no frame

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0183](ADR-0183-explicit-numeric-text-sizing.md) in part (that scheduling
  through a retained handle between segments creates completion demand)
- **Related:** [ADR-0030](ADR-0030-platform-text-input-ime-capability.md) (the IME cursor-area
  loop), [ADR-0334](ADR-0334-a-failed-frame-is-painted-again-after-the-dropped-frame-backoff.md)
  (the re-armed demand of a dropped frame's callbacks)

## Context and problem statement

A presentation's `PostFrameHandle::schedule` made between two of its segments also set the
scope's completion demand and asked the scheduler for a frame (ADR-0183). A callback was never
left waiting, but registering one cost a frame. A loop that re-registers from its own callback
then costs a frame on every vsync: `EditableText`'s IME cursor-area loop (ADR-0030) runs while a
field holds the focus, so a focused field with nothing changing kept its presentation producing
completion-only frames forever. On a device that is an idle frame loop that drains the battery;
under `Tester` the application never settles, so `pump_and_settle` fails in any test that types.

Flutter's `addPostFrameCallback` schedules no frame, and its cursor-area loop
(`_schedulePeriodicPostFrameCallbacks`) re-registers each frame at no cost while nothing else
asks for one; a caller that needs a frame calls `scheduleFrame`.

Should registering a post-frame callback ask for a frame?

## Decision drivers

- An idle application runs no frame: work that waits for a frame costs nothing until one runs.
- Accepted work is delivered: a callback that needs a frame nothing else asks for still gets one.
- One registration form: a second, non-demanding `schedule` would leave the costly one as the
  default that new code copies.
- Flutter's shape where no Rust reason departs from it.

## Considered options

1. Registering asks for no frame; a caller that needs one asks with `PostFrameHandle::schedule_frame`.
2. Keep the demand, and give the cursor-area loop a second registration that asks for none.
3. Keep the demand, and make the cursor-area loop re-register only when its rect changed.

## Decision

Option 1.

- `PostFrameHandle::schedule` and `schedule_or_return` queue the callback for the next frame that
  completes its segment and ask for no frame. The one exception is a registration made while a
  retained segment waits to resume (suspended or servicing): resuming completes only the retained
  segment, so the registration asks for the successor it joins.
- `PostFrameHandle::schedule_frame` asks for a frame of the handle's presentation, as Flutter's
  `scheduleFrame`: it sets the scope's completion demand (outside a running segment) and requests
  a frame, so a clean presentation completes a segment without rebuilding or submitting a scene.
  An unscoped handle asks its scheduler for a frame.
- Completion demand otherwise comes only from a frame that aborted with callbacks waiting
  (ADR-0334), unchanged.
- A caller whose callback waits on a frame nothing else asks for calls `schedule_frame` after
  `schedule`: a selection menu's item hides the menu after the frame, a refresh indicator
  performs its refresh after the release frame, a swiped row and a `Dismissible` deliver after
  their rest, a back gesture's disposal reports its end, and the IME cursor-area loop asks once
  when it starts. Registrations made inside a frame, or from an operation that already dirties
  the tree, ask for nothing.

### Consequences

- Good, because a focused text field leaves its presentation idle, and `Tester::pump_and_settle`
  settles after typing.
- Good, because a per-frame loop costs nothing while nothing else runs frames.
- Bad, because a caller that forgets `schedule_frame` waits for the next frame something else
  asks for; the callers that need one were audited when this landed.

### Confirmation

- `a_focused_text_field_settles_after_typing` and `a_focused_text_field_runs_no_frame_while_idle`
  (flui-testing) fail when registering asks for a frame.
- `a_requested_completion_survives_wake_failure_and_host_clearing` (flui-scheduler) checks that
  registering outside a retained segment asks for no frame and leaves no demand, and that
  `schedule_frame` does.
- `a_registration_while_a_segment_is_retained_asks_for_its_successor` (flui-scheduler) checks
  that a registration while suspended or servicing asks for the successor it joins.
- `a_clean_presentation_delivers_successor_callbacks_without_rebuilding` (flui-runtime) checks
  that a registered callback alone leaves the presentation idle and runs after the requested
  frame without a rebuild or a submitted scene.

## Pros and cons of the options

### 1. Registering asks for no frame

- Good, because it is Flutter's split, and one registration form remains.
- Bad, because every caller that relied on the implicit frame needs an explicit request.

### 2. A second, non-demanding registration

- Good, because no existing caller changes.
- Bad, because two forms of one primitive, with the costly one the default.

### 3. Re-register only on a changed rect

- Bad, because the loop can only learn the rect moved by reading it after a frame, so it must run
  after every frame; a moved ancestor would go unseen.

## More information

The scheduler's rule is in `crates/flui-scheduler/ARCHITECTURE.md`, the runtime's in
`crates/flui-runtime/ARCHITECTURE.md`.
