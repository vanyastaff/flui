# ADR-0353: Production is paced by the platform, and a frame that showed nothing waits for the fallback

- **Status:** Accepted (2026-11-17)
- **Date:** 2026-11-17
- **Supersedes:** [ADR-0058](ADR-0058-the-platform-paces-production-not-a-sleep.md) whole: this
  record states the whole frame-pacing rule, with decision 4 changed (leftover work waits behind a
  pending fallback). It restates, without superseding the other decisions of those records, the
  parts that bore on pacing: [ADR-0347](ADR-0347-a-frame-request-carries-the-hosts-frame-times-and-reports-that-name-their-frame.md)
  decision 4 (the swapchain's frame latency is a setting whose default is 2),
  [ADR-0249](ADR-0249-the-android-loop-blocks-until-woken.md) §4 (Android's backgrounded pump is
  paced by a deadline), [ADR-0268](ADR-0268-the-ios-display-link-paces-the-backgrounded-pump.md)
  (the iOS display link paces its backgrounded pump),
  [ADR-0305](ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md) §5
  (the slot holds the window's `SurfaceTarget` and calls its `pre_present_notify`) and
  [ADR-0236](ADR-0236-display-snapshot-and-refresh-period.md) decision 2 (where the fallback
  period comes from)
- **Superseded-by:** [ADR-0356](ADR-0356-a-surface-its-compositor-paces-presents-without-waiting-on-the-compositor.md)
  decision 0 only, its first sentence: a surface its compositor paces (Wayland) presents in
  `Mailbox`, else `Immediate`; the rest of the decision stands.
- **Related:** [ADR-0044](ADR-0044-driver-loop-hybrid.md) §7 (the wake-deadline hook), [ADR-0045](ADR-0045-raster-lane.md)
  decision 3 (what paces production once the produce loop does not block),
  [ADR-0254](ADR-0254-window-contract.md) §5 (`SurfaceTarget`),
  [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (frame pacing is a sanctioned leapfrog zone:
  Flutter's engine-driven vsync callback is a different runtime model, not a contract FLUI
  transcribes), [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (a wake becomes a
  frame at the runner's per-backend wake gate)
- **Sources:** the original frame-pacing context of retired
  [ADR-0029](https://github.com/vanyastaff/flui/blob/aaecd27847a6338538f70e230a1aa01a018b0fd6/docs/adr/ADR-0029-frame-pacing-swapchain-block-with-fallback-throttle.md),
  and the [live-smoke harness](https://github.com/vanyastaff/flui/tree/e8a42673ad23a2e88614940a266cd01224dcb25f/tools/live-smoke)

## Context and problem statement

Pacing used to be a fixed `thread::sleep` computed from `AppConfig::target_fps`, a polling
interval uncorrelated with the display that existed because `Mailbox` never blocks. Replacing it
with the blocking `Fifo` present as the pacer left a ~16 ms sleep for frames that ran the pipeline
but never presented, and that sleep became the pacer: a 165 Hz panel ran at ~60 fps with input
stalled most of every cycle. The windowing library FLUI then used showed that a Wayland frame callback is armed only in
`Window::pre_present_notify()` (which FLUI never called), and that the `Fifo` block engages
behind the swapchain, not per call. ADR-0058 replaced the sleep with the platform's signal and a
non-blocking deadline; five later records changed one clause each of it.

A gap remained in the deadline's reach. While a fallback deferral is pending, the last frame
presented nothing: no present blocked at vsync and no compositor paces the next one. The dirty
predicate ignored only the runtime's own redraw echo in that state, and counted pending
build, layout and gesture work as always dirty. A tree that dirties itself each frame (an
animation driven outside a ticker) therefore ran frames at CPU speed into a sink that shows
nothing: a dead raster executor answering every frame `NoPresent` (ADR-0305 §9), or an occluded
window. What paces a frame that follows one that showed nothing, and what may still run at once?

## Decision drivers

- The frame path is synchronous and the event-loop thread never sleeps: input dispatches while a
  frame waits.
- A frame's pacer is the platform's (the blocking `Fifo` acquire, a compositor frame callback,
  AppKit's display pass, a display link); the framework's deadline is only the floor under the
  frames those do not pace.
- A deadline source must be in the dirty predicate and self-clearing, or its wake is skipped.
- Pacing is wrong in both directions: a spin at CPU speed wastes the machine, a missed wake freezes
  the window.
- One record states one rule (AGENTS.md, "A document states the current rule").

## Considered options

1. Keep leftover build, layout and gesture work immediate while a fallback is pending (ADR-0058
   decision 4 as written).
2. Pace the work a frame that showed nothing left behind its pending fallback, with its redraw
   echo; new work, inbox redraws, a due raster attempt and the due fallback run at once (chosen).
3. Pace only behind a dead raster executor: a sink that reports `NoPresent` repeatedly stops
   asking for frames, and the predicate stays as it was.

## Decision

Chosen option: 2, because the cause is not the executor but a frame that showed nothing, and every
such sink (a dead executor, an occluded window, a hidden Wayland surface) is the same condition at
the runner. Option 1 spins; option 3 fixes one sink and leaves the occluded window spinning.

**0. `Fifo` is the default present mode, and the event loop waits.** `select_present_mode` picks
`Fifo`; `Mailbox` is not reachable from `flui-app`. Every native owner loop blocks until it is
woken or its wake deadline falls due (ADR-0044 §7), so no loop polls.
`AppConfig::target_fps` is advisory; nothing paces from it. The
swapchain's `desired_maximum_frame_latency` is the typed `FrameLatency` setting, `Balanced` (2,
wgpu's default and the measured default) unless the application or a window sets `Low` (ADR-0347
decision 4; the window's slot supplies it at every `Surface::configure`, ADR-0305 §5). At 1, a
frame whose own work exceeds what is left of the period after the previous drawable leaves scanout
waits a whole extra period in the acquire, which halved the frame rate of a realistic workload on
AppKit/Metal.

**1. The platform's own frame-pacing signal is armed before every present.** The window's slot on
the raster executor holds its `SurfaceTarget` and calls `SurfaceTarget::pre_present_notify`
directly, immediately before `queue.present`, only for a frame that will present and never on a
skip path: a Wayland frame callback requested with no commit behind it withholds every later
redraw. A host without a frame callback returns at once. `flui-engine` names only
`flui-platform`'s `SurfaceTarget`, never a native window.

**2. No sleep. What replaces it is a deadline.** `FallbackWake` defers the next ticker-only wake to
`0.95 ×` one display period after the last present and reaches the loop through the wake-deadline
hook (the loop's wake deadline, ADR-0044 §7). The event-loop thread never sleeps. The period is the
current mode of the window's own display, from the display snapshot (ADR-0236 decision 2), re-read
when the window's display or its period changes; a display that reports none, Wayland included,
uses `DEFAULT_REFRESH_PERIOD` (16.667 ms, `flui-runtime`). The factor is 0.95 rather than a full
period so that, where the present blocks, the pump stays phase-locked to the display instead of
sliding later every frame and beating against vsync. The deadline applies only when the slot has no
frame in flight and its last settlement did not present (ADR-0305).

**3. Only the frame callback may consume the deadline; the wake query must report it.**
`FallbackWake::gate` (frame callback) consumes; `next_wake` (wake-deadline hook) reports and never
clears an armed deadline still within one period of `now`. A loop asks for its deadline on the
iteration the deadline expires, before it dispatches the wake that deadline brings; clearing a
just-passed deadline there left the loop blocked with nothing to wake
it. The first report of a due deadline is answered however late the loop asks; a deadline reported
due and left unanswered for a full period is abandoned instead of re-reported (the hidden Wayland
surface), bounded at about one wasted wake, and clearing it lifts the echo suppression.

**4. While a fallback is pending, the work its frame left waits for it like the redraw echo.**
`frame_is_dirty` (`crates/flui-app/src/app/runner/frame_pacing.rs`) takes a `FallbackGate`:
`pending` (armed and not yet due), `due` (this wake is the deferred one) and `holds_work` (the
frame that armed it left tree work: `FallbackWake::arm_after_no_present` records
`UiRuntime::has_pending_tree_work` right after that pump). It also takes the runtime's
`PendingWork`, split in three; their union is `has_pending_work`. Tree work
(`has_pending_tree_work`) is what the next frame does to the tree: a pending build, a dirty render
node, a completion the next frame takes. Input work (`has_pending_input_work`) is what a frame does
for input still in progress: pointer motion to resample, a gesture deadline still armed. Due work
(`has_pending_due_work`) has an instant of its own: a due task deadline, a dropped frame whose
backoff is over, a gesture deadline that came due, a haptic delivery, a frame-rate hold that ran
out. With `pending`, the UI runtime's own redraw echo (`needs_redraw`, which every pump with a
running ticker sets again) is not dirty, and neither is input work: a held press re-arms its
deadline every frame, so it would spin as surely as the echo, and the frame that serves it can wait
one fallback. Tree work is not dirty only with `holds_work` too, so tree work that arrives after a
frame that left none (a link opened, a tap's state change) runs at once. Due work is never masked:
it runs on the wake its own deadline brings, so a timer or a long press is not served late and its
wake is not skipped and re-armed. The runtime reads input work before due work, so a gesture
deadline that comes due between the two reads is counted at least once. An inbox redraw, an armed
raster attempt (`next_attempt_at`, a device rebuild or a surface restore) and the due fallback
count at once. A frame's immediate retry (a panicking resize asking again) is tree work that frame
left, so it waits for the fallback and cannot spin; a dropped frame's paced retry is due work at
its backoff deadline. A frame the loop thread ran that presents nothing and leaves nothing the
runtime counts as pending (the gate closes, so it arms no deferral) releases a pending fallback's
`holds_work` (`FallbackWake::release_held_work`); the deadline stays, and tree work after that
frame runs at once. Work a backoff or an abandoned device masks is not counted, and it returns as
due work or through an explicit retry, bounded by those. A frame a raster thread held does not
release the hold; it lasts until the deadline, a later loop-thread frame that closes the gate, or
a present. With no fallback armed, every pending work is dirty at once. A press, a release or a
key is dispatched when it arrives (the platform input callback calls `handle_input_addressed`
directly). The frame that shows its effect waits for a pending fallback when that effect is a
redraw request, a scheduled frame or input work, and when it is tree work only while the fallback
holds tree work.
Pointer motion is coalesced or resampled and delivered by the frame,
so on a window whose last frame showed nothing, its handlers run with that frame, at most one
fallback (0.95 display period) late. A deadline source must be in the dirty predicate and
self-clearing, or its wake arrives and is skipped: each
desktop loop turns the fallback's deadline into the frame it was asked for (`WakeDeadline::take_due`
on Win32, X11 and Wayland; the wake pump's look, `WakeDeadline::turn`, on AppKit). The fallback is desktop's:
Android paces a rendering wake by its choreographer and iOS by its display link, and both pass an
empty `FallbackGate`.

**5. A backgrounded pump is paced by a deadline, or by the display link, never by a sleep.**
Android: a backgrounded (`PumpBackground`) pump that leaves work asking for another wake arms
`BackgroundPace` one `BACKGROUNDED_PUMP_PACE` (16 ms) ahead; the wake hook reports it, a wake
before it does not pump, the wake it brings consumes it, and a wake with frames enabled drops it
(ADR-0249 §4). iOS: the `PumpBackground` arm runs `UiRuntime::pump_background` and returns; the
display link delivers at most one tick per refresh and none while held, so it is the pace and
`BackgroundPace` is compiled for Android and tests only (ADR-0268).
`DeviceRecoveryBackoff::BASE` does not alias a pacing constant. No runner sleeps on its loop
thread. [ADR-0199](ADR-0199-presentation-sampling-recovery.md) adds due input-deadline service to
the background operation; it does not change this pacing or enable frame production while frames
are disabled.

## Per-backend facts

- **Wayland / Vulkan:** compositor-paced once decision 1 arms frame callbacks; a hidden surface
  gets no callbacks, so its tickers freeze (battery-correct).
- **X11 / Windows:** redraws still arrive while occluded; decision 2's deadline bounds them. The
  `Fifo` acquire does the pacing (measured acquire p90 just under the panel period).
- **macOS (AppKit / Metal):** `queue.present()` does not block; the period comes from AppKit's
  vsync-aligned display pass. The acquire blocks only when a frame arrives with no drawable free,
  which the frame-latency setting of decision 0 addresses.
- **Unmeasured:** Windows' native backend; a window moving between monitors with different refresh
  rates. VRR panels report their ceiling; the deadline is a floor on produce rate, so a narrower
  range costs at most one deferred wake per frame.

Time-based UX (an auto-dismiss timeout driven by a ticker) does not advance while a surface is
frozen. That belongs to a wall-clock timer service, not to pacing.

## Pros and cons of the options

### 1. Leftover work stays immediate

- Good, because a frame that finds dirty work runs without waiting for the display.
- Bad, because a tree that dirties itself each frame spins at CPU speed into a sink that shows
  nothing.

### 2. Leftover work waits behind a pending fallback (chosen)

- Good, because every sink that showed nothing is bounded to one frame per 0.95 period, with one
  predicate and no knowledge of the sink's kind.
- Good, because discrete input is unaffected: a press, a release or a key is dispatched on arrival,
  and tree work that arrives after a frame that left none runs at once. Pointer motion and the
  frames that show input wait at most one fallback, and only on a window that showed nothing.
- Bad, because the frame after a non-presenting one is up to 0.95 of a period late even when the
  work is real; that frame would have been shown at the next vsync at best.

### 3. Pace only a dead executor

- Good, because the predicate stays as it was.
- Bad, because it needs a sink-specific state and leaves an occluded window, or any other sink
  that shows nothing, spinning.

## Consequences

- A 165 Hz panel animates at 165 fps, not 60, and the event-loop thread never blocks on a sleep.
- Wayland gets compositor pacing and hidden-surface silence from the protocol rather than a timer.
- A self-dirtying tree over a sink that shows nothing costs one frame per fallback period.
- The pacing path is traced: `flui.gpu`'s `surface_acquired` (with the acquire duration),
  `pre_present_notified`, and `flui.pace`'s `fallback_armed` / `fallback_abandoned` /
  `segment_poll`. The live-smoke harness asserts one `pre_present_notified` per `present_submitted`.
- The records ADR-0058 gathered keep their other decisions; each still names its pacing clause but
  this record is where the rule is read.

## Confirmation

Tests in `crates/flui-app/src/app/runner/frame_pacing.rs`, rows of `desktop_pacing_matrix` and
`background_pace_tests`:

- `leftover_work_after_a_frame_that_presented_nothing_waits_for_the_fallback` (decision 4: the arm
  records the tree work its frame left; the echo and that tree work wait, a held press waits even
  when the frame left no tree work, due work runs on its own wake, the due fallback runs the frame,
  tree work after a frame that left none runs at once, as do an inbox redraw and an armed raster
  attempt, and a release keeps the deadline but frees later tree work; red when tree work counts past a
  pending fallback that holds work, and red when input
  work counts past any pending fallback), in `flui-runtime`'s contracts,
  `a_dropped_frames_retry_is_due_work_and_a_repaint_is_tree_work` and
  `a_pointer_move_is_input_work_and_neither_tree_nor_due_work` (the split of
  `has_pending_work`), and in `flui-app`'s `app_it`,
  `a_resize_that_keeps_panicking_is_not_retried_forever` (a panicking resize is retried once, at
  the fallback's deadline, not on every period).
- `a_pending_fallback_defers_a_wake_with_no_dirty_work_but_never_dirty_work` (decision 4).
- `idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips`,
  `the_fallback_deadline_is_anchored_to_the_last_present` (decision 2).
- `an_unanswered_deadline_is_abandoned_by_the_period_in_force_when_asked`,
  `a_deadline_first_read_late_is_still_reported_due` (decision 3).
- `a_held_frame_arms_no_fallback` (decision 2).
- `a_background_pump_defers_the_next_by_one_pace` (decision 5, Android).
- `a_frame_latency_setting_reaches_the_configuration_and_survives_release` (`flui-engine`,
  decision 0).

The live-smoke harness's one `pre_present_notified` per `present_submitted` confirms decision 1.
