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

`SubmitVerdict::Failed` is a frame no retry fixes this frame. Before this record it covered two
kinds: a generic render error or a frame the raster side could not take, which a later attempt may
get through; and a window the host knows cannot show a frame until it changes (a surface or size
the device refuses, a renderer that panicked on the scene), whose next change asks for a frame.
ADR-0048 lets `Failed` drain telemetry and arm nothing. The frame commits nothing either, so the
presentation stays `Uncommitted`, and pointer input that arrives is held until a commit
(`crates/flui-runtime/src/ui_runtime/input.rs`, `should_hold_pointer`). For the first kind
nothing then asks for a frame. An application with no animation keeps its last frame on screen,
and holds every pointer event, until an unrelated frame happens to run. This happens inline
today, and queued frames (ADR-0305 §4) add a second way in: a settlement that commits an older
tree.

Retrying at once is no answer. A frame that failed may fail the same way next frame. The
dropped-frame streak that paces retries (ADR-0267) is reset by every clean segment, and a frame
that fails at submit had a clean segment, so its retries would run at the display rate forever.
Retrying the second kind at all is waste: it meets the same refusal until the window changes.

How does a presentation whose last frame failed get its tree on screen, without spinning?

## Decision drivers

- Accepted work is delivered: input held for a commit needs a frame that commits.
- No spin: a frame that keeps failing costs at most one attempt per backoff interval.
- One retry pacing for every dropped frame: the backoff ADR-0267 gives a lost device.
- No attempt the host knows is refused: a window that waits for its next change costs nothing.
- A given-up device holds its frames (ADR-0267), whatever the verdict.

## Considered options

1. Retry a failed frame after the dropped-frame backoff, with the streak kept until a frame
   commits the current tree, and a separate `Refused` verdict for a window that waits for its
   next change.
2. Option 1 without `Refused`: every failed frame, refused windows included, retries on the
   backoff.
3. Retry at once, on the next display period.
4. Release held input when a frame fails, against the tree that was not shown.
5. Keep ADR-0048: no retry.

## Decision

Chosen option: 1, because it is the only option that delivers held input, bounds the attempts,
and spends none on a window the host knows is refused.

- **§1.** A frame that settles `Failed` paints its presentation's tree again after the
  dropped-frame backoff (`failure_retry_delay`, doubling from one display period up to
  `FAILURE_RETRY_CAP`). This holds unless a frame still pending was painted from that tree or a
  later one, or the device was given up. The submit consumed the paint demand, so the retry is a
  full repaint. The frame counts into the dropped-frame streak (`note_frame_failure`) without a
  `FrameFailureReport`, since the sink already logged the cause. A key or button press ends the
  wait early, as for any dropped frame.
- **§2.** A clean segment that painted a scene does not reset the streak. The scene's verdict
  does, once the frame commits the current tree (`Presented`, `NoPresent`; a `NotShown` frame
  commits nothing, ADR-0335 §1). A clean
  segment that painted nothing still resets it, and so does one whose scene reaches no sink. An
  older queued frame's commit leaves the streak, and the wait a newer failed frame armed.
- **§3.** A settlement that commits an older tree, with no pending frame painted from the current
  one, asks for a repaint, whether input is held or not (`owe_current_tree`), unless the device
  was given up.
- **§4.** A host whose window cannot show a frame until the window changes answers
  `SubmitVerdict::Refused`: the surface or the size is one the device refuses, or the renderer
  panicked on this scene. The runtime records the dropped frame, drains its telemetry, commits
  nothing and arms nothing; the window's next change (a size the device takes, a new surface, a
  rebuilt device, a rebuild of the tree) asks for the frame. The scene keeps the refused version,
  and a texture frame does not publish it again (ADR-0304 §8) while it is the current version,
  until the raster side shows it or a later version of its lineage (a republish of it in flight
  that the window then takes): the texture's frames wait in its registry, the newest per id.
  `flui-app`'s raster host answers it, and `Failed` stays for what a later attempt may get
  through.

### Consequences

- Good, because an application whose frame failed shows its tree within the backoff, and the input
  it held replays on that commit.
- Good, because a sink that fails every frame costs one attempt per second at the cap, and a
  refused window costs none.
- Bad, because a failure that is permanent and that the host cannot name (a detached slot) is
  retried once per second until the presentation goes. The cost is one paint, and the frame
  driver's wait keeps the host asleep in between, except while the presentation has completion
  demand (a callback its dropped frame left waiting): the commit anchor re-arms it at every owner turn of the
  wait ([gaps](../gaps.md#the-web-never-wakes-when-a-dropped-frames-backoff-ends)).
- Bad, because a refused window holds its pointer input until it changes, as before this record.
  A renderer whose panic would not repeat leaves its window on the last frame until then, since
  the host cannot tell such a panic from one that would.
- Neutral, because `consecutive_failures` in a later `FrameFailureReport` now counts failed
  submits as well as dropped segments, and frames lost with their device count 1, 2, 3… instead
  of 1 each time: their painted segments no longer reset the streak, so their wait doubles to the
  cap as ADR-0267 §1 asks of every dropped frame.
- Neutral, because a scene that reaches no sink (a first frame still withheld, a scene another
  presentation's scene replaced in the same pump) resets the streak at its clean segment, as a
  segment that painted nothing does.

### Confirmation

Tests in `crates/flui-runtime/src/ui_runtime/frame_commit_state_tests.rs`:

- `a_failed_frame_is_painted_again_after_a_growing_backoff` (§1, §2): each attempt submits again,
  the wait doubles, and a frame that presents clears the streak and the wait;
- `held_input_behind_a_failed_frame_asks_for_the_frame_that_shows_the_tree` (§1, §2): the input
  held behind a failed frame replays on the retried frame's commit, and the older frame's
  settlement leaves the wait standing;
- `a_withheld_settlement_of_an_older_tree_leaves_the_failed_frames_retry` (§2): a withheld (`NotShown`)
  settlement of an older frame leaves the wait standing;
- `a_settlement_of_an_older_tree_asks_for_the_current_one` (§3): a settlement of an older tree
  with no input held asks for a repaint, and asks for none while a frame from the current tree is
  pending or while the device is given up.

`failed_frame::a_failed_frame_is_painted_again_after_a_backoff_that_grows` in
`crates/flui-runtime/tests/contracts/failed_frame.rs` pins §1 and §2 through `UiRuntime::pump`: no frame is submitted
before the deadline `next_wake` reports, each retry submits once, the waits grow, and a presented
frame ends the wait. In the same file,
`a_dropped_frame_whose_wake_panics_still_waits_out_the_backoff` pins that a `Failed` or
`DeviceLost` frame whose platform wake panics keeps its backoff and records its telemetry, and
`a_refused_frame_waits_for_its_windows_next_change` pins §4: no wait, no frame, until the host
asks again. `redraw_wake_routing::a_tail_panic_whose_wake_panics_keeps_its_report_and_backoff`
(`crates/flui-runtime/src/ui_runtime/tests/`) pins the same order for a segment that panics in its
tail.

`crates/flui-app/tests/runner_close.rs` pins §4 through the raster host:
`a_window_past_the_device_limit_waits_for_a_size_it_takes`,
`a_window_over_the_rebuilt_device_limit_waits_for_a_size_it_takes` and
`a_window_whose_new_surface_is_refused_waits_for_its_next_change` arm no wait while the window is
refused and draw on its next change, and `a_resize_that_keeps_panicking_is_not_retried_forever`
stops after the one retry of a panicked resize. `flui-runtime`'s
`external_textures::a_refused_window_costs_no_texture_frame` and `flui-app`'s
`a_refused_window_costs_no_texture_frame` pin that texture frames fed to a refused window publish
nothing and leave no frame work owed, and `external_textures::a_shown_refused_version_takes_texture_frames_again`
that a window shown the version it refused takes them again.

## Pros and cons of the options

### 1. Retry after the dropped-frame backoff, refused windows wait for a change

- Good, because it reuses the wait, the press bypass and the cap that a dropped segment and a lost
  device already have.
- Good, because a window the host knows is refused spends no paint until it changes.
- Bad, because the streak's reset moves from the segment to the verdict, which ADR-0048 set.
- Bad, because a sink gains a verdict, and must choose between `Failed` and `Refused`.

### 2. Every failed frame retries on the backoff

- Good, because the verdict set stays as it is.
- Bad, because a window at a size the device refuses paints once per second until it is resized.

### 3. Retry at once

- Good, because it recovers a frame that failed once within one display period.
- Bad, because a sink that fails every frame makes the presentation render at the display rate
  forever.

### 4. Release held input on failure

- Good, because no frame is spent.
- Bad, because the input hit-tests a tree the screen does not show, which is the mismatch the hold
  exists to prevent, and the stale frame stays on screen.

### 5. Keep ADR-0048

- Good, because nothing changes.
- Bad, because the application keeps its last frame and holds its input until an unrelated frame
  runs.

## More information

`docs/gaps.md` (the ADR-0305 line) lists what a settlement still does not owe. The hold covers
pointer input and an OS drag's hover (ADR-0211 §16). A frame answered `NotShown` waits out the same
backoff once its withdrawal outlasts its budget (ADR-0335 §2).
