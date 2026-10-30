# ADR-0269: The choreographer paces Android frames, and a frame request carries its vsync

- **Status:** Accepted (2026-10-30)
- **Date:** 2026-10-30
- **Related:** [ADR-0257](ADR-0257-frame-timing-and-latency.md) decision 1 (a frame request carries
  the host's vsync and target present time), [ADR-0249](ADR-0249-the-android-loop-blocks-until-woken.md)
  §2 (the loop has no timer of its own; this ADR adds the turn that waits for the choreographer),
  [ADR-0244](ADR-0244-os-version-floors-and-gates.md) §4 (gates are types, and an API above the floor
  is found with `dlsym`), [ADR-0268](ADR-0268-the-ios-display-link-paces-the-backgrounded-pump.md)
  (the iOS display link)

## Context and problem statement

An Android frame runs on the first turn of the looper that finds a frame owed (`FrameDemand`) or a
wake deadline due, and its request is stamped `frame_request_now()`: the vsync is the loop's own
reading, and the target present time is unknown. iOS hands its request the display link's
`timestamp` and `targetTimestamp`, and the Web the `requestAnimationFrame` time. Android has the
same source in `AChoreographer`, which the loop thread already owns for its refresh-rate callback
(`display::android`).

The facts:

| Fact | Where |
|------|-------|
| `AChoreographer_getInstance` and `AChoreographer_postFrameCallback` exist from API 24; the callback's frame time is a `long`, 32 bits in a 32-bit process | `<android/choreographer.h>` |
| `AChoreographer_postFrameCallback64` (API 29) takes an `int64_t` frame time | same |
| `AChoreographer_postVsyncCallback` (API 33) hands `AChoreographerFrameCallbackData`, whose preferred frame timeline names `expectedPresentationTimeNanos` | same |
| A posted frame callback can't be taken back | same |
| The callback runs inside `ALooper_pollOnce` on the thread that posted it, which then goes on waiting | `android-activity` `poll_events` |
| The frame time is on `CLOCK_MONOTONIC` | `System.nanoTime`, `Choreographer.FrameCallback` |
| The backend's floor is API 23 | `platform-support.toml` |

Should Android frames keep running on the loop's turn and only be stamped better, or run on the
choreographer's frame?

## Decision drivers

- A frame request carries what the platform reports, never an invention (ADR-0257 decision 1).
- The loop blocks until woken and never busy-waits (ADR-0249).
- A function above the floor is never linked and is reached only through its gate (ADR-0244 §4).
- No accepted work is dropped: a frame the loop is waiting on still runs.

## Considered options

1. **Keep the turn-driven frames and stamp them from the last refresh period.**
   - Good: no change to when frames run.
   - Bad: the period says how long a frame lasts, not when it started. The stamp would be an
     estimate, which ADR-0257 rules out.
2. **The choreographer's next frame paces the frames: post a callback, deliver on its tick.**
   - Good: the request carries the platform's vsync, and on API 33 its expected presentation time.
     Frames line up with the display as on iOS and the Web.
   - Bad: the first frame after idle waits up to one refresh for the callback.
3. **As option 2, from API 29 only, skipping the API 24 post.**
   - Good: one callback signature everywhere.
   - Bad: a 64-bit device on API 24 to 28 has the same frame time through the older post, and
     would lose it for no reason.

## Decision

Chosen option: 2, because it is the only one that reports the platform's own frame time. The
API 24 post is kept for 64-bit processes, where its `long` is the same 64 bits as the API 29 one.

1. **The posts are gated and found by name.** `owner::android::ndk` declares a marker per function,
   each a `Gate` with its level and resolved through `NdkFn` (the level, then `dlsym`):
   `ChoreographerInstance` (24), `PostFrameCallback` (24, only on `target_pointer_width = "64"`),
   `PostFrameCallback64` (29), and `PostVsyncCallback`, `VsyncFrameTime`, `VsyncPreferredTimeline`
   and `VsyncExpectedPresent` (33). `FrameCallbacks::resolve` takes the newest post the system has:
   the vsync callback only with all three readings of its data, then `PostFrameCallback64`, then
   `PostFrameCallback`. It finds none below API 24, in a 32-bit process below API 29, or when the
   instance is missing.
2. **A frame waits for the choreographer's next one.** `owner::android::frame_clock::FrameClock`
   holds the loop thread's choreographer. A turn that would deliver a frame posts one callback,
   unless one is still posted, and blocks: `LoopTurn::awaiting_vsync` makes `poll_timeout` wait
   until woken, or until a deadline or preference sample still ahead. A deadline already behind is
   the frame being waited for.
3. **The callback records and wakes.** It runs as one `os_entry`, records a `VsyncTick` (the frame
   time, and on API 33 the preferred timeline's expected presentation) and wakes the looper. It
   calls no user code. Each post hands the callback a counted reference to the state it records
   into, which the callback releases. One still posted at the loop's exit is leaked with that
   state, because a frame callback cannot be removed.
4. **A tick serves the turn whose poll ran it.** After the poll, the loop takes the tick. The frame
   runs only when there is one and the same turn can deliver (`PostPollRedraw::take_if_deliverable`:
   resumed, and a request or a due deadline). A tick that turn can't deliver is dropped and the next
   frame posts again, so no frame runs on a stale vsync. A Pause during the poll keeps the request
   for the Resume, as before.
5. **The request carries the tick.** `VsyncTick::timing` places the vsync `age` before a
   `CLOCK_MONOTONIC` reading and the expected presentation `until_present` after it.
   `frame_request_timed` turns both into event-clock times, so no epoch is aligned. A vsync ahead of
   the reading is the reading; a presentation not after both the vsync and the reading is unknown.
6. **Without a post, frames run as before:** on the turn that has one, stamped
   `frame_request_now()`.

### Consequences

- Good, because Android frame requests carry the display's vsync, and from API 33 a target present
  time that bounds idle work (ADR-0257 decision 3).
- Good, because frames start on the display's frame and not at whatever point the loop turn reached.
- Bad, because a frame owed after idle waits up to one refresh.
- Bad, because with the screen off the choreographer sends no callback, so an owed frame waits for
  the display. That is the iOS and Web behaviour; owner services and timers still run.
- Neutral, because presentation feedback stays `NotYetOnThisHost`: an expected presentation time is
  not a report that the frame was shown.

### Confirmation

- `ndk_gate_table` rows for each marker, and `the_frame_callback_is_the_newest_the_system_has` in
  `os_capability_table` (`flui-native`, run on the Linux CI host).
- `a_tick_places_its_vsync_and_presentation_around_the_reading` (`owner::android::frame_clock`).
- `a_paused_loop_blocks_until_it_is_woken` (`owner::android::poll`) rows for a turn waiting for the
  next frame.
- `os_entry` around both callbacks: `xtask panic-boundary`.
- CI's `cross-typecheck (aarch64-linux-android)` builds the loop. A run on a device: frames follow
  the display's rate, and `FrameRequest::target_present` is set on API 33.
