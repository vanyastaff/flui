# ADR-0267: A frame dropped on a lost GPU device waits out the dropped-frame backoff, and a rebuild that keeps failing is given up until the application asks again

- **Status:** Accepted (2026-10-30)
- **Date:** 2026-10-30
- **Supersedes:** [ADR-0101](ADR-0101-transient-render-failure-retains-frame-demand.md), its
  sentence "this decision does not introduce a timeout that silently drops owed content", for a
  lost device only: the owed frame of a device whose rebuild was given up is held, reported, and
  drawn again only after the application asks for a rebuild.
- **Related:** [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) §6 (the
  dropped-frame backoff a device-lost frame now takes), [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md)
  (no GPU edge in `flui-runtime`, which is why the rebuild is reported in the runtime's own terms),
  [ADR-0045](ADR-0045-raster-lane.md) (the raster lane the rebuild runs on)

## Context and problem statement

When the GPU device is lost (a driver reset, a GPU switch, an unplugged eGPU), `flui-app`'s frame
driver rebuilds it around each frame (`pump_with_device_recovery`) on its own backoff
(`RetryBackoff`: 16 ms doubling to one second). The UI runtime saw the frame's
`SubmitVerdict::DeviceLost` and asked for the next frame at once. The rebuild was paced; the frames
were not. On a desktop a device that never came back cost one frame per vsync for the life of the
process, and on the headless host, which delivers a frame request in the same drain, the loop never
returned. The rebuild itself was never given up either, so a machine without a working GPU ran a
full device rebuild every second, forever, with nothing telling the application.

How does a lost device stop costing frames, and how does an application learn that the window will
not render again by itself and get it back?

## Decision drivers

- No hot loop: a lost device costs no more frames than any other dropped frame (ADR-0258 §6).
- No silent stall: the application is told when the window stops rendering, and can get it back
  without a restart.
- A driver reset or a GPU switch, which takes seconds, recovers by itself.
- `flui-runtime` stays free of GPU types (ADR-0083).
- Input, timers and the application keep running while the device is gone; `run` does not return.

## Considered options

1. A device-lost frame takes the dropped-frame backoff; the rebuild is given up after a fixed count
   of failures at the backoff's ceiling, reported, and restarted by an application call.
2. A device-lost frame takes the dropped-frame backoff; the rebuild is never given up.
3. Treat a lost device that does not come back as terminal: report it and end `run` with an error.
4. Stop frames while the device is lost, without a report; the rebuild alone wakes them.

## Decision

Chosen option: 1, because it is the only option that bounds both the frames and the rebuilds,
recovers a driver reset by itself, and leaves the decision about a GPU that is gone to the
application.

### §1 A frame dropped on a lost device waits out the dropped-frame backoff

`render_frame`'s `SubmitVerdict::DeviceLost` arm records the dropped frame, marks the presentation
for a full repaint, reports it, and arms the presentation's failure retry exactly as any
`FrameDropped` failure does (ADR-0258 §6). It no longer asks for a frame at once. The host's wake
deadline carries the retry through `UiRuntime::next_wake`, as before.

### §2 The report says where the rebuild stands

The report is a new kind, typed in the runtime's own terms:

```rust
// flui-runtime::frame_failure
#[non_exhaustive]
pub enum FrameFailureKind {
    // ...
    #[non_exhaustive]
    DeviceLost { recovery: DeviceRecovery },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum DeviceRecovery {
    #[default]
    Pending,                       // no rebuild has run yet
    Failed { attempts: u32 },      // the last `attempts` rebuilds failed; another is armed
    Abandoned { attempts: u32 },   // given up after `attempts` failed rebuilds
}
```

Its disposition is `FrameDropped`. The frame driver tells the runtime after each rebuild through
`UiRuntime::note_primary_device_recovery(Option<DeviceRecovery>)` (`None` once the device is back),
and every dropped frame carries the last value.

### §3 A rebuild failing at the ceiling is given up

`flui-app`'s `DeviceRecoveryBackoff` wraps the shared `RetryBackoff` with a streak. A failed
rebuild whose next attempt is a full ceiling away counts against
`DEVICE_RECOVERY_ATTEMPTS_AT_CAP = 10`. The backoff reaches its ceiling after seven failures, about
two seconds after the loss, so the driver has about twelve seconds in all: longer than a Windows
driver reset (TDR, two seconds by default) or a GPU switch takes. A rebuild still failing then is
given up:

- the runtime reports one `DeviceLost { recovery: Abandoned { attempts } }`;
- the presentation's failure retry is disarmed, and while it is given up it asks for no frame and
  arms no retry; a frame that another source asks for (an animation, input) still runs its non-GPU
  half, and is neither reported nor retried;
- the frame driver runs no more rebuilds and reports no wake deadline for one.

A successful rebuild resets the streak.

### §4 The application asks again

```rust
// flui-app
impl AppHandle {
    pub fn retry_render_device(&self) -> Result<(), AppControlError>;
}
// flui-runtime
impl UiRuntime {
    pub fn retry_render_device(&self) -> bool;
}
```

`AppHandle::retry_render_device` counts a request in the application's `DeviceRetry` and wakes
every rendering window's frame. The window's next frame takes the presentation out of the given-up
state (`UiRuntime::retry_render_device`, which marks a full repaint and asks for a frame) before its
dirty gate, and the frame driver, seeing the runtime no longer given up, starts the streak over and
rebuilds at once. A window whose device is healthy or still being rebuilt ignores the request. The
wakes run without the request list's lock held.

### Consequences

- A lost device costs at most one frame per dropped-frame retry interval, and the headless host no
  longer spins on it.
- A machine whose GPU does not come back stops rebuilding after about twelve seconds; the window
  shows its last frame until the application asks again.
- An application that installs no `FrameFailureHandler` is not told; the runner logs the give-up at
  `error`.
- `AppHandle` exists on the desktop runner, so only desktop windows can be asked again. An Android
  or iOS window whose rebuild was given up stays given up until the process restarts; the gap is
  listed in `docs/gaps.md`.

### Confirmation

- §1, §2: `a_frame_dropped_on_a_lost_device_waits_out_the_backoff`,
  `a_failed_rebuild_is_reported_with_the_dropped_frame` (`flui-runtime`, `tests/contracts/device_loss.rs`).
- §3: `a_given_up_rebuild_holds_frames_until_the_application_asks_again` (`flui-runtime`), and the
  `device_recovery_matrix` row
  `a_rebuild_failing_at_the_ceiling_is_given_up_until_the_application_asks` (`flui-app`).
- §1, §3, §4 through the runner: the `runner_close` rows
  `a_lost_device_waits_out_its_rebuild_backoff` (one rebuild per turn and a wake deadline in the
  future; the old code never returned from the turn) and
  `a_given_up_device_renders_again_when_the_application_asks` (the headless run with a stand-in
  renderer whose rebuild always fails, then `AppHandle::retry_render_device`).

## Pros and cons of the options

### 1. Backoff, give up at a count, application retry (chosen)

- Good, because frames and rebuilds are both bounded.
- Good, because a driver reset recovers by itself, and a GPU that is gone is the application's call.
- Bad, because it adds a public kind, a public enum and a method on `AppHandle`.
- Bad, because the count is a policy number; a driver that takes longer than twelve seconds needs
  the application's retry.

### 2. Backoff, never give up

- Good, because nothing new is public, and a GPU that comes back after a minute recovers by itself.
- Bad, because a machine without a working GPU rebuilds the device every second forever, and the
  application is never told that the window stopped rendering.

### 3. Terminal after a count

- Good, because the failure is loud.
- Bad, because a lost GPU ends the application, where input, timers and saving still work; an
  application cannot recover or save first.

### 4. Hold frames without a report

- Good, because nothing new is public.
- Bad, because a stalled window is invisible to the application, and the rebuild still runs
  forever.

## More information

The give-up count lives in `crates/flui-app/src/app/runner/device_recovery.rs`
(`DEVICE_RECOVERY_ATTEMPTS_AT_CAP`). Web's recovery (`bootstrap_web`) is async and is not covered by
this decision.
