# ADR-0058: The platform paces production; a sleep never does

- **Status:** Accepted
- **Date:** 2026-09-05
- **Absorbs:** ADR-0058
- **Amends:** [ADR-0044](ADR-0044-driver-loop-hybrid.md) §4 (the Wayland row),
  [ADR-0045](ADR-0045-raster-lane.md) decision 3 (what paces production once the produce loop
  stops blocking)

Frame pacing is a sanctioned leapfrog zone (ADR-0027): Flutter's engine-driven vsync callback
is a different runtime model, not a contract FLUI transcribes.

## Context

Pacing used to be a fixed `thread::sleep` computed from `AppConfig::target_fps`, applied
whenever a ticker kept re-requesting frames. It was a polling interval uncorrelated with the
display, and it existed only because the present mode was `Mailbox`, whose `present()` never
blocks; without the sleep a running animation spun at tens of thousands of frames per second.

The first fix made the GPU's blocking `Fifo` present the pacer and kept a fixed ~16 ms sleep
only for frames that ran the pipeline but never presented. Measured later, that sleep had
become the pacer: a frame presents, the ticker immediately asks for another, the redraw
arrives ~0.1 ms later with nothing changed, the pipeline presents nothing, and the sleep blocks
the event-loop thread for 16 ms. A 165 Hz panel ran at ~60 fps with input stalled for most of
every cycle. Two further facts came from reading winit's source:

1. winit withholds `RedrawRequested` while a Wayland frame callback is outstanding, and arms
   that callback only in `Window::pre_present_notify()`, which FLUI never called. Wayland
   compositor pacing was never on.
2. The `Fifo` block engages behind the swapchain, not per call: an acquire cannot block until
   the image queue is full, and the sleep drained the queue before that could happen.

## Decision

**0. `Fifo` is the default present mode, and the event loop waits.** `select_present_mode`
picks `Fifo`; `Mailbox` stays a possible future opt-in for latency-sensitive apps and is not
reachable from `flui-app`. `WinitApp::about_to_wait` sets `ControlFlow::Wait` explicitly every
iteration even though it is winit's default, so a change of default upstream is a visible diff
rather than a silent busy poll. `AppConfig::target_fps` is advisory; nothing paces from it.
The swapchain's `desired_maximum_frame_latency` is 2 (wgpu's default): at 1, a frame whose own
work exceeds what is left of the period after the previous drawable leaves scanout waits a
whole extra period in the acquire, which halved the frame rate of a realistic workload on
AppKit/Metal.

**1. The platform's own frame-pacing signal is armed before every present.**
`PlatformWindow::pre_present_notify` (default no-op) is called by the raster backend
immediately before `queue.present`, through `RasterBackend::set_pre_present_hook`. The engine
owns the *when* (only for a frame that will present, never on a skip path — a Wayland frame
callback requested with no commit behind it withholds every later redraw); the app layer owns
the *what*, wiring the window's notify into the backend at bootstrap
(`install_pre_present_hook`). The hook is a backend capability rather than a direct call
because `flui-engine` does not depend on `flui-platform`: the raster side takes an owned scene
and a closure, never a window.

**2. No sleep. What replaces it is a deadline.** `FallbackWake` defers the next ticker-only
wake to `0.95 ×` one display period after the last present, and reaches the loop through the
wake-deadline hook (`ControlFlow::WaitUntil`, ADR-0044 §7). The event-loop thread never
sleeps, so input dispatches during the interval. The period comes from the window's own
display (`PlatformWindow::refresh_period`), re-read on resize because a window can change
monitors; `DEFAULT_DISPLAY_PERIOD` (16.667 ms) covers a backend that cannot report one,
including Wayland, which is compositor-paced by decision 1. The factor is 0.95 rather than a
full period so that, where the present blocks, the pump stays phase-locked to the display
instead of sliding later every frame and beating against vsync.

**3. Only the frame callback may consume the deadline; the wake query must report it.**
`FallbackWake::gate` (frame callback) consumes; `next_wake` (wake-deadline hook) reports and
never clears an armed deadline still within one period of `now`. winit runs `about_to_wait` on
the iteration a deadline expires, before the redraw its `ResumeTimeReached` poke queues has
been dispatched. Clearing a just-passed deadline there made the hook answer `None`, the loop
parked in `ControlFlow::Wait`, and — because a pending deferral suppresses the realm's own
redraw echo — nothing ever woke it: a real freeze. A deadline more than one full period late
is abandoned instead of re-reported; that is the hidden-Wayland-surface case, bounded at about
one wasted wake, and clearing it lifts the echo suppression.

**4. The dirty predicate ignores the realm's own redraw echo while a deferral is pending, and
admits the due deadline.** Every pump with a running ticker ends by re-requesting a frame,
which sets `needs_redraw`; while deferring, that echo *is* the deferred wake, so it is not
dirty. Inbox redraws, pending build/gesture work and an armed device-recovery attempt always
are. A deadline source must be in the dirty predicate **and** self-clearing, or its wake
arrives and is skipped.

**5. Android keeps its sleep, renamed.** `BACKGROUNDED_PUMP_PACE` bounds the backgrounded
(`PumpAsync`) arm, which has no wake-deadline hook to arm instead. `DeviceRecoveryBackoff::BASE`
no longer aliases a pacing constant.

## Per-backend facts

- **Wayland / Vulkan:** compositor-paced once decision 1 arms frame callbacks; a hidden
  surface gets no callbacks, so its tickers freeze (battery-correct).
- **X11 / Windows:** redraws still arrive while occluded; decision 2's deadline bounds them.
  The `Fifo` acquire does the pacing (measured acquire p90 just under the panel period).
- **macOS (AppKit / Metal):** `queue.present()` does not block; the period comes from AppKit's
  vsync-aligned display pass. The acquire blocks only when a frame arrives with no drawable
  free, which is what the frame-latency setting in decision 0 addresses.
- **Unmeasured:** Windows' native backend; a window moving between monitors with different
  refresh rates. VRR panels report their ceiling; the deadline is a floor on produce rate, so a
  narrower range costs at most one deferred wake per frame.

Time-based UX (an auto-dismiss timeout driven by a ticker) does not advance while a surface is
frozen. That belongs to a wall-clock timer service, not to pacing.

## Consequences

- A 165 Hz panel animates at 165 fps, not 60, and the event-loop thread never blocks on a
  sleep.
- Wayland gets compositor pacing and hidden-surface silence from the protocol rather than a
  timer.
- The pacing path is traced: `flui.gpu`'s `surface_acquired` (with the acquire duration),
  `pre_present_notified`, and `flui.pace`'s `fallback_armed` / `fallback_abandoned` /
  `segment_poll`. The live-smoke harness asserts one `pre_present_notified` per
  `present_submitted`.

## Alternatives rejected

- **A shorter fixed sleep.** Still uncorrelated with the display, and it still blocks input.
- **`WaitUntil(next_vsync)` computed from the refresh rate as the primary pacer.** A second
  pacing mechanism where the platform already provides one; decision 2 uses the period only
  as a fallback deadline.
- **Deriving the fallback from `target_fps`.** Ties a floor to a value that is explicitly not
  pacing.
- **`CVDisplayLink` on macOS.** The measured tail was a swapchain pool-width effect, not a
  trigger-timing one.
