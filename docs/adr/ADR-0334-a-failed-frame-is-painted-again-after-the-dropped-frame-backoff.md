# ADR-0334: A failed frame is painted again after the dropped-frame backoff

- **Status:** Accepted (2026-11-14)
- **Date:** 2026-11-14
- **Supersedes:** [ADR-0048](ADR-0048-frame-transaction-boundary.md) in part (that `Failed`
  does not retry, and that a clean segment resets the dropped-frame streak before its painted
  scene is submitted)
- **Related:** [ADR-0267](ADR-0267-a-lost-gpu-device-is-rebuilt-on-a-backoff-and-given-up.md)
  (the dropped-frame backoff and a given-up device), [ADR-0305](ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)
  §4 (a queued frame settles by its stamp)

## Context and problem statement

`SubmitVerdict::Failed` is a frame no retry fixes: a generic render error, or a submit the raster
side refused. ADR-0048 lets it drain telemetry and arm nothing. The frame commits nothing either,
so the presentation stays `Uncommitted`, and pointer input that arrives is held until a commit
(`crates/flui-runtime/src/ui_runtime/input.rs`, `should_hold_pointer`). Nothing then asks for a
frame. An application with no animation keeps its last frame on screen, and holds every pointer
event, until an unrelated frame happens to run. This happens inline today, and queued frames
(ADR-0305 §4) add a second way in: a settlement that commits an older tree.

Retrying at once is no answer. A refused submit fails the same way next frame. The dropped-frame
streak that paces retries (ADR-0267) is reset by every clean segment, and a frame that fails at
submit had a clean segment, so its retries would run at the display rate forever.

How does a presentation whose last frame failed get its tree on screen, without spinning?

## Decision drivers

- Accepted work is delivered: input held for a commit needs a frame that commits.
- No spin: a frame that keeps failing costs at most one attempt per backoff interval.
- One retry pacing for every dropped frame: the backoff ADR-0267 gives a lost device.
- A given-up device holds its frames (ADR-0267), whatever the verdict.

## Considered options

1. Retry a failed frame after the dropped-frame backoff, with the streak kept until a frame
   commits the current tree.
2. Retry at once, on the next display period.
3. Release held input when a frame fails, against the tree that was not shown.
4. Keep ADR-0048: no retry.

## Decision

Chosen option: 1, because it is the only option that both delivers held input and bounds the
attempts.

- **§1.** A frame that settles `Failed` paints its presentation's tree again after the
  dropped-frame backoff (`failure_retry_delay`, doubling from one display period up to
  `FAILURE_RETRY_CAP`). This holds unless a frame still pending was painted from that tree or a
  later one, or the device was given up. The submit consumed the paint demand, so the retry is a
  full repaint. The frame counts into the dropped-frame streak (`note_frame_failure`) without a
  `FrameFailureReport`, since the sink already logged the cause. A key or button press ends the
  wait early, as for any dropped frame.
- **§2.** A clean segment that painted a scene does not reset the streak. The scene's verdict
  does, once the frame commits the current tree (`Presented`, `NoPresent`, `NotShown`). A clean
  segment that painted nothing still resets it. An older queued frame's commit leaves the streak,
  and the wait a newer failed frame armed.
- **§3.** A settlement that commits an older tree, with no pending frame painted from the current
  one, asks for a repaint, whether input is held or not (`owe_current_tree`).

### Consequences

- Good, because an application whose frame failed shows its tree within the backoff, and the input
  it held replays on that commit.
- Good, because a sink that refuses every frame costs one attempt per second at the cap.
- Bad, because a failure that is permanent (a detached slot) is retried once per second until the
  presentation goes. The cost is one paint, and the frame driver's wait keeps the host asleep in
  between.
- Neutral, because `consecutive_failures` in a later `FrameFailureReport` now counts failed
  submits as well as dropped segments.

### Confirmation

Tests in `crates/flui-runtime/src/ui_runtime/frame_commit_state_tests.rs`:

- `a_failed_frame_is_painted_again_after_a_growing_backoff` (§1, §2): each attempt submits again,
  the wait doubles, and a frame that presents clears the streak and the wait;
- `held_input_behind_a_failed_frame_asks_for_the_frame_that_shows_the_tree` (§1, §2): the input
  held behind a failed frame replays on the retried frame's commit, and the older frame's
  settlement leaves the wait standing;
- `a_parked_withheld_settlement_leaves_the_failed_frames_retry` (§2): a parked `NotShown`
  settlement of an older frame leaves the wait standing.

## Pros and cons of the options

### 1. Retry after the dropped-frame backoff

- Good, because it reuses the wait, the press bypass and the cap that a dropped segment and a lost
  device already have.
- Bad, because the streak's reset moves from the segment to the verdict, which ADR-0048 set.

### 2. Retry at once

- Good, because it recovers a frame that failed once within one display period.
- Bad, because a sink that refuses every frame makes the presentation render at the display rate
  forever.

### 3. Release held input on failure

- Good, because no frame is spent.
- Bad, because the input hit-tests a tree the screen does not show, which is the mismatch the hold
  exists to prevent, and the stale frame stays on screen.

### 4. Keep ADR-0048

- Good, because nothing changes.
- Bad, because the application keeps its last frame and holds its input until an unrelated frame
  runs.

## More information

`docs/gaps.md` (the ADR-0305 line) lists what a settlement still does not owe: a frame answered
`NotShown` commits its tree, and the hold covers pointer events only.
