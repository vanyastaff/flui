# ADR-0257: A frame request carries the host's vsync and target present time, and frame latency is a typed setting

- **Status:** Accepted (2026-10-26). Not implemented: no backend reports a platform frame timestamp
  yet. Every backend fills the vsync with its own reading of the event clock when the request is
  admitted, and no backend fills `target_present`. A platform's row in `docs/platform-parity.md`
  moves only after a run on that platform shows the rebased stamp is right (Confirmation).
- **Date:** 2026-10-26
- **Supersedes:** [ADR-0236](ADR-0236-display-snapshot-and-refresh-period.md) decision 8 only: the
  frame tick may be the frame's vsync instant, a sampled time. It is still not shifted to the
  presentation time, and the rest of the decision stands.
- **Supersedes:** [ADR-0058](ADR-0058-the-platform-paces-production-not-a-sleep.md) decision 0 only,
  its sentence fixing `desired_maximum_frame_latency` at 2: the number of frames in flight is a
  setting whose default is 2. `Fifo` stays the only present mode, and the rest stands.
- **Related:** [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md) §2 (the event timeline a
  vsync is stamped on), [ADR-0249](ADR-0249-the-android-loop-blocks-until-woken.md) (a loop that
  blocks until woken is why `Mailbox` is not offered)

## Context and problem statement

A host wake reaches the frame pump as a bare `on_request_frame()`. The pump reads the clock when the
owner runs, so event-loop latency becomes animation and scroll jitter at 120 and 144 Hz. The
platforms report better: the browser's `requestAnimationFrame` timestamp, `CADisplayLink`'s
`timestamp` and `targetTimestamp`, Choreographer's `frameTimeNanos`, DXGI's last vblank. The iOS
backend reads them to measure the period and discards them. `HostInput::Frame { vsync }` already
carries a vsync to pointer resampling, but every host fills it with the admission time, and no host
supplies an idle deadline, so the idle-priority deferral the scheduler implements never defers.

Latency has the same shape on the other side of the frame. The engine configures the surface with
`Fifo` and `desired_maximum_frame_latency: 2` in one literal. An application where input latency
matters more than throughput (drawing, a game) cannot ask for one frame in flight, and the number
changed once by editing the literal, after a measurement: a 2,000-row list at 100 Hz presented at
exactly half the display rate with 1.

How does a host tell the frame pump what it knows about a frame, and how does an application choose
how far the swapchain runs ahead of the display?

## Decision drivers

- the frame path is synchronous and the pump reads its clock once per frame (ADR-0083);
- a platform that reports nothing must behave exactly as it does today;
- the tick must never run backwards, and a wrong platform timestamp must not freeze or skip it;
- the loop blocks until woken (ADR-0249), so a present mode that never blocks spins it;
- the request grows again (a refresh period, a frame id) without another signature change.

## Considered options

1. A `FrameRequest` value on `on_request_frame`, and a `#[non_exhaustive]` `FrameLatency` on
   `AppConfig`.
2. Keep the bare callback and add one setter per datum (`set_frame_vsync`, `set_target_present`).
3. As option 1, and add `Mailbox` as a `FrameLatency` variant.

## Decision

Chosen option: 1, because one value is the only shape that grows without breaking every callback,
and `Mailbox` needs a pacer FLUI does not have.

1. **A frame request is a value.** `on_request_frame` receives
   `FrameRequest { vsync: EventTime, target_present: Option<EventTime> }`, `#[non_exhaustive]`, on
   the event timeline of ADR-0207 §2. A backend fills what its platform reports and nothing it would
   have to invent: `vsync` is the platform's frame timestamp rebased onto the event epoch, or the
   backend's own reading of that clock at admission where the platform reports none;
   `target_present` is `None` unless the platform names when this frame will be shown. Neither is a
   prediction. A bare vsync converts into a request that knows nothing else.
2. **The pump takes the frame's time from the request.** The tick is the vsync instant, held between
   the previous frame's time and the host's own reading when it pumped: never earlier than the last
   frame (the clock is monotone), never later than the wake (a platform timestamp ahead of that is a
   defect, not a time). A pump with no delivered request, or one whose vsync lies off the clock's
   range, uses the host's reading as before. A request a pump did not render is forgotten when the
   pump ends, so a skipped frame's vsync never dates a later wake. This is a sampled instant, not a
   shifted one: ADR-0236 decision 8's argument against an offset stands, and the tick is never the
   presentation time.
3. **`target_present` bounds idle work.** When a request names it, idle-priority tasks run only
   until that instant and the host's own deadline is not asked; otherwise the deadline is the
   host's, as before. It travels with the vsync in `HostInput::Frame`, so a recorded session
   replays the same bound.
4. **Frame latency is a typed setting.** `FrameLatency` is `#[non_exhaustive]`: `Balanced` is two
   frames in flight, the measured default, and `Low` is one. `AppConfig::frame_latency` carries it
   (`with_frame_latency`), each runner applies it to its renderer after creation, and the renderer
   stores it and supplies it at every `Surface::configure`, including the recovery and recreation
   paths. A per-window override is a call on that window's renderer. A backend that ignores the
   bound (wgpu on GL) keeps ignoring it.
5. **`Fifo` stays the only present mode.** `Mailbox` never blocks `present()`, so the wake-driven
   loop would spin at tens of thousands of frames a second, the defect ADR-0058 removed. It enters
   with a pacer that does not rely on the present call blocking, under its own ADR.

### Consequences

- Good, because a backend that learns the platform's vsync changes one call, and the pump, the
  recording and the idle bound follow with no further plumbing.
- Good, because a host with no timestamp is unchanged: its vsync is its admission time, which is
  what the pump read before, less the queueing between admission and the owner's turn.
- Good, because an application can choose latency without editing the engine.
- Bad, because `Low` can halve the frame rate of a frame whose work exceeds the display period; it
  is opt-in for that reason.
- Bad, because the vsync clamp hides a backend whose rebase is wrong instead of failing: the frame
  runs at the wake's time, which is today's behaviour.
- Neutral, because the tick is a sampled instant: controllers anchored at their first tick do not
  see the change.

### Confirmation

- `a_delivered_vsync_is_the_frames_time_held_between_the_last_frame_and_the_wake` (flui-runtime
  `tests/contracts/frame_clock.rs`) pins the sampling, both clamps and the fallback.
- `a_target_present_time_bounds_the_idle_work_of_its_frame` pins the idle bound and that the
  host's deadline is not asked when the request names one.
- `a_frame_latency_setting_reaches_the_configuration_and_survives_release` (flui-engine) pins the
  stored setting, and the `recording_vocabulary` row for `Frame` pins `target_present` through a
  recording's round trip.
- A backend's timestamp is confirmed by an executed run on that platform; `docs/platform-parity.md`
  names it when it exists.

## Pros and cons of the options

### 1. A `FrameRequest` value and a `FrameLatency` setting

- Good, because both grow by a field or a variant without a breaking signature.
- Good, because the pump and the recording read one value.
- Bad, because every `on_request_frame` caller changes once.

### 2. A bare callback and a setter per datum

- Good, because no caller changes.
- Bad, because the data would travel beside the callback, with an ordering a backend must keep
  right, and the recording would need the same ordering.

### 3. As option 1, with `Mailbox` as a variant

- Good, because it is the lowest-latency mode wgpu offers.
- Bad, because `present()` never blocks under it: the loop spins until a separate pacer exists, and
  a setting that silently burns a core is worse than no setting.

## More information

Revisit when a backend reports a platform timestamp: the rebase it uses (the browser's time origin,
`CACurrentMediaTime`, `CLOCK_MONOTONIC` for Choreographer) is its own decision, run on that
platform, and `target_present` is then filled where the platform names it (`targetTimestamp`).
