# ADR-0029: Frame pacing — swapchain-block with fallback throttle

*Steady-state frame pacing comes from the GPU's blocking Fifo present, not from a fixed-duration sleep in the frame-drive loop; a coarse fallback throttle covers only the frames that never reach `present()` while a ticker keeps the loop awake.*

---

- **Status:** Accepted
- **Date:** 2026-07-17
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-engine/src/wgpu/renderer.rs` (`select_present_mode`, `Renderer::render_scene`), `crates/flui-platform/src/platforms/winit/platform.rs` (`WinitApp::about_to_wait`), `crates/flui-app/src/app/{runner.rs,binding.rs,config.rs}`, `crates/flui-platform/src/traits/capabilities.rs`, `docs/ROADMAP.md` App.1
- **Related:** ADR-0027 (sanctioned leapfrog zones name *presentation architecture* explicitly — this ADR is exactly that category: Flutter's own scheduler/vsync binding is a different runtime model (`Window.scheduleFrame`/`onBeginFrame` driven by the engine's platform-specific vsync callback), not a behavioral oracle FLUI transcribes here per Prime Directive #2). This is itself a leapfrog-zone unit: no Flutter source is cited as the contract below — the contract IS the spec.

---

## Context

`docs/ROADMAP.md`'s App.1 phase named "true vsync-driven, on-demand pacing" as the one unmet exit criterion for application integration: `ControlFlow::Wait` appeared nowhere in `flui-app`/`flui-platform`, and the frame-drive loop's only defense against spinning was a **fixed-duration sleep** in `runner.rs`'s desktop frame callback — computed once from `AppConfig::target_fps` (default 60 → ~16.67 ms) and applied whenever a wake carried no "dirty" work but a ticker (e.g. a running `AnimationController`) kept re-requesting frames. That sleep:

- Was a coarse polling interval, not a real vsync signal — it could drift out of phase with the display's actual refresh cadence, and had no relationship to whatever monitor the window ended up on (60 Hz, 120 Hz, 165 Hz, …).
- Existed because the previous default present mode was `Mailbox` (uncapped, triple-buffered) — chosen for low latency — which meant `present()` never blocked, so *nothing* paced a ticker-driven frame without this sleep. The comment on the removed code cited an observed **~30 000 fps busy-spin** with `Mailbox` and no sleep — the failure mode this ADR closes properly instead of papering over.

## Decision

**Pacing moves from a userspace sleep to the GPU's blocking present call, with `Fifo` as the default present mode.** Four changes land together:

1. **`select_present_mode` defaults to `Fifo`**, dropping the previous `Mailbox`-if-available preference. `Renderer::render_scene`'s `output.present()` under `Fifo` blocks until the next vsync for every frame that actually presents — that block *is* the steady-state pacing mechanism now. `Mailbox` remains available in `wgpu::PresentMode`'s surface-capability set and is documented as a future *opt-in* for latency-sensitive apps willing to trade pacing for responsiveness; it is not reachable from `flui-app` today.
2. **`WinitApp::about_to_wait` sets `ControlFlow::Wait` explicitly**, every iteration, even though it is winit's own documented default. This is a defensive pin, not new behavior: if a future winit release changed its default (e.g. toward `Poll`), the wake-driven frame loop would silently regress into a busy poll with no compiler or CI signal — an explicit `set_control_flow` call turns that into a reviewable one-line diff instead.
3. **The fixed `frame_budget` sleep in `runner.rs`'s desktop frame callback is deleted.** In its place, `render_scene`'s return type changed from `Result<(), EngineError>` to `Result<bool, EngineError>` — the bool is whether the call actually reached `present()` — threaded up through `RasterBackend::render_scene` and `AppBinding::render_frame_entered` to the frame-drive closure. A **coarse fallback throttle** (`no_present_fallback_pace`, a fixed ~1/60 s sleep) applies only when a frame did **not** present (no damage, an occluded surface, or a lost surface — none of which block on vsync) **and** something is still going to wake the loop again regardless (`AppBinding::needs_redraw()` or `Scheduler::is_frame_scheduled()` — i.e. a ticker/animation keeps the gate open). Without this fallback, a repeating ticker behind a window that never presents (minimized, occluded, or mid-`SurfaceLost` retry) busy-spins exactly like the pre-`Fifo` `Mailbox` case, because no vsync block ever engages for a frame that skips `present()`. The gate-decision itself (`should_render_frame`) and the fallback decision (`no_present_fallback_pace`) are pulled out as pure, unit-tested functions — `run_desktop` opens a real window and GPU device and cannot run headlessly, so the decisions it makes were extracted specifically to stay testable without one. This throttles; it does not pace — an un-presented frame carries no vsync signal, so the bound is a fixed CPU-time cap, not frame-accurate cadence.
4. **`AppConfig::target_fps` is documented as advisory, not enforced**, with every consumer audited: it is logged at startup (`target_fps_advisory`, informational only), read by `EmbedderScheduler::stats` (unwired scaffolding, not reachable from the running app, and from a *separate* `flui_scheduler::Scheduler` instance unrelated to this field), and independently duplicated by `PlatformCapabilities::default_target_fps` (a platform-reported hint nothing currently reads into this field — `AppConfig::default` hardcodes `60` regardless of platform). None of these drive the pacing model above; the audit exists so none of them keep implying otherwise.

### Occlusion semantics

- **Wayland:** compositors deliver frame callbacks only while a surface is visible — an occluded/minimized window's tickers effectively freeze (no wake arrives to drive them), which is battery-correct behavior and requires no special handling here.
- **Windows/X11 minimized:** redraws can still be delivered even while occluded; the no-present fallback throttle (point 3 above) is what bounds those redraws instead of letting them busy-spin.
- **Either way, time-based UX does not progress while frozen.** A `SnackBar` auto-dismiss timer (or any other timeout-shaped animation) driven by a ticker that stops receiving wakes does not advance during that freeze. This ADR does not fix that — it is a correctness gap belonging to a future **Timer service** (a wall-clock-driven timeout mechanism independent of the frame-tick gate), named here so it is not mistaken for something this change already solves.

## Evidence

Measured 2026-07-17 on this machine: Ubuntu 26.04, Wayland session (`XDG_SESSION_TYPE=wayland`), primary display `DP-1` at its native **164.89 Hz** refresh rate (`xrandr`), NVIDIA GeForce RTX 3070 Ti via the Vulkan backend. `cargo run -p flui --example animated_box_app` with `FLUI_FRAME_HISTOGRAM=1` (the example's built-in inter-tick histogram, added alongside this change — see its module doc), run for 15 s while the `AnimationController` bounced continuously (this demo has no idle state; see below):

| Window | sample_count | median (ms) | p90 (ms) | max (ms) |
|---|---|---|---|---|
| 1 | 300 | 6.058 | 6.149 | 13.487 |
| 2 | 300 | 6.059 | 6.107 | 7.811 |
| 3 | 300 | 6.058 | 6.105 | 7.683 |
| 4 | 300 | 6.056 | 6.117 | 7.414 |
| 5 | 300 | 6.058 | 6.097 | 7.079 |
| 6 | 300 | 6.059 | 6.091 | 11.989 |
| 7 | 300 | 6.058 | 6.110 | 7.062 |

The median (~6.058 ms) matches the monitor's native period (1 / 164.89 Hz ≈ 6.065 ms) to within measurement noise across all 2100 samples, with p90 within 0.1 ms of median and no sleep-quantization artifact (a fixed-sleep implementation would show a visible floor near the sleep duration, not a value tracking the *display's own* refresh rate). This is direct evidence that `Fifo` present is genuinely blocking on the real display's vsync, not on `target_fps`'s advisory 60, and not busy-spinning. The two occasional double-refresh outliers (~2×period, likely a missed vsync deadline under system jitter) are within the tolerance this ADR expects from a real desktop compositor, not a regression.

**What this evidence does and does not cover:**
- **Covered:** steady-state during-animation cadence, on Wayland, on this GPU/backend. This is the primary claim App.1's exit criterion cares about.
- **Not captured in this run:** an idle (zero-frame) measurement — `animated_box_app`'s controller `repeat()`s forever by design and has no path to stop, so it cannot demonstrate idling. That invariant is instead pinned at the unit level (`idle_wake_with_no_dirty_work_and_no_scheduled_frame_renders_nothing`, `crates/flui-app/src/app/runner.rs`) and, at the gate level, by the pre-existing `vsync_continuation_keeps_gate_open_while_running_and_closes_on_settle` (`crates/flui-app/src/app/binding.rs`), which proves the redraw gate closes once a controller settles.
- **Not captured:** the minimized-window-with-a-live-ticker fallback path, live. Attempted on this Wayland/GNOME session — `xdotool` cannot address a native Wayland surface (no Xwayland fallback for this window), and no compositor-IPC tool was available to minimize it programmatically; deemed infeasible within this ADR's evidence pass rather than skipped silently. The fallback throttle's *decision logic* is instead proven by `no_present_fallback_bounds_repeating_no_present_wakes` (`runner.rs`), a bounded-wall-clock-window simulation using the exact same predicate + `thread::sleep` pairing the production closure calls — confirmed to catch the regression by manually deleting the fallback in place and rerunning the test (7.6M iterations in 80 ms vs. the assertion's ceiling of 50) before restoring it.
- **X11 native:** not separately run — this session's only display backend reachable from `flui-platform`'s winit integration is Wayland (`WAYLAND_DISPLAY` set, `XDG_SESSION_TYPE=wayland`); no native X11 session was available on this machine to compare.
- **Native (non-winit) backends and other platforms:** out of scope, deferred to Cross.P (`docs/ROADMAP.md`) per that phase's platform-breadth charter.

**The histogram did not disprove `Fifo` blocking** — the pre-committed fallback below was not needed.

## Alternatives rejected

- **`WaitUntil(next_vsync)` pacing computed from `PlatformDisplay::refresh_rate()`.** This was the pre-committed Plan B if the evidence above had shown `Fifo` still spinning uncapped (e.g. ~30 000 fps) on this stack. `flui-platform` already exposes the needed primitive (`PlatformDisplay::refresh_rate`, backed by winit's `refresh_rate_millihertz()`), so it remains buildable without new infrastructure if a future platform/backend combination needs it. Not adopted now because the evidence above shows `Fifo` blocking correctly on the lead platform (Linux/Wayland/Vulkan) — this alternative would have added a second, redundant pacing mechanism the primary one doesn't need. Revisit per-platform if a Cross.P backend (native Windows/macOS, or a non-Fifo-respecting driver) shows the busy-spin symptom this ADR was written to guard against.
- **Keep the fixed frame-budget sleep, just shrink it.** Rejected: a smaller fixed duration is still a coarse guess uncorrelated with the display's actual refresh rate, and does nothing to remove the redundant pacing layer once `Fifo` present is doing the real work — it would just make the redundancy less visible, not resolve it.
- **Derive the no-present fallback's duration from `AppConfig::target_fps`.** Considered, rejected in favor of a fixed ~1/60 s constant: `target_fps` is explicitly advisory (point 4 above) and this is a coarse CPU-time throttle for a degenerate path (no damage / occluded / lost surface with a live ticker), not real pacing — tying it to a value the rest of this ADR spends effort clarifying is *not* pacing would reintroduce the exact confusion being closed.

## What is untouched

Prime Directive #1 (behavior loyalty) does not apply here — this whole area is a sanctioned leapfrog zone (ADR-0027: presentation architecture). No Flutter scheduler/vsync source was consulted or claimed as a behavioral reference for this change; the contract in this ADR is the spec.
