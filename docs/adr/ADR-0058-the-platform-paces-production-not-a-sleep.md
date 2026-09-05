# ADR-0058: The platform paces production; a sleep never does

- **Status:** Accepted
- **Date:** 2026-09-05
- **Amends:** [ADR-0029](ADR-0029-frame-pacing-swapchain-block-with-fallback-throttle.md)
  decision 3 (the fallback throttle), [ADR-0044](ADR-0044-driver-loop-hybrid.md)
  §4 (the Wayland row), [ADR-0045](ADR-0045-raster-lane.md) decision 3 (what
  paces production once the produce loop stops blocking)
- **Supersedes nothing.** ADR-0029's central decision — the blocking `Fifo`
  present is the steady-state pacer where it blocks — stands.

## Context

ADR-0029 established, with a histogram, that the blocking `Fifo` present paces
production: 6.058 ms median against a 164.89 Hz panel, p90 within 0.1 ms.
It deleted the fixed frame-budget sleep on that basis and kept one coarse
fallback — `no_present_fallback_pace`, a fixed 16 ms `thread::sleep` taken on
the event-loop thread when a frame ran the pipeline but never reached
`present()` while a ticker kept asking for more.

Re-measured on 2026-09-05 with the same example (`examples/animated_box_app`,
dev profile) on the same hardware and panel, the picture had changed:

| | ADR-0029 (2026-07-17) | 2026-09-05, before this record |
|---|---|---|
| inter-present median | 6.058 ms | 16.5 ms |
| p90 | ~6.1 ms | 17.4 ms |
| presents in 20 s | — | stopped at ~+11 s |
| fallback sleeps | not counted | 382 per 684 presents |

The sleep had become the pacer. Every cycle looked like this: a frame
presents at T; the running ticker immediately asks for the next frame
(`FrameClock::try_arm_redraw_request`); the redraw arrives ~0.1 ms later;
nothing has changed in 0.1 ms, so the pipeline runs and presents nothing;
`no_present_fallback_pace` sleeps 16 ms **on the event-loop thread**. A
165 Hz panel therefore ran at ~60 frames per second, and input was blocked
for 16 ms out of every cycle.

Two further facts, both established by reading winit 0.30.13's source rather
than by inference:

1. **Wayland's compositor pacing was never switched on.** winit withholds
   `RedrawRequested` while a frame callback is outstanding, and arms that
   callback *only* in `Window::pre_present_notify()`
   (`platform_impl/linux/wayland/window/mod.rs:301`). FLUI never called it —
   zero matches in the whole tree — so on this workspace's own reference
   desktop every redraw request was delivered immediately, exactly as on X11.
   ADR-0044 §4's Wayland row ("Yes — per-surface frame callbacks…") described
   winit's *capability*, not this tree's behaviour, for the life of that
   record. Its own honesty note already said the row was unmeasured.
2. **The `Fifo` block is real, but it engages behind the swapchain, not per
   call.** wgpu-hal configures `min_image_count(maximum_frame_latency + 1)` —
   two images at our `desired_maximum_frame_latency: 1` — so an acquire
   cannot block until the queue is actually full. The 16 ms sleep drained
   that queue before the third acquire ever happened, which is why a probe
   taken *with the sleep in place* measured a 13 µs acquire and concluded the
   block was gone. With the sleep set to zero the same probe shows acquire
   blocking: p90 5.8 ms, p99 7.8 ms against a 6.06 ms panel period.

## Decision

**1. The platform's own frame-pacing signal is armed before every present.**
`PlatformWindow::pre_present_notify` (default no-op) is called by the raster
backend immediately before `queue.present`, through
`RasterBackend::set_pre_present_hook`. The engine owns the *when* (only for a
frame that will present, never on a skip path — a Wayland frame callback
requested with no commit behind it withholds every later redraw); the app
layer owns the *what*, wiring the window's notify into the backend at
bootstrap (`install_pre_present_hook`).

The hook is a backend capability rather than a direct engine→platform call
because `flui-engine` does not depend on `flui-platform` and must not: the
raster side takes an owned scene and a closure, never a window.

**2. The fixed sleep is deleted. What replaces it is a deadline, not a
shorter sleep.** `FallbackWake` defers the *next ticker-only wake* to one
display period after the *last present*, and reaches the loop through the
existing wake-deadline hook (`ControlFlow::WaitUntil`, ADR-0044 §7). The
event-loop thread never sleeps, so input dispatches during the interval
instead of behind it.

The period comes from the window's own display (`PlatformWindow::refresh_period`,
winit: `current_monitor().refresh_rate_millihertz()`), re-read on resize
because a window can change monitors. `DEFAULT_DISPLAY_PERIOD` (16.667 ms)
covers a backend that cannot report one — including Wayland, where winit
returns no current monitor; that path is compositor-paced by decision 1 and
does not depend on the number.

The deferral is `0.95 × period`, not a full period. On a stack whose present
blocks, the block absorbs the difference and the pump stays phase-locked to
the display; a full period would slide later every frame and beat against
vsync into a periodic dropped frame. On a non-blocking stack the ~5 % surplus
is absorbed by the swapchain.

**3. Only the frame callback may consume the deadline; the wake query must
report it.** `FallbackWake::gate` (frame callback) consumes; `next_wake`
(wake-deadline hook) reports and never clears an armed deadline that is still
within one period of `now`.

This split is not stylistic, and getting it wrong is a production freeze this
record is written on the far side of. winit runs `about_to_wait` on the very
iteration a deadline expires, *before* the redraw its `ResumeTimeReached` poke
queues has been dispatched. A first implementation cleared a just-passed
deadline in `next_wake`: the hook then answered `None`, the loop parked in
`ControlFlow::Wait`, the poke never happened, and — because a pending
deferral suppresses the realm's own redraw echo — nothing woke the loop again.
Measured on a real 164.89 Hz X11 session: the animation froze at +11.03 s with
`next_wake` observing the deadline **5 µs** late, and stayed frozen for the
remaining 9 s of the run.

A deadline more than one full period late is abandoned instead of re-reported.
That is the case where the wake can never be delivered — a hidden Wayland
surface withholds redraws — and bounds it at roughly one wasted wake, leaving
the presentation idle, which is correct for a hidden presentation. Clearing it
also lifts the echo suppression, so any ordinary wake produces again.

**4. The dirty predicate ignores the realm's own redraw echo while a
deferral is pending, and admits the due deadline itself.** Every pump with a
running ticker ends by re-requesting a frame (`wake_frame`), which sets
`needs_redraw`. While deferring, that echo *is* the wake being deferred, so it
does not count as dirty; inbox redraws, pending build/gesture work, and an
armed device-recovery attempt always do. The due deadline is the fifth term:
a deadline source must be in the dirty predicate **and** be self-clearing, or
its wake arrives and is skipped.

**5. Android keeps its sleep, renamed.** `BACKGROUNDED_PUMP_PACE` bounds the
backgrounded (`PumpAsync`) arm there because that frame source has no
wake-deadline hook to arm instead. Naming it separately stops a
display-derived pacing constant from being read as a backgrounded-pump
throttle. `DeviceRecoveryBackoff::BASE` likewise stopped aliasing the pacing
constant: a device that just died is not presenting anything to pace.

## Evidence

All captures: `examples/animated_box_app`, dev profile, NVIDIA RTX 3070 Ti
(driver 595.84), 3440x1440 @ 164.89 Hz (6.065 ms period), 20 s runs, X11 runs
with the window raised each second. Logs are kept beside the runtime-program
checkpoint under `.rust-studio/specs/2026-09-05-runtime-program/`.

| Session | | before | after |
|---|---|---|---|
| **X11** | presents in 20 s | 1555 | 3228 |
| | inter-present median | 16.49 ms | 6.83 ms |
| | p90 | 19.17 ms | 7.14 ms |
| | gaps > 1.6 periods | 1003 / 1428 | 46 / 2976 |
| **Wayland** | presents in 20 s | 1298, stopping at +11.1 s | 2852, full 20 s |
| | inter-present median | 1.69 ms bursts / 17.5 ms p90 (bimodal) | 6.79 ms |
| | gaps > 1.6 periods | 385 / 1155 | 169 / 2639 |

The full table, including the per-second present counts a median cannot show
and the two probe runs, is in
`.rust-studio/specs/2026-09-05-runtime-program/adr-0058-measurements.md`.
Fields that had no probe in the "before" build are marked "not traced" there
rather than reported as zero.

The X11 acquire histogram after the change shows the `Fifo` block doing the
pacing it is supposed to do (p90 5.78 ms against a 6.065 ms period) rather
than a sleep doing it.

**The freeze is attributed to the build it happened in.** The Wayland "before"
run stops presenting at +11.1 s while the animation keeps rebuilding — the
original defect. A *second*, different freeze appeared on X11 in the
intermediate build where decision 1 had landed but `next_wake` still consumed
its own deadline; that one is decision 3's evidence and is quoted in the
measurements file. Neither is the X11 "before" run, which does not freeze —
it merely runs at 60 fps on a 165 Hz panel.

**Not measured, stated rather than assumed:** Windows and macOS take the same
code path but are compile-only in CI; `pre_present_notify` is a no-op on both
today (winit forwards it; the native backends do not implement it).
Multi-monitor migration re-reads the period on resize, which is the signal
winit delivers for a monitor change, but a move between monitors of different
refresh rates has not been driven end to end here. VRR panels report their
ceiling through `refresh_rate_millihertz`; the fallback is a floor on produce
rate, not a target, so a VRR range narrower than the reported ceiling costs at
most one deferred wake per frame.

## Consequences

- A 165 Hz panel gets 165 frames per second of animation instead of 60.
- The event-loop thread no longer blocks on a sleep, so input is dispatched
  during the pacing interval rather than after it.
- Wayland gets compositor pacing and hidden-surface silence for free, from the
  protocol rather than from a timer.
- The pacing path now has three trace events for a future investigation to
  read: `flui.gpu`'s `surface_acquired` (with the acquire duration, the number
  that says whether the display is pacing this thread), `pre_present_notified`,
  and `flui.pace`'s `fallback_armed` / `fallback_abandoned` / `segment_poll`.
  The live-smoke harness asserts one `pre_present_notified` per
  `present_submitted` over a whole run — the ordering contract decision 1
  rests on, checked against the real wgpu renderer.
- ADR-0045 decision 3's re-measurement requirement is unaffected in intent but
  changed in comparand: the still-serial baseline it asks for is now this
  record's "after" column, not a 16 ms-quantized one.
