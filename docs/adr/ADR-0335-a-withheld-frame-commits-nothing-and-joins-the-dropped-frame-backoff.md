# ADR-0335: A withheld frame commits nothing and joins the dropped-frame backoff

- **Status:** Accepted (2026-11-15)
- **Date:** 2026-11-15
- **Supersedes:** [ADR-0068](ADR-0068-frame-disposition-replaces-the-presented-bit.md) §3 in part
  (that the frame tail commits a withheld frame's tree) and §4 in part (that past the budget the
  frame parks, and that the park clears the withheld streak)
- **Related:** [ADR-0305](ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)
  §4 (input regions publish with a presented frame), [ADR-0334](ADR-0334-a-failed-frame-is-painted-again-after-the-dropped-frame-backoff.md)
  (the dropped-frame backoff for a failed frame), [ADR-0304](ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md)
  §7 (a withheld frame is retried from the held version)

## Context and problem statement

`SubmitVerdict::NotShown` is a frame that rendered and could not be shown: the surface was
withdrawn or occluded. ADR-0068 §3 lets the frame tail commit the tree anyway, as `NoPresent`
does, and retries the frame on the next display period; §4 caps the retries at
`MAX_NOT_SHOWN_RETRIES` (128) and then parks, clearing the streak so a later frame opens a fresh
budget.

The commit says the screen shows a tree it does not. It publishes the frame's input regions and
marks the presentation `Committed`, so pointer input that was held for the commit replays against
that tree, and input that arrives next dispatches against it. ADR-0305 §4 keeps input regions for a
`Presented` frame, so hit testing matches the pixels on screen. The two records disagree, and the
code follows ADR-0068.

Committing nothing is not enough on its own. Past the budget the frame parks, and nothing wakes
it: pointer input that arrives is held until a commit, and a hold has no wake of its own. A window
whose surface stays withdrawn for more than 128 attempts and then returns (a window on an inactive
Space, a display asleep) would hold every pointer event until an unrelated frame happens to run.

How does a presentation whose frame was withheld keep hit testing on the pixels it shows, without
spinning and without stranding its input?

## Decision drivers

- Hit testing matches the pixels on screen: a frame that was not shown commits nothing (ADR-0305 §4).
- Accepted work is delivered: input held for a commit needs a frame that commits.
- No spin: a surface that stays withdrawn costs a bounded number of attempts per second.
- One retry pacing for every dropped frame: the backoff of ADR-0267 and ADR-0334.
- A withdrawal of a few frames (AppKit's cold start withholds about 12 while `occlusionState`
  reports the window visible) is ridden out at the display rate.

## Considered options

1. A withheld frame commits nothing; the budget's attempts follow each other, and each attempt past
   it waits out the dropped-frame backoff.
2. A withheld frame commits nothing; the frame parks past the budget, and a held pointer event asks
   for a frame.
3. A withheld frame commits nothing; every attempt waits out the backoff, with no budget.
4. Keep ADR-0068: the withheld frame commits its tree.

## Decision

Chosen option: 1, because it is the only option that keeps hit testing on the pixels shown,
delivers held input once the surface returns, bounds the attempts, and keeps the cold-start
transient at the display rate.

- **§1.** A frame that settles `NotShown` commits nothing: the tree revision stays uncommitted, the
  input regions of the last presented frame stay in force, pointer input is held for the frame that
  shows the tree, its layer reads park for a later scene, and its frame-aligned haptics wait. It
  does not end the dropped-frame streak (ADR-0334 §2). An inline `NotShown` supersedes none of the
  frames still queued, which their settlements may still show.
- **§2.** The first `MAX_NOT_SHOWN_RETRIES` consecutive withheld attempts are retried on the next
  display period. Each attempt past them counts into the dropped-frame streak (`note_frame_failure`)
  and waits out the dropped-frame backoff (`failure_retry_delay`, doubling from one display period up
  to `FAILURE_RETRY_CAP`) before it repaints. The withheld streak stays above the budget until a frame
  presents or finds no damage, so each attempt after the budget waits too, rather than opening a new
  burst. A key or button press ends the wait early, as for any dropped frame.
- **§3.** Past the budget, a settlement of an older tree, a frame still pending from this tree or a
  later one, and a given-up device (ADR-0267) arm no wait: the current tree is owed by its own frame.
- **§4.** The retry repaints in full until ADR-0304 §7's retry from the held version is built; the
  wake and the repaint are armed before the trace, whose subscriber is user code.

### Consequences

- Good, because pointer input never hit-tests a tree the screen did not show, and held input
  replays on the commit of the frame that shows it.
- Good, because a window whose surface returns after a long withdrawal shows its tree within one
  second, and its held input replays then, with no unrelated frame needed.
- Bad, because a surface that never returns, while frames stay enabled, costs one paint per second
  after the budget, where ADR-0068 parked it. The lifecycle gate stops frames for a window the
  platform reports hidden, so this is the case where occlusion and the drawable disagree.
- Bad, because pointer input waits while a surface withholds frames, on cold start for about 132 ms.
  Until then it was dispatched against the tree the screen did not show.
- Neutral, because the cold-start transient keeps its display-rate retries.

### Confirmation

Tests in `crates/flui-runtime/src/ui_runtime/frame_commit_state_tests.rs`:

- `a_withheld_frame_commits_nothing_and_holds_input_for_the_frame_that_shows_it` (§1): the tree
  stays uncommitted after a withheld frame, a press is held, and the frame that presents commits and
  replays it;
- `the_withheld_retry_is_bounded_and_then_waits_out_the_backoff` (§2): the budget's attempts follow
  each other, each attempt past it waits out a growing backoff, a press ends the wait, and the frame
  that presents replays the press and ends the streak;
- `a_parked_withheld_settlement_leaves_the_failed_frames_retry` (§3): a withheld settlement of an older
  tree past the budget leaves the wait a newer failed frame armed;
- `a_subscriber_that_panics_on_the_trace_of_a_withheld_frame_keeps_the_retry` (§4): a subscriber's
  panic on the withheld frame's trace keeps the retry.

## Pros and cons of the options

### 1. Commit nothing, budget, then the backoff

- Good, because it reuses the wait, the press bypass and the cap a failed frame already has.
- Good, because the transient keeps its display-rate retries.
- Bad, because a surface that never returns costs one paint per second.

### 2. Commit nothing, park, a held pointer event asks for a frame

- Good, because a parked surface costs nothing until input arrives.
- Bad, because each held pointer move would ask for an attempt, so a withdrawn surface repaints at
  the pointer's rate, and a window with no input still never shows its tree when the surface returns.

### 3. Commit nothing, the backoff from the first attempt

- Good, because one rule covers every attempt.
- Bad, because the cold-start transient of about 12 frames waits through a doubling backoff, and the
  first frame of a window shows later.

### 4. Keep ADR-0068

- Good, because nothing changes.
- Bad, because pointer input hit-tests a tree the screen does not show, against ADR-0305 §4.

## More information

ADR-0068 §1 and §2 (the three-state disposition, exhaustive) and its measurements stand; §3's
repaint half is ADR-0304 §7's, and §3's two-part retention (the wake and the repaint) stands.
