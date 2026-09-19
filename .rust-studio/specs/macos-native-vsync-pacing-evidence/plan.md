# Plan — macOS native (AppKit) vsync-pacing evidence, and the frame-source defect blocking it

Status: **the pacing evidence this task was opened for is taken and recorded (§2.8), on a real
Mac (2026-09-17), and the mechanism behind it was then measured directly and corrected (§2.9).**
The residual defect §2.6 named is **closed** — and by a *different mechanism than the plan
predicted*, with the attribution recorded rather than blurred (§2.8, §5). §2.9 goes one further:
it shows the pacing mechanism §2.8 attributed to the `Fifo` present is wrong for this backend
(the present is 42 µs; the period is AppKit's display-pass cadence), and it **diagnoses** the
3–4 % missed-deadline tail §2.8 had left undetermined — those frames stall ~9 ms inside their own
pipeline before the present. §2.7's two gaps are one closed and one retracted as disproved by
measurement. Apple layer only (macOS/AppKit), per the standing constraint that this machine
cannot build or test the Windows/Linux backends.

## 1. Task and where it comes from

`docs/ROADMAP.md:303` (the App.1/Cross.P named gap):

> Native (non-winit) backend vsync-pacing evidence (Windows/macOS) — deferred to Cross.P; the
> wgpu-side `Fifo` mechanism is backend-agnostic, but only the Linux/Wayland path has been
> measured so far.

*The quote is kept as the state this task opened from; as of 2026-09-17 the line itself has been
rewritten — the macOS half is MET and recorded (ADR-0029's "macOS native (AppKit)" subsection), so
only Windows remains deferred.*

The measurement it mirrors (`docs/ROADMAP.md:261`, ADR-0029) was taken on a winit-backed Wayland
window: median inter-tick cadence 6.058 ms against a 6.065 ms native period, 2100 samples,
ADR-0029 recording the caveat *"this evidence is Linux/Wayland/Vulkan only"*.

Taking the macOS half turned up a defect in front of the measurement: the native backend had no
frame source that could sustain a loop, so there was nothing to pace.

## 2. What was established empirically

### 2.1 The native backend rendered exactly one frame, then froze

`examples/animated_box_app` (continuous animation, `Vsync`-registered controller) staged into a
minimal `.app` and run on the native AppKit backend: 5+ runs, two launch routes (direct exec and
`open`/LaunchServices), every one of them ~20 redraw requests, **1 `drawRect:`**, 1 attempted
frame, `outcome="occluded"`, nothing presented, window blank indefinitely.

### 2.2 The pump is `drawRect:`, and it cannot re-arm itself

`request_redraw()` → `setNeedsDisplay:YES` on the content view → AppKit display pass →
`drawRect:` → `callbacks.dispatch_request_frame()`. That is the only pump; there is no
`CVDisplayLink`, no `CADisplayLink`, no `NSView.displayLink(target:selector:)` anywhere in
`flui-platform`. The frame runs *inside* `drawRect:`, so the next frame is requested from inside
the display pass.

An isolated AppKit probe (no FLUI, `probes/probe3.m`) measured what a `setNeedsDisplay:` issued
there does, on a window that is fully visible in every arm (`occlusionState` = 8194):

| re-arm issued from inside `drawRect:` | draws over 4 s |
|---|---|
| `[view setNeedsDisplay:YES]` | 1 |
| nothing at all (control) | 1 |
| `[view.layer setNeedsDisplay]` | 1 |
| `setNeedsDisplay:` deferred to the next main-queue turn | **396** |
| timer poke (`setNeedsDisplay:` from outside the pass), control | 17 / 16 requests |

`[view needsDisplay]` reads **NO** immediately after the in-pass call — discarded, not deferred.
`[view.layer needsDisplay]` reads **YES** while still never being serviced. So the in-pass call is
dropped by AppKit, the layer route does not rescue it, and deferring the same call by one turn of
the lane is what re-arms the view. **`drawRect:` cannot self-sustain a frame loop** — that is the
whole reason every run got exactly one frame, and it is a property of AppKit, not of FLUI's view
setup.

### 2.3 Ruled out, each by a direct experiment

The CAMetalLayer/`raw_window_material` layer question (the view stays layer-*backed*, not
layer-hosting; arms 3/4/6 of the first probe all drew 17 times); the `AnimationController has no
ticker` warning (`without_ticker` + `Vsync` is the intended shape, and the continuation wake
provably fired); a realm-vs-presentation `Vsync` mismatch; a stuck `DispatchDrain`; the
liquid-glass `setContentView:` path; and **"the window is never granted visibility"** — the run-12
trace shows the visible bit arriving (`occlusion=8192` → `8194` with `is_key=true`,
`app_active=true`) once the app activates, so visibility was never the gate. Activation was also
falsified directly: an `open`-launched run had `app_active=true`/`is_key=true` from the start and
froze identically.

Frame body, from the run-12 trace (one poked cycle): requests → `DRAW` at `:58.234` → `ACQ` at
`:58.234` (`acquire_us=705`) → `present_submitted` at `:58.236` (`present_us=69`) → both re-arm
requests at `:58.234`/`:58.236`, i.e. **inside** the pass. The frame is ~2 ms of work entirely
inside `drawRect:`; the present is sub-millisecond.

### 2.4 The fix

Three pieces, all in `crates/flui-platform/src/platforms/macos/`:

- **`display_pass.rs`** (new) — a thread-local marker plus an RAII guard
  (`DisplayPassGuard::enter()`) that restores the previous value on drop, so nesting unwinds
  correctly. Thread grain, not per-window: "displaying" is a property of the thread AppKit is
  displaying on, which makes the decision conservative in the safe direction (a deferred
  `setNeedsDisplay:` is always honoured; it costs one lane turn at worst).
- **`view.rs::draw_rect`** enters the guard for the frame it dispatches.
- **`window.rs::dispatch_redraw_request`** (new, free-standing, AppKit-free) is the single
  decision point `request_redraw` calls: inside a pass it starts the deferral, otherwise the
  inline send. **`defer_redraw`** hands the `setNeedsDisplay:` to the next owner-lane turn via
  `exec_async_guarded` → `route_on_owner`, reaching the window through the shared window map by
  pointer key (no raw `id` captured, no `Weak` self-cycle). The deferred turn **re-checks the
  marker** before messaging AppKit and re-defers, bounded at `MAX_DEFER_HOPS = 4`: a display pass
  that opens a nested run loop services the same lane from inside itself, so a turn can land in a
  pass again, and messaging AppKit there would discard the request silently — exactly the stall
  the fix exists to remove. At the cap the request is dropped with a warning that names the
  situation, rather than retrying unbounded (which would spin at the lane's full rate while the
  nested loop drains it).

This is ADR-0039 §4(c) realised on AppKit — the frame transaction is a region the drain gate is
closed for, and a wake arriving while it is closed defers rather than draining. The change is
contract-affirming, not an ad-hoc patch.

Measured on the same demo, same machine, with a 2 s diagnostic poke that starts the loop and
cannot mask it (that instrument has since been **deleted**, see §4):

```
Counter({'REQ deferred=true': 1991, 'DRAW': 660, 'ACQ success': 657, 'PRESENT': 657,
         'REQ deferred=false': 7, 'ACQ occluded': 1})
```

**657 frames presented where every previous run presented zero**, driven by 1991 deferred
re-arms. The loop is self-sustaining and vsync-paced once it is running.

### 2.5 Pacing evidence

The display is **100 Hz**: 3440×1440, `maximumFramesPerSecond=100`,
`CGDisplayModeGetRefreshRate` = 100.000 Hz → native period **10.000 ms**.

Two long runs, same build, same machine, raw present traces kept as
`evidence-run2-presents.log.gz` / `evidence-run3-presents.log.gz` (statistics recomputed from
those logs, not from memory):

| statistic | run 2 | run 3 |
|---|---|---|
| presents / span | 3488 / 35.35 s | 3720 / 37.54 s |
| mean fps | 98.7 | 99.1 |
| median inter-present | **10.189 ms** | **10.233 ms** |
| mean | 10.137 ms | 10.094 ms |
| p90 | 11.108 ms | 11.130 ms |
| min / max | 2.774 / 33.201 ms | 5.746 / 32.057 ms |
| median ÷ native period | **1.0189** | **1.0233** |
| deltas within 9–11 ms | 2469 / 3487 | 2483 / 3719 |
| deltas > 20 ms | 3 | 1 |

**This supersedes an earlier, flattering number.** A first run over a 6.78 s window gave median
10.000 ms and ratio 1.0000; the two long runs above give 10.189/10.233 ms and ratios 1.019/1.023.
The honest headline is the long-run figure: on this stack the present is paced *near* the display
period, ~2 % long, not locked to it — and the sub-period minima (2.8–5.7 ms) say why, since a
`Fifo` present that blocked on every frame could not return in a quarter of a refresh. Compare
the Wayland reference: median 6.058 ms against a 6.065 ms native period, ratio 0.9988. The
mechanism is the same (wgpu `Fifo`); the macOS figure is measurably looser, and that difference is
recorded rather than rounded away.

**Scope limit:** these runs are from a build that still carried the diagnostic poke, which is why
the loop started at all (§2.6). Once started the cadence is self-sustained and unassisted — the
probe in §4 proves the re-arm chain sustains frames on its own — but the startup path is still
broken and is named as such.

### 2.6 The residual: liveness still needs an external tick

With **no** external tick at all, the loop does not start: 3 draws, 0 presents, in 3/3 runs —
perfectly reproducible. The original stated mechanism — "the first frames run while the window is
still not displayable, such a frame is *skipped* (no present, and therefore no re-arm)" — is
**wrong in the specific that matters, and the run-12 trace shows why.** What that trace measures,
re-read 2026-09-17:

```
14.031550  drawRect: dispatching frame request          ← cold-start display pass #1
14.032160    Surface occluded; skipping frame
14.032186    Frame skipped: no damage or surface occluded (no present)   frame=1
14.032199    wake_frame: platform window request_redraw sent             ← RE-ARMS ANYWAY
14.041348  Window gained focus                                          ← activation is NOT the gate
14.047648  drawRect: dispatching frame request          ← display pass #2
           … 1.92 s in which this build emits NOTHING AT ALL …
15.967669  drawRect: dispatching frame request          ← the deleted 2 s diagnostic poke
```

So the not-displayable frame **does** re-arm — a redraw request goes out 13 µs after it, which is
what produces display pass #2 — and activation is not the gate either, since pass #2 lands 6 ms
*after* the window gained focus. The stall is on the wake **after** the skipped frame: pass #2
produces no trace output of any kind, and nothing arrives for the next 1.92 s.

**Why pass #2 does nothing, read from the code rather than inferred:** every other wake path in
`desktop.rs`'s frame closure leaves a trace, so a wake that emits nothing is one that reached
`WakeAction::Skip` and `return`ed — the one arm with no logging (§2.6.1 adds the trace that will
make this a reading instead of an inference). `Skip` is reachable only with `frames_enabled && !dirty &&
!frame_scheduled`, and all three hold:

- `frames_enabled` is true — a fresh realm's scheduler starts `Resumed` (`desktop.rs:807-809`, the
  bootstrap's own `Resumed` dispatch is a documented no-op), and the macOS backend dispatches no
  lifecycle signal at all, so **hypothesis (2)'s "frames disabled" has no producer on this
  backend** — `PumpAsync` is unreachable here.
- `!dirty` because the previous frame ran the whole pipeline and took `SubmitVerdict::NoPresent`,
  which reaches `ui_realm.rs:3042`'s `else` branch → `mark_rendered()` clears `needs_redraw`
  (**the damage is consumed by a frame that never reached the screen**), and `retry_needed` is set
  only by the `SurfaceStale` and pipeline-`Errored` arms, never by `NoPresent` — so nothing
  re-dirties and nothing is poked.
- `!frame_scheduled` because the demo's controller warns `AnimationController has no ticker; the
  animation will not advance` (14.031901) — no ticker is registered, so no scheduled demand.

Then `Skip` returns without running a frame and without re-arming, AppKit has no pending display,
and the loop is dead. One external nudge fixes it permanently for the life of the process,
because from then on every frame is displayable and every frame re-arms.

**This is a third mechanism, not one of the two §2.6 originally named** — and it is narrower and
more actionable than "a frame that ends without re-arming": *a frame that consumes the damage
without presenting it, leaving the following wake with nothing to do.* Two consequences the tick
slice must not get wrong:

- **A display link does not subsume this.** A CVDisplayLink tick arriving while the surface is
  occluded still finds `dirty == false` (the damage was already consumed) and still resolves to
  `Skip`, forever. The damage loss needs its own fix; the link only removes the *dependence on
  AppKit asking again*.
- **The fix has a shape already in this codebase.** `NoPresent` currently collapses three
  distinct causes — genuinely nothing to draw, surface occluded, surface released — because
  `acquire_surface_texture_with` returns `Ok(None)` for both `Occluded` and `Released`
  (`flui-engine/src/wgpu/renderer.rs:109-121`) and `render_scene` reduces that to `Ok(false)`.
  Only "nothing to draw" means the damage was legitimately consumed; the other two mean it was
  lost. Distinguishing them is the precondition for the fix, and it is engine-side.

Two honest limits on the statement above, both open:

1. **The `Skip` classification is inferred from an absence, not yet measured** — pass #2's zero
   output is consistent with `Skip` and with nothing else in that closure, but it is still an
   absence. The instrumented run below converts it into a positive reading.
2. A display link — a tick delivered every refresh regardless of window state — is the fix for
   this half, and is also the "true vsync-driven tick" the backend does not have today. ADR-0044
   names `CADisplayLink` as macOS's intended produce signal; ADR-0045 is **Proposed** and keeps
   macOS on `RasterMode::Inline` until the locked `wgpu-hal`'s Metal path stops messaging
   `NSView`/`NSWindow` from the calling thread, so this half is partly upstream-gated.
   `NSView.displayLink(target:selector:)` (macOS 14+) is the modern, non-deprecated spelling and
   needs a guarded `respondsToSelector:` probe plus an explicit `MACOSX_DEPLOYMENT_TARGET`
   decision. **Corrected by the re-analysis above:** the link is *one* of the two fixes this
   residual needs, and on its own it is inert — see the first consequence bullet.

### 2.6.1 The measurement that settles it

One run, no external tick, no input, on the current tree (which no longer carries the deleted
diagnostic poke, so an unassisted cold start is what it produces by construction):

1. **Instrument the gate, permanently.** `wake_action` (`runner/frame_pacing.rs`) is where every
   wake resolves, and today only the `Render` arm leaves any trace — a loop that stops is a wake
   that resolved to `Skip` or `PumpAsync`, so a stall and a platform that simply stopped
   delivering wakes are indistinguishable in the logs. A `flui.pace` `trace` carrying the resolved
   action plus its four inputs (`frames_enabled`, `dirty`, `frame_scheduled`, `fallback_pending`)
   closes that gap at the one place all three backends share. Trace level, not debug: it fires
   once per platform wake, which on a healthy loop is the display rate.
2. **Run the demo cold.** `target/debug/examples/sliver_demo` — the binary the existing traces
   came from (`Starting FLUI application title=FLUI App size=800x600`, `examples/sliver_demo.rs`'s
   `flui::run_app(SliverDemoApp)`), driven with `RUST_LOG=flui.pace=trace,flui_platform=trace,
   flui_engine=trace,flui_app=trace` for ~8 s with no input, then killed — the same shape the
   run-12 trace was captured with, since this backend has no self-close hook
   (`FLUI_SELF_CLOSE_AFTER_MS` is winit-only).
3. **Read the last wake.** Predicted, and falsifiable: the final `wake_resolved` line reads
   `action=Skip frames_enabled=true dirty=false frame_scheduled=false fallback_pending=false`,
   preceded by exactly one `Surface occluded; skipping frame`. If it instead reads `Render`, the
   damage-loss reading is wrong and hypothesis (1)'s "skipped frame" stands as originally stated.
   `PumpAsync` would refute the `frames_enabled` argument outright.

**MEASURED 2026-09-17. The prediction is refuted, and the mechanism is upstream of `wake_action`.**

Three cold runs of `target/debug/examples/sliver_demo` (~8 s, no input, killed; two of them
byte-for-byte identical). Instrumentation added permanently in the same pass: `wake_action` now
emits `flui.pace`/`wake_resolved` (`frame_pacing.rs`), and the frame tail emits
`flui.pace`/`frame_tail` (`ui_realm.rs`, at the `if retry_needed` decision).

- **`wake_resolved` fires exactly twice per run, both `action=Render
  frames_enabled=true dirty=true frame_scheduled=true fallback_pending=false`.** No `Skip`, no
  `PumpAsync`, ever. The last wake is `Render` — so the predicted signature does not exist, and the
  §2.6 sentence "a frame that consumes the damage … leaving the following wake with nothing to do"
  is **wrong about there being a following wake at all**.
- `frame_tail presented=false any_failed=false retry_needed=false` on **both** frames. The tail's
  retry arm is not taken, `mark_rendered()` runs, and **no wake is armed** — by the realm, in either
  frame. (An earlier reading of run 1 inferred frame 1 *did* have `retry_needed=true`; the trace
  added to settle it disproved that, which is why it was added rather than argued.)
- 2 `drawRect:` passes, 2 `Surface occluded; skipping frame`, 2 `Frame skipped: … (no present)`,
  **0 `Frame rendered successfully`**.
- **The window is not occluded by any AppKit measure**: the backend's own trace at that instant
  reads `window_visible=true occlusion=8192 view_in_window=true is_key=true app_active=true
  alpha=1.0 backing_scale=1.0`. `8192` is `NSWindowOcclusionStateVisible`. The `Occluded` verdict is
  `wgpu::CurrentSurfaceTexture::Occluded`, returned by `get_current_texture()` — **no FLUI-side
  occlusion flag is involved**, so there is no stale flag to clear.
- **It is a transient, and the stored run proves it.** `evidence-run1-frames.log.gz` contains
  exactly **one** `Surface occluded` and then **2593** `Frame rendered successfully`. The surface
  becomes available within milliseconds of the cold start.
- After the second frame the log ends mid-tail (no `wake_frame`, no `fallback_armed`) and nothing
  further is emitted for the remaining ~7.9 s. `sample` on the live process at +4 s: the main thread
  is parked in `-[NSApplication run]` → `_nextEventMatchingEventMask:untilDate:inMode:dequeue:`,
  **no flui frame on the stack**, process state `SN`, 0.0 % CPU. The frame *completed*; the run loop
  is blocked with nothing scheduled.
- 234 `wake_frame` calls in the run — all in frame 1 (a burst during the cold build), **zero** in
  frame 2. 238 `request_redraw: setNeedsDisplay` sends, landing after frame 1's pass. Exactly **1**
  `fallback_armed` (frame 1, `in_us=9500`) — and **nothing ever actuates it**.

**The mechanism, as measured.** (1) The cold build's redraw marks call `wake_frame()` while inside
the display pass; `dispatch_redraw_request` defers them; they land as a `setNeedsDisplay:` burst
after the pass and produce frame 2. (2) Both frames find the surface `Occluded` and present nothing,
and the `NoPresent` contract consumes the work and correctly arms no retry. (3) Frame 2 has no marks
of its own — the tree is built and `mark_rendered()` cleared the flag — so nothing re-arms. (4)
Nothing on the backend actuates the loop: the fallback deadline is armed but never consulted
(`set_wake_deadline_hook` is a no-op on AppKit) and there is no display link. **The loop parks, and
the occlusion that parked it is gone milliseconds later.**

**What this does to §6 item 2 — the split survives, but both halves change meaning.**

- **2(a) is NOT withdrawn — but its justification inverts.** The claim that the last frame "holds
  damage it should have kept" is wrong in the form it was written: `retry_needed=false` is the
  *correct* answer for the `NoPresent` arm, and arming a retry unconditionally would repaint a
  genuinely occluded window at ~62 Hz — the mistake already rejected once. What the run does show is
  a **state nobody intended**: the demo built its tree, produced a scene, never showed it, and is
  now idle *and blank* — frame 2 reached neither `fallback_armed` nor any wake because
  `keeps_gate_open` (`needs_redraw || is_frame_scheduled || has_pending_work`) was false. So the
  surviving question is narrower than "carry *why* there was no present": it is **whether a frame
  that was never shown may clear `needs_redraw`**. Retaining it costs nothing (an empty tree retains
  nothing, so the normal "no damage" `Ok(false)` is unaffected) and needs no engine-signature change
  to be safe — but it must be stated and tested as a rule, not assumed.
- **2(b) is necessary, and is measurably not sufficient on its own.** The actuator is what is
  missing (the `fallback_armed` deadline is armed and never consulted), but a wake alone does not
  render: with `dirty=false` and no scheduled ticker, an actuated deadline resolves to `Skip` — the
  very arm §2.6.1 originally predicted, now correctly located *after* the fix rather than before it.
  A display link on an occluded-but-retained window would otherwise tick into a permanent no-op.
  The two halves therefore compose: **retain the damage (a), and give the loop something that comes
  back (b)**; either alone leaves the window blank or the loop spinning. Occlusion must be the
  link's start/stop switch for the same reason, and the one-shot synchronous frame on re-activation
  (gpui's `activated_least_once`) covers the cold start whose transient clears before any tick.

**Also settled, and it removes work from the list:** the `Occluded` verdict needs no re-evaluation
plumbing. It is wgpu's `get_current_texture()` drawable state, not a FLUI flag, and it clears by
itself — so there is nothing to invalidate on focus/visibility.

#### 2.6.2 The actuator on AppKit — shape, and the one thing that decides it

Settled while scouting 2(b), recorded here because each point cost a wrong turn to find:

- **The deadline already reaches the platform hook.** `desktop.rs:333`'s closure folds
  `device_recovery_backoff.next_attempt_at()` and `fallback.next_wake(now)` through
  `merge_wake_deadlines` into `install_wake_deadline_hook`, and `FallbackWake::next_deadline`'s own
  doc says it exists to feed "the platform's wake-deadline hook". So 2(b) is *only* actuation — the
  plumbing above it is complete and shared.
- **It must run on the owner thread, and this is not a style preference.** The installed hook body
  re-enters `APP_RUNTIME`, which is **thread-local** (`runtime.rs`'s `FrameWakeHandle` doc says so
  explicitly, and `install_wake_deadline_hook` calls `APP_RUNTIME.with(...)`). A background thread
  consulting the hook would find no runtime and answer `None` forever. That rules out the obvious
  "a deadline thread parked on a condvar" design outright — so no new thread, and no GCD global
  queue.
- **`dispatch::Queue::exec_after` (main queue) is the right primitive**, not a hand-rolled loop.
  `owner_queue()` is already `dispatch::Queue::main`, the main queue is drained by
  `-[NSApplication run]` on the main thread, and `exec_after(&self, delay, F)` is a one-shot
  delayed block — so `[NSApp run]` stays exactly as it is, and no `objc::declare` target class is
  needed (which an `NSTimer`/`performSelector:` route would require). Android is the in-workspace
  precedent for actuating this hook in a backend without `ControlFlow::WaitUntil`; it actuates
  inside its own loop, which AppKit does not expose.
- **Where the timer is re-armed — the chain cannot re-arm itself alone, and this is the one seam
  it buys.** Two rejected first answers, then the shape that works:
  - *A post-frame trigger in `view.rs::draw_rect`* (the AppKit analogue of winit's `about_to_wait`)
    is unreachable: nothing in `view.rs` can see `MacOSPlatform`. `ViewContext` holds only the
    per-window `WindowCallbacks`, `MacOSWindow` is handed the window map and never the platform,
    and the macOS module has no global platform handle.
  - *A module-level `OnceLock<Weak<MacOSPlatform>>`* is the wrong shape for this repo specifically:
    the workspace has spent several releases deleting ambient singletons (`impl_binding_singleton!`,
    #553) and ratchets the surviving process-globals by symbol in `runtime-contract.toml`, so a new
    one would be a regression against its own direction.
  - *A chain that simply stops when the hook answers `None`* does not close: a frame arms its
    fallback deadline **after** the redraw request that caused it, so a consult triggered by that
    redraw runs too early to see it — and at install time the hook answers `None` too, so the chain
    would never start at all.
  - **The shape that works: arm on the redraw request, and consult once more after it.** A
    `Weak<WakePump>` (holding the handlers `Arc` + a wake closure over the window map) is passed to
    `MacOSWindow` at construction — a constructor parameter from `open_window`, which already holds
    `self`, so it is an explicit dependency and not an ambient one. Every redraw request arms it;
    when the hook answers `None` it still schedules **one** follow-up consult a pacing period later,
    which reads whatever deadline the frame it triggered went on to arm. The chain is self-limiting
    in the way that makes it safe: it stops on a consult that finds nothing *and* requested no frame,
    so an idle app — which sends no redraw requests — never ticks at all. The cost is that a deadline
    is honored up to one period late, a latency inside the fallback gate's stated purpose (it exists
    to un-stall, not to pace; a healthy loop is paced by the display), not a correctness change.
- **What that buys, and what it deliberately does not decide:** with the actuator live, 2(a)'s
  retention rule becomes *observable and measurable* — including the failure mode the next bullet
  describes, which exists only as a derivation until there is a live loop to watch. That is the
  recorded reason (b) precedes (a).
- **The retention rule in 2(a) can only be tuned once the actuator exists**, for the reason in the
  bullet above: "retain `needs_redraw` on `NoPresent`" is unsafe as written, because a window that
  genuinely cannot present would then keep `keeps_frame_gate_open` true and arm a fresh 9.5 ms
  deadline on every pump — a ~105 Hz idle loop with nothing to show. The rule needs the *cause*
  (`Occluded` vs "no damage"), which is exactly the information `render_scene`'s two `Ok(false)`
  returns collapse and its own doc admits it collapses ("`false` covers every skip path... no
  damage, an occluded surface, or a surface the owner has released"). So the engine-side widening
  is **not** optional after all — the first reading of this measurement withdrew it too early.

#### 2.6.3 The actuator is IMPLEMENTED and MEASURED — and 2(b) alone is measured insufficient

Built 2026-09-17 as `crates/flui-platform/src/platforms/macos/wake_pump.rs` +
`MacOSPlatform::set_wake_deadline_hook` + `MacOSWindow::install_wake_pump`/`arm_wake_pump`
(armed from `PlatformWindow::request_redraw`). Gate-clean on this Mac: `cargo check`, `cargo clippy
--all-targets -- -D warnings`, `cargo fmt --check`, and 7 unit tests over the decision logic
(`tick_plan`, the generation guard, and the anti-starvation rule below).

- **It is live on the real path, which had to be checked rather than assumed.** `desktop.rs:56`
  calls `current_platform()`, and on macOS that returns `MacOSPlatform` (`flui-platform/src/lib.rs`,
  the `cfg(all(target_os = "macos", not(windows)))` arm) — so `install_wake_deadline_hook`
  (`desktop.rs:333`) *is* installed on this backend, and the `fallback_armed` deadline the earlier
  measurement saw had a hook that simply dropped it. The `lib.rs` doc's "macOS … stub" note is
  stale for this path and reads as a reason to check, not a reason to skip.
- **A redraw burst starves the pump, and only measurement showed it.** The first implementation
  re-scheduled on every arm. Measured: **241 `wake_pump_armed`, 0 `wake_pump_fired`, 3
  `wake_pump_idle`, 238 ticks superseded** — a cold start's redraw storm cancels its own tick every
  8 ms, so the deadline armed in the middle of the burst is never consulted at all. Fixed by making
  `arm` refuse to move a pending tick *later* (only ever earlier); the instant lives in a plain
  `AtomicU64` of microseconds since a fixed base rather than a `Mutex<Option<Instant>>` — the mutex
  form tripped port-check's `LockDiscipline/StatementDrop` on the assignment that drops the old
  value, and an atomic carries no invariant the lock was protecting anyway. Two unit tests cover the
  rule (`an_arm_never_moves_a_pending_tick_later`, `an_arm_earlier_than_the_pending_tick_replaces_it`).
  This is the first thing in this slice that was wrong in a way no amount of reading would have
  found.
- **The result: the actuator works, and it does not un-stall the loop.** Four cold runs of
  `sliver_demo` on the new build, same recipe as §2.6.1: `wake_resolved` = **2, 2, 3, 3** (baseline
  2); `Frame rendered successfully` = **1, 0, 0, 0** (baseline 0); `Frame skipped` = 1, 2, 2, 2.
  In run 2 the pump genuinely **fired** (`wake_pump_fired=1`) — a deadline came due, it requested a
  frame, and a third frame followed. It then stalled anyway. So 2(b) is real, reachable, and
  demonstrably able to produce a frame — **and it is not sufficient**, which is exactly the
  composed-halves conclusion this plan derived at §2.6.1 and now has measured rather than argued.
  The reason is visible in the same traces: the no-present frames end `retry_needed=false` and arm
  no deadline at all, so there is nothing for the actuator to honor.
- **One outlier, and it is NOT credited to this change.** A single run of the new build rendered
  **113 frames / 316 wakes / 316 drawRect** where every neighbouring run rendered 2–3. It did not
  reproduce in four subsequent runs and its cause is unknown (the surface-occlusion transient, or
  window activation/ordering at launch, are the candidates — the trace shows 1 `Surface occluded`
  in it against 2 in the stalled runs). It is recorded here as an unexplained observation, not as
  evidence for the fix: crediting it would be the "MVP reported as parity" failure this repo's
  Definition of Done names, and the four runs that disagree with it are the reason that failure is
  detectable at all.
- **What this does to §6 item 2: (b) is done, and (a) is now the only remaining blocker.** The loop
  has an actuator that fires on a real deadline; what it lacks is a reason to be woken, because a
  frame that produced content and could not show it consumes the work and clears `needs_redraw`.
  That is 2(a), and its "distinguish the cause of the no-present" requirement (§2.6.2's last bullet,
  the engine-side widening) is unchanged and now has a live bench to be tuned against.

#### 2.6.4 The 2(a) seam, located — and why it is a protocol change, not a local patch

Scouted 2026-09-17 while (b) was still being measured. 2(a) does not need new plumbing found; it
needs one collapse *undone*, at a known line.

- **The collapse is `DirectSink::submit`** (`crates/flui-app/src/app/raster_lane.rs:451`):
  `Ok(true) => Presented`, `Ok(false) => NoPresent`. Everything downstream then treats `NoPresent`
  as one thing: the frame tail reads it as "the work was consumed, nothing to come back for", so
  `retry_needed` stays false, the damage is cleared and no wake is armed. That is *correct* for
  `NoDamage` and *wrong* for an occluded surface — and the bool cannot tell them apart.
- **The engine is where the answer exists and is thrown away.** `RasterBackend::render_scene`'s own
  doc states the collapse deliberately ("`false` covers every skip path ... no damage, an occluded
  surface, or a surface the owner has released"), and the wgpu implementation collapses it at two
  separate `return Ok(false)` sites — `renderer.rs:1910` (no damage) and `:1916` (the `Occluded`
  outcome from `SurfaceAcquireOutcome::Occluded` and every other silent skip). So this is a widening
  of an existing distinction at the moment it is discarded, not new detection; `render_scene`'s
  private helper already returns `Ok(None)` for exactly the occluded case.
- **The shape:** `Result<bool, EngineError>` → `Result<PresentOutcome, EngineError>` with
  `Presented | NoDamage | NotShown`, where `NotShown` is "content was owed and could not be put on
  screen" (occluded, or a surface released for a suspend). `SubmitVerdict` gains the matching
  variant and the frame tail retains its damage on that variant alone — which is exactly the
  distinction §2.6.2's last bullet says the retention rule needs to be safe.
- **Obligations this slice carries, all of them mechanical and none optional:**
  - `render_scene` is a **monitored export** (`docs/runtime-contract.toml:212`), so the registry
    moves in the same change.
  - It is a **protocol-level contract change to a public trait**, which per Prime Directive #1 owes
    an ADR naming what is better and why — a backend that can say *why* it skipped is strictly more
    informative than one that can only say *that* it did, and the alternative (a second query method
    on the trait) doubles the failure modes in the window between the two calls.
  - Every implementor moves: the wgpu renderer, plus the in-crate test double at
    `raster_lane.rs:518` and every other `RasterBackend` impl in the tree.
  - Replacement tests: the occlusion skip must be distinguishable from the no-damage skip at the
    `DirectSink` boundary *and* at the wgpu renderer, and the frame tail must be shown to retain on
    one and clear on the other.
- **The one question still open, and it is a tuning question with a live bench:** how long
  `NotShown` retains. Unbounded retention is the ~105 Hz idle loop §2.6.2 derives, and it is now
  *measurable* rather than hypothetical — the pump fires on a real deadline, so a wrong rule shows
  up as a spinning loop in the `flui.pace` traces instead of as a silent stall. A bounded rule
  (retain while the surface has been continuously unavailable for no more than N attempts, then
  stop and let the next real event restart) is the shape to try first; ADR-0044 §7 already rejected
  a periodic poll once and this must not reintroduce one by the back door.

**The gpui-ce citation is now verified, and it was wrong in one specific way.** A reading pass over
fresh shallow+sparse clones (gpui-ce `main` @ `8e36ac0`, 2026-09-15; zed-industries/zed `main` @
`c24e309`, 2026-09-17) — cross-checked with `gh search code` — found:

- Both projects build macOS pacing on **`CVDisplayLink`, not `CADisplayLink`**
  (`crates/gpui_macos/src/display_link.rs:1`: `//! Frame pacing for macOS windows, built on
  CVDisplayLink.`). `CADisplayLink` / `NSView.displayLink(target:selector:)` appears **nowhere** in
  either repo. My earlier "gpui does it with CADisplayLink" recollection was false; the file exists
  but on a different API.
- **Neither codebase calls `setNeedsDisplay:` at all** — the only hit is
  `setNeedsDisplayOnBoundsChange(true)` on the `CAMetalLayer`, and there is no `drawRect` path in
  either (`0` hits). So the market-leading Rust macOS stack never encounters the defect this slice
  fixed, because it does not build its frame loop out of `drawRect:` + `setNeedsDisplay:` in the
  first place. **This is not evidence about AppKit's behaviour** — that claim rests on this slice's
  own four probe arms — it is evidence that the path FLUI was on is the one nobody else is on.
- Their design: one immortal `CVDisplayLink` per `CGDirectDisplayID` in a `static Mutex<Registry>`,
  windows subscribe rather than own, the link runs iff it has subscribers, and **occlusion is the
  master switch** (`start_display_link`/`stop_display_link` driven by
  `NSWindowOcclusionState::Visible`, re-armed on screen change). Frames render **directly** from the
  display-link callback on the main queue (a GCD `_dispatch_source_type_data_add` on main, merged by
  the link's output callback); `displayLayer:` is the second, AppKit-initiated entry, and both paths
  stop the link → render → restart it. There is also a **one-shot synchronous frame on
  re-activation**, gated on `activated_least_once` — gpui has the activation-stall fix that §2.6
  says FLUI lacks.
- The link is **leaked by design**, because `CVDisplayLinkStop` is asynchronous and releasing raced
  its io thread (upstream segfaults #32116 / ZED-7XR). That wart is exactly what `CADisplayLink`
  does not have, so this reads as support for ADR-0044's choice rather than a challenge to it —
  with the caveat that the market is on the deprecated-in-macOS-15 API and Apple's direction is the
  other one.
- One deferral *does* exist there, of a kind not to be confused with this slice's: upstream Zed
  PR #3592 (`f12510b8`, 2023-12-11) *"Defer drawing the window until the CoreAnimation
  `displayLayer:` method is called"* — deferring to a platform signal, not around a discarded
  `setNeedsDisplay:`. Core gpui's `deferring re-entrant window draw request` is its own comment's
  description of a **Windows-only** re-entrancy case, not an AppKit workaround.

**What the pass did not establish, and what is not claimed from it:** no Apple documentation was
consulted and nothing was compiled or run (source reading only), the clones are `--depth 1` so an
older revision containing `setNeedsDisplay:` cannot be ruled out, and `gh search code` indexes the
default branch with possible indexing gaps — a caveat on every "zero hits" above, though the clone
greps agree.

#### 2.6.5 The widening landed, and the open tuning question is CLOSED (2026-09-17)

**Landed.** `PresentDisposition { Presented, NoDamage, NotShown }` replaced
`Result<bool>` on `RasterBackend::render_scene`; `SubmitVerdict` gained the matching variant; the
frame tail retains on `NotShown` and clears on `NoDamage`. ADR-0068 records it, the registry entry
is `frame-disposition-distinguishes-withheld-from-idle` (state `partial`), and the four obligations
§2.6.4 listed are all discharged. The naming collision §2.6.4 flagged is real but harmless:
`flui_scheduler::frame_telemetry::PresentOutcome { Presented, Errored }` already existed, so the
engine's type is `PresentDisposition` — a different name for a different axis (did it reach the
screen, vs did it finish), and the two never meet in one match.

**The bounded rule, resolved by measurement rather than by the derivation §2.6.4 sketched.** The
§2.6.2 derivation predicted an unbounded retention would show up as a ~105 Hz idle loop. It was
right about the mechanism and wrong about the symptom, and the way it was wrong matters:

- Three cold runs with retention and **no** bound gave 240/137/150 presents against the baseline's
  1/0/0 — the blank-window failure is closed.
- But that is *not* an idle loop. The demo is a static 104-line tree with no ticker, and sampling
  the runs shows frames continue only while pointer events arrive: in the longest pointer-free gaps
  (269–415 ms) there are **1–2 wakes and zero presents**. The frame rate is the app servicing
  ~89 Hz of real pointer traffic from the mouse sitting over the demo window, which is also why
  `dirty=true` on every wake. A control run that turned retention OFF but left the classification
  intact reproduced the baseline exactly (0 presents, 3 wakes), so retention is what revives the
  window — but the *storm* is input, not retention.
- So the evidence **cannot** falsify the unbounded loop: the observed withdrawal lasted ~132 ms.
  The cap is therefore justified by the mechanism and carried by its own tests, not by a trace that
  does not exercise it.

**Why the platform cannot be the bound** (this corrects §2.6.4's working assumption, and the same
correction was applied to the source comment and the registry statement): AppKit's occlusion path
does terminate a genuinely occluded window — `windowDidChangeOcclusionState:` → `WindowVisibility`
→ `AppLifecycleState::Hidden` → `frames_enabled == false` → `wake_action` returns `PumpAsync`. But
that gate keys off `occlusionState`, while what withdraws the drawable is the swapchain's own
availability, and **the measured trace has them disagreeing**: `occlusionState` reported the window
VISIBLE (`8192`) for the entire ~132 ms the drawable was unavailable. Wherever they disagree and
stay disagreeing (inactive Space, display asleep) the gate never engages.

**The rule chosen:** `MAX_NOT_SHOWN_RETRIES = 128` consecutive withheld attempts, with the streak
held per-presentation on `PresentationState` and cleared by any frame that ends otherwise. Cold
starts measured with the cap in place withdrew for 2, 2 and 3 attempts (the coldest run seen, the
one that motivated the retention, took 12), so the cap **never engaged** — `streak` peaked at 3 and
the give-up arm never fired — while the window still came alive at 275/334/449 presents. It is a
backstop, not a limit the ordinary transient touches, which is the calibration to want given the
asymmetry: too large costs one bounded burst on a surface that never returns, too small restores
the blank window.

**Verification shape:** one mutant per rule, each killed by exactly one test — reverting
`retry_needs_repaint` fails `a_frame_the_surface_never_showed_is_retained_and_repainted`; widening
the cap to `u32::MAX` fails `the_withheld_retry_is_bounded_and_then_parks`; dropping the streak
clear from the `Presented` arm fails `a_presented_frame_clears_the_withheld_streak`. **Residual,
unchanged and stated in the registry:** the wgpu arm that supplies `acquired_surface = false` from
a real acquisition failure is read-reviewed, not executed — it needs a windowed surface with an
occluded drawable that no `flui-engine` test can construct.

### 2.7 Two further macOS gaps found on the way

1. ~~**`refresh_period()` is not implemented for macOS**~~ — **closed 2026-09-17.** It was trait
   default `None` (`traits/window.rs:290`), overridden only by winit, so the runner logged
   `frame-pacing fallback period period_us=16667 reported=false`: it assumed 60 Hz on a 100 Hz
   panel. Implemented from `CGDisplayModeGetRefreshRate` over the display's **current mode**
   (reached via `NSScreenNumber`; a rate of 0 → `None`), cached in `MacOSWindowState` beside
   `scale_factor` and refreshed wherever the scale is, and read at construction — safe there
   because the window is created at the origin, a point on a screen. Source chosen by reading
   winit 0.30.13's macOS `refresh_rate_millihertz` at the pinned commit, not from memory;
   `NSScreen.maximumFramesPerSecond` was rejected (macOS 12+ against an 11.0 deployment floor,
   and it reports the hardware maximum, which over-reports in a 60 Hz ProMotion mode); winit's
   `CVDisplayLink` fallback is deferred to the tick slice that links CVDisplayLink anyway.
   **Verified on the real display, real backend:** `just macos-frame-pump` reports
   `FRAME_PUMP_PROBE_REFRESH_PERIOD=reported period_us=10000 hz=100.0` — matching the independent
   `scale_order.m` probe's `CGDisplayModeGetRefreshRate` of 100.000 Hz on the same panel — with
   the pump still PASS at 301 frames / 100.2 fps. Three always-run tests on the extracted
   arithmetic; removing its guard fails one of them (mutation-checked). The durable record is
   the `## Mapping decisions` entry in `crates/flui-platform/ARCHITECTURE.md`.
   **Still open, named:** a display whose mode reports 0 has no fallback yet, and a mode change
   that does not move the window (a user switching refresh rate in System Settings) is picked up
   on the next move or resize rather than immediately, since this backend observes no
   screen-parameters notification. The `set_wake_deadline_hook` half (ADR-0058 decision 2) is
   **not** done — it remains the tick slice's work, so the *reported* period is now right while
   the platform-driven tick it feeds does not exist yet.
2. ~~**`backingScaleFactor` is read before the window is on a screen**
   (`window.rs:292`, before `makeKeyAndOrderFront:` at `:296`) → `scale: 1` on a
   Retina Mac.~~ — **retracted 2026-09-17, disproved by measurement.** This was
   an inference from the code's order, not an observation. `probes/scale_order.m`
   reports the window's screen `live` (not nil) and the scale **1.00 at all three
   stages** — after init, after `makeKeyAndOrderFront:`, after `center` — equal
   to the screen's own 1.00. The window is created at the origin, a point on a
   screen, so the read has a screen to read. The staleness reading is refuted by
   code as well: `handle_screen_changed` → `handle_backing_properties_changed`
   re-reads `backingScaleFactor` live from the window, updates the stored value,
   and dispatches a resize, so the cached scale cannot go stale after a move
   either. **No change was made, and none is owed.** Honest boundary: this
   machine has one display at scale 1.00, so a Retina or multi-display input is
   untestable here — but nothing in FLUI's own path produces one, since the
   creation frame is always on the main screen.

### 2.8 The pacing evidence itself (plan §6 item 4's deliverable)

Measured **2026-09-17** on this Mac — Apple M1, macOS, native **AppKit** backend
(`current_platform()` → `MacOSPlatform`; the log's own bootstrap lines read
`macOS platform initialized with AppKit` and `Selected GPU: Apple M1 … Backend: Metal`)
— on a **100.000 Hz** panel. Three independent sources agree on the period: the
`CGDisplayModeGetRefreshRate` probe (`probes/scale_order.m`, 100.000 Hz), the
runner's own report `frame-pacing fallback period period_us=10000 reported=true`
(the §2.7 `refresh_period()` slice working), and `just macos-frame-pump`'s
`FRAME_PUMP_PROBE_REFRESH_PERIOD=reported period_us=10000 hz=100.0`. Native period
= **10.000 ms**.

Instrument: `examples/animated_box_app.rs` with `FLUI_FRAME_HISTOGRAM=1` — the
*same* instrument ADR-0029's Wayland table was taken with, 300-tick windows,
three consecutive 45 s runs.

| run | windows | ticks | median-of-medians | ratio to 10.000 ms | range of window medians | presents | span | fps |
|---|---|---|---|---|---|---|---|---|
| 1 | 14 | 4200 | **9.97500 ms** | **0.99750** | 9.95338..9.99425 | 4299 | 44.352 s | 96.93 |
| 2 | 14 | 4200 | **9.96046 ms** | **0.99605** | 9.93346..9.99450 | 4318 | 44.815 s | 96.35 |
| 3 | 14 | 4200 | **9.96852 ms** | **0.99685** | 9.92925..10.00896 | 4279 | 44.704 s | 95.72 |

42 windows, **12,600 measured ticks**, **12,896 presents**. The three runs are
consecutive launches of the same binary (present stamps: run 1 ends at
77764.745 s of the traced day and run 2 begins at 77766.037; run 2 ends 77810.853,
run 3 begins 77812.275), so this is a *repeatability* measurement rather than three
configurations.

**Why this is present-pacing and not fallback-pacing.** Two discriminators, both
quantitative rather than narrative:

1. **The count.** Presents ≈ ticks: 4299 / 4318 / 4279 against a measured 4200
   ticks each. (Presents slightly exceed ticks because the present count covers the
   whole run, including the startup frames that precede the first histogram window.)
   Every tick reached the GPU.
2. **The value — the sharper one.** The fallback throttle's deadline is
   `FALLBACK_PERIOD_FRACTION = 0.95` × period
   (`crates/flui-app/src/app/runner/frame_pacing.rs:219`, applied at `:349` as
   `period.mul_f64(...)` anchored to `last_present_at`), i.e. **9.5 ms** here. A loop
   paced by the fallback would show a median at 9.5 ms. Every run's median sits at
   **9.96–9.98 ms**: *above* the fallback deadline, and 0.25–0.40 % *below* the
   panel's own period. That can only be the vsync block.

**Cross-check against the Wayland table (ADR-0029), same instrument.** Wayland's
median is 6.058 ms against its display's 6.065 ms period — ratio **0.9988** — on
Ubuntu / RTX 3070 Ti at 164.89 Hz. macOS native AppKit's **0.99605–0.99750** is in
the same band, so the modal frame on this backend is released by the panel's own
vsync to within one third of a percent, exactly as the `Fifo` contract predicts.
The comparison is same-instrument, *different host*, which is why the ratio to the
period — not the millisecond figure — is the quantity being compared.

**The divergence, stated rather than smoothed over: the tail is heavier here.**
p90 per run (median across that run's windows) is **11.35–11.55 ms ≈ 1.14× period**,
and the per-window worst frame ranges 20.37–32.58 ms (**2.0–3.3×**), against
Wayland's p90 of 6.091–6.149 ms (≈1.005–1.014×) and worst of 7.06–13.49 ms
(1.16–2.22×). The mean rate follows: 95.72–96.93 fps against a 100 fps panel, so
roughly **3–4 % of frames miss their deadline** on this host. ADR-0029 dismissed
its own occasional ~2× outlier as jitter "within the tolerance this ADR expects
from a real desktop compositor"; on macOS that outlier is *routine* — the median
window's worst frame is ~2.2 periods — so the same language would understate it.
**Named as an open lead, not explained away:** this is a desktop under ambient load
(load average ≈3.3–5.0 during these runs: Warp, WindowServer, Ollama), and this
backend has no display-link-driven tick yet — its wake path is the `wake_pump`
main-queue tick (ADR-0058 decision 2) with an 8 ms `FOLLOW_UP`, a coarser actuator
than Wayland's frame callback. Whether the tail is the host, that tick granularity,
or a `Fifo`-on-Metal interaction is **not determined by this evidence**, and is
recorded as undetermined rather than attributed.

**Contamination disclosure.** A first batch of three runs was taken while `just ci`
(full workspace compile plus the whole suite) ran on the same machine, and is
**discarded**: the third of them presented 782 frames in 8.4 s and then stalled for
36 s. Every figure above comes from the later batch, taken with no build or test
active and with the load average recorded per run.

**On-demand, measured with a denominator.** `material_demo` — the full Material app,
a static tree with no animation — was driven on this same native backend for a
**measured 20.047 s** wall-clock window: **exactly 1** `surface frame submitted and
presented`, 1 `First frame rendered`, 283 log lines in total. A presenting app
floods the log (each animated run above emits ~4300 present lines); this one stops
dead after its first frame. That is the on-demand half of the exit criterion — a
static app parks with a live GPU surface and nothing polling.

**And in pixels, not only in counters.** `screencapture -x` of the running native
window shows a real macOS window titled "FLUI App" holding the Material Demo
`AppBar`, "Selected none", a list of `Card` rows `item 0`…`item 6`, and a
`FloatingActionButton` — a full Material app rendered by the native AppKit backend
on the real display. §4's "no reference cross-check is possible" applies to the
*comparison*, not to this *observation*: this is FLUI's own output.

**A false warning found and closed in the same pass.** Both instrument examples
built their controller with `AnimationController::without_ticker`, which logs
`AnimationController has no ticker; the animation will not advance` on `repeat()` —
**false here**, since the ambient `VsyncScope` drives the value through `tick_at`
regardless. Both now use `with_detached_ticker`, the constructor the framework's own
`AnimatedSize` chose for the identical pattern (a ticker whose start/stop
transitions are real but which no `UpdateScheduler` pumps), because `is_animating()`
is deliberately ticker-based (Flutter parity: `Ticker.isActive`). Verified inert
rather than assumed: after the change `grep -c 'has no ticker'` is **0** in all
three runs *while the animation still ticked 12,600 times*. The same pattern exists
at other `without_ticker` call sites in `flui-widgets` (`fade_transition`,
`animated_switcher`, `implicitly_animated`, `dismissible`, `back_gesture`) —
**recorded, not swept**: this plan's scope is the macOS layer, and whether each of
those sites also warns while demonstrably animating is a `flui-widgets` finding.

**Cold start, re-measured on the exact demo §2.6.1 measured the stall on.**
§2.6.1 recorded a static app that armed a fallback deadline nothing ever actuated, ran two
frames — both `presented=false retry_needed=false` — and parked with nothing on screen. Re-run
cold on 2026-09-17 (`sliver_demo`, measured 12.028 s, no poke: that build is deleted), the same
demo now runs **three** frames and the third **presents**:

```text
39.334  wake_pump_armed  delay_us=8000
39.369  wake_resolved action=Render dirty=true        → First frame rendered
39.381  frame_tail presented=false retry_needed=true  → fallback_armed
39.398  wake_resolved action=Render dirty=true
39.398  frame_tail presented=false retry_needed=true  → fallback_armed
39.403  wake_resolved action=Render dirty=true
39.413  present_submitted present_us=67
39.413  frame_tail presented=true  retry_needed=false
39.413  wake_pump_idle  "found no deadline; chain stopped"
```

The two withheld frames carry `retry_needed=true` where §2.6.1's carried `false` — so the
**`PresentDisposition` classification plus the retention rule (§2.6.5) is what turns "give up
after two" into "retry and present on the third", not the wake pump.** Naming which of the two
mechanisms did the work matters, because the pump is the more expensive one and on this run it
did nothing at all.

The pump's own behaviour here is the other half of the evidence, and it is the self-limiting
chain working as designed: one arm at 39.334 with `delay_us=8000` (its first consult found no
deadline — a frame arms its fallback *after* the redraw request that triggered it, which is what
`FOLLOW_UP` exists for), then its tick ran at 39.413 rather than 39.342, 71 ms late because the
main thread was busy running those frames — and by then the app had presented and cleared the
deadline, so the consult answered `None` and the chain **stopped**. One arm, one tick, zero frame
requests, no spinning, on a cold-started app that then parked. That is the "an idle app sends no
redraw requests, so nothing is ever scheduled for it" guarantee observed rather than asserted.

**What this retires:** §5's *"not claimed: that the backend sustains frames from a cold start"*
is now a claim with evidence behind it, on both shapes of app — the animated one (45 s at ~96 fps,
three cold runs) and the static one (present, then park).

**What this closes, and what it does not.** It closes §6 item 4 for macOS: the
native AppKit backend is measured, on the lead platform's own instrument, to be
pacing on the display's vsync, with a parked idle state. It does **not** touch
Windows' native backend — that cannot be built or run on this host under the
standing scope constraint — so the ROADMAP line narrows from "Windows/macOS" to
Windows, rather than being deleted. And it does not claim the tail above is
harmless; see the named open lead.

### 2.9 Where the period actually goes — and the mechanism §2.8 attributed is WRONG (2026-09-17)

§2.8 established the *verdict* on this backend correctly (median 0.996–0.9975 of the panel
period, presents ≈ ticks) and then attributed it to ADR-0029's mechanism: *the GPU's blocking
`Fifo` present is what paces the loop.* Item 3's premise rested on that attribution — "this
backend has no real pacing source, because its wake path is coarser than a compositor frame
callback". So the first thing item 3 owed was a check of the premise, and the check refutes it.

**Method.** Two 45 s runs of `animated_box_app` with *both* ends of the produce path and the
frame/present/wake events traced together
(`RUST_LOG='info,flui.pace=trace,flui.gpu=trace,flui_platform::platforms::macos=trace'`), joined
offline by the microsecond timestamps both ends already carry. No production code was changed to
take this measurement — the two ends of the pump were already instrumented, the earlier runs
simply had not enabled the platform target. Run 1: 4,314 draws, 4,314 frames, 4,297 presents over
45.028 s. Run 2, independent: 4,343 draws, 4,343 frames, 4,340 presents over 45.020 s.

**Finding 1 — the produce path is transparent.** Per 300-tick histogram window, the interval
between `drawRect:` calls and the interval the app measures between ticks agree to **−0.005 ms at
p90** (14 windows, draw p90 median 11.5040 ms vs tick p90 median 11.5047 ms; worst per-window
delta 0.065 ms, most under 0.03 ms). The frame source *is* the tick — `drawRect:` dispatches the
frame request — so this says AppKit's display pass adds no delay of its own: a frame is drawn
exactly as late as the loop asked for it and no later.

**Finding 2 — `queue.present()` never blocks.** Measured directly (`present_us`, the duration of
the `queue.present` call at `wgpu/renderer.rs:2103-2105`):

| quantity | run | p50 | p90 | p99 | max | mean |
|---|---|---|---|---|---|---|
| `queue.present()` cost | 1 / 2 | **0.042 / 0.041** | 0.065 / 0.065 | 0.131 / 0.133 | 0.224 / 0.268 | 0.046 / 0.045 |
| frame lifetime (`drawRect:` → frame tail) | 1 / 2 | **0.779 / 0.752** | 1.499 / 1.484 | 9.534 / 9.241 | 20.732 / 20.077 | 1.209 / 1.121 |
| idle gap between frames | 1 / 2 | **9.156 / 9.152** | 10.586 / 10.594 | 11.523 / 11.541 | 19.078 / 19.548 | 9.178 / 9.191 |
| inter-draw interval | 1 / 2 | 9.987 / 9.974 | 11.502 / 11.419 | 20.621 / 20.471 | 31.674 / 30.763 | 10.386 / 10.312 |

The present is **0.42 % of the period**. The frame's own work is **7.8 %** of it. The remaining
**92 %** is the app parked in its run loop waiting for AppKit's next display pass. Run 2 gives
0.41 % / 7.5 % / 92 % — the same split, to a tenth of a percent.

**So the pacing mechanism on macOS/AppKit is AppKit's display-pass scheduling — the window
server's vsync-aligned commit that decides when `drawRect:` fires next — not the blocking `Fifo`
present.** ADR-0029's decision (Fifo default, no fixed sleep) is unaffected and its *verdict*
stands: the cadence is genuinely vsync-locked. What is corrected is the **attributed mechanism**,
which §2.8 stated as "the `Fifo` mechanism is confirmed backend-agnostic in practice, not only by
construction" and which four record sites now repeat. That sentence is false for macOS and must
not be left standing (§6 item 4's edit, redone below). It was a correct observation — median ≈
period, presents ≈ ticks — carrying a mechanism that does not hold; exactly the shape Prime
Directive #1's honest-accounting clause exists to catch.

**Finding 3 — the tail is diagnosed, and it is not the actuator.** §2.8 left the 3–4 % missed
frames undetermined, naming host load, the coarse wake actuator, and a `Fifo`-on-Metal
interaction as candidates. The joined trace settles it:

| frame lifetime | n | next interval p50 | next interval p90 | late (>11.5 ms) |
|---|---|---|---|---|
| 0.0–0.5 ms | 234 | 10.068 ms | 11.449 ms | 9.0 % |
| 0.5–0.8 ms | 2089 | 10.007 ms | 11.334 ms | 8.0 % |
| 0.8–1.2 ms | 888 | 9.905 ms | 11.090 ms | 5.1 % |
| 1.2–2.0 ms | 940 | 9.868 ms | 11.017 ms | 4.7 % |
| 2.0–4.0 ms | 5 | 9.605 ms | 11.230 ms | 0.0 % |
| **4.0 ms+** | **157** | **20.356 ms** | **21.866 ms** | **98.7 %** |

Pearson **r = +0.904** between a frame's own duration and the interval after it. 157 of 4,313
frames (3.64 %) exceed 4 ms, and 98.7 % of those are late — at **p50 20.36 ms, i.e. exactly 2×**.
The stall length is quantised on the period (143 of the 162 frames over 2 ms land in 8–12 ms,
none in 12–18 ms), and it sits **inside the frame, upstream of `present()`**: on stall frames
`drawRect:` → `pre_present_notified` is p50 8.928 ms of the 9.002 ms lifetime, while
`pre_present_notified` → present stays at 0.061 ms. Nothing in the present call, the wake pump
(1 arm, 1 fire, all run), or the request stream changes on those frames.

Run 2 reproduces the diagnosis independently: **r = +0.883**, 126 of 4,342 frames (2.90 %) over
4 ms, **100.0 %** of them late at p50 **20.286 ms**, with the same empty band between the
one-period and two-period clusters. So "~3 % of frames stall ~9 ms and take exactly two periods"
holds across runs, at 3.64 % and 2.90 %.

**What this does to item 3.** A `CVDisplayLink` changes *when the produce call is issued*. The
period is not being lost at the produce call: the produce path is transparent (Finding 1), the
present is 42 µs (Finding 2), and the 3.6 % of frames that miss their deadline do so because
their own work stalls ~9 ms before the present (Finding 3). A display link would therefore not
move the tail — the delay is downstream of the trigger it would replace — and would not improve
the steady-state ratio, which is already 0.9981 of the period. Item 3's premise as written
("no real pacing source") is refuted; its *scope* narrows to the one thing that survives, named
below rather than assumed.

**The request stream, for completeness.** 12,964 `setNeedsDisplay:` calls for 4,314 draws — **3
real produce calls per frame**, 60 % of them within 0.25 ms of the previous one; AppKit coalesces
them (`needs_display=false` on every line), so they cost three `msg_send`s and no extra passes.
They do **not** cause the tail: requests-per-interval is 3.00 on on-time frames and 3.04 on late
ones. All 13,041 took the deferred path and **zero** deferral hops, zero drops and zero
"no live callbacks" lines were logged — the deferral budget never engaged.

**Not determined, and named rather than attributed:** *why* 3.6 % of frames stall ~9 ms. The
quantisation on the period says "waiting on the display", and its position inside the frame before
the present narrows it to the frame pipeline — the swapchain acquire, `queue.submit()` under GPU
backpressure, or CPU work would all land in that window from this trace alone, and the three are
not separated here. That is the open lead item 3 leaves behind — **and §2.10 closes it the same
day, from instrumentation that already existed: it is the acquire.**

**The claim was in more places than four, and the sweep is now complete.** The first correction
pass edited the four *record* sites (this plan, ADR-0029, ROADMAP, `runtime-contract.toml`'s
`fifo-default-present-mode` statement) and recorded that as the whole of it. A repo-wide sweep for
the assertion found **eleven more shipped sites** — doc comments on public API, module docs, a test
comment and a README bullet — all repeating it as fact. All eleven are now scope-qualified rather
than deleted:

| site | was | now |
|---|---|---|
| `flui-engine` `wgpu/renderer.rs`, `select_present_mode` doc | "Fifo … is the steady-state pacing mechanism for the whole frame loop" | scoped to Vulkan/Wayland, with the measured AppKit split named |
| `flui-engine` `wgpu/renderer.rs`, `render_scene` doc | "`present()` ran, which … blocked until the next vsync — the steady-state pacing the frame loop relies on" | whether the call paced the frame depends on the backend; both named |
| `flui-scheduler` `frame_telemetry.rs`, `submit_at` doc | "blocks until the next vsync, so this instant is present-inclusive" | present-inclusive on both; on AppKit the span is nearly all produce |
| `flui-app` `app/ui_realm.rs`, telemetry-sampling comment + 2 test comments | "the production `Fifo` case BLOCKS until the next vsync" | an in-call pacing block, backend-scoped |
| `flui-app` `app/runner/mod.rs`, startup comment | "comes entirely from the GPU-side blocking Fifo present" | the present path, split by backend |
| `flui-app` `app/runner/frame_pacing.rs`, module doc | "Fifo present blocks every PRESENTED frame at display cadence" | "paced at display cadence by the backend's own present path" |
| `flui-app` `README.md`, frame-loop bullet | "physical pacing … comes from the blocking Fifo present" | the platform's present path |
| `flui-platform` `traits/capabilities.rs`, `default_target_fps` doc | "it comes from the GPU-side blocking Fifo present" | the GPU side's present path |
| `docs/runtime-contract.toml`, the #556 deletion comment | "real frame pacing lives in the blocking Fifo present" | "in the GPU's present path" |

**Left standing deliberately, each with its reason:** `flui-platform`'s `platforms/winit/platform.rs`
module doc and ADR-0044's X11 table row — both scoped to the winit backends, which *are* the
Vulkan/Wayland path where the blocking present holds; the two `CHANGELOG.md` entries — historical
records of what shipped at the time, where rewriting them would falsify the release record instead
of correcting a live claim; and `docs/audits/2026-07-23-architecture-audit.md` — a dated audit in
an archival root.

**Corroboration from an unexpected place: ADR-0045 already says *where* the block lands.** The
raster-lane record states "acquisition is where the vsync block lands", and its amendment narrows
ADR-0029's decision point 3 to "the blocking `Fifo` present is the pacer **only while the raster
owner runs inline on the produce thread**". That is a different location from the one ADR-0029
named — the drawable acquire, not the present — and this session's measurement is consistent with
ADR-0045 and not with reading `present()` as the pacer: on AppKit `queue.present()` is 42 µs while
the frames that miss their deadline sit ~9 ms longer upstream of it, inside the window that
contains `acquire_surface_texture()`. Two records written for other reasons agree on the location;
the attribution ADR-0029 carried is the outlier. That also sharpens the open lead immediately
above: the instrument the next slice needs is not more detail around `present()`, but a marker
between `acquire_surface_texture()` and the present.

### 2.10 The ~9 ms stall attributed — and it needed no new instrumentation (2026-09-17)

§2.9 left one thing open, named rather than attributed: *which* of the swapchain acquire,
`queue.submit()` under GPU backpressure, or CPU work in the frame's own phases the ~9 ms wait is. It
also assumed the split would need "one more marker inside the frame pipeline". Both were wrong in
the same direction — the split was already instrumented, twice over:

- `flui.frame`'s `frame_telemetry` carries `segment_us` and `produce_to_present_us`, and the
  `clock_timestamp` argument is passed to `record_frame` **as** `segment_start`
  (`ui_realm.rs:3272-3278`), so `produce_to_present_us − segment_us` *is* `submit_at − segment_end`
  — the duration of the raster submit call. That isolates the frame's CPU phases from everything
  downstream of them without touching a line of code.
- `surface_acquired` already carries `acquire_us`, the self-measured duration of
  `surface.get_current_texture()` (`renderer.rs:703-709`). The comment above it already said what
  that number is for: *"Under `Fifo` with a frame latency of 1 this is where the vsync block lands
  (ADR-0045 decision 3), so its duration is the one number that says whether the display is pacing
  this thread."*

Two 45 s runs with `flui.frame=trace` added to §2.9's log filter; nothing else changed and no
production code was touched. Run A: 4,276 acquires / 4,273 frames. Run B, independent and under
different conditions (2,174 draws — the loop ran at roughly half A's rate): 2,162 acquires / 2,160
frames. All figures µs:

| component | run | p50 | p90 | p99 | max |
|---|---|---|---|---|---|
| `segment_us` — build+layout+paint | A / B | **88 / 83** | 155 / 148 | 192 / 187 | 356 / 249 |
| submit call (derived) | A / B | **602 / 575** | 1189 / 1140 | 9187 / 9320 | 19521 / 19212 |
| — of which `acquire_us` | A / B | **62 / 60** | 126 / 119 | 8685 / 8819 | 18874 / 18516 |
| — of which `acquire → present` (encode + `queue.submit()`) | A / B | **455 / 434** | 852 / 817 | 1051 / 1018 | 1464 / 1863 |

**It is the acquire, and the other two candidates are excluded rather than merely ranked.** Frames
whose `acquire_us` exceeds 4 ms — **148 (3.46 %)** in A, **69 (3.19 %)** in B — have a p50 acquire of
**8,323 / 8,347 µs**, ~0.83 of the 10 ms period, with a max at ~18.5 ms (two periods). On those very
frames: `segment_us` p50 is **75 / 77 µs**, i.e. *normal* — the frame's own CPU work is untouched —
and `acquire → present` p50 is **436 / 451 µs**, also normal, within 20 µs of the population
median. **Not one** of those frames has an encode/submit span above 4 ms. So:

- **CPU work in the frame's own phases — ruled out.** `segment_us` never exceeds 356 µs in either
  run, two orders of magnitude from the stall, on the stalling frames as much as on any other.
- **`queue.submit()` under GPU backpressure — ruled out.** The encode-and-submit span on the
  stalling frames is indistinguishable from the population's.
- **The swapchain acquire — confirmed**, and it is the only component that grows.

**A cross-check against §2.9's independent instrument.** §2.9 measured by timestamp join that on
these frames `drawRect:` → `pre_present_notified` was p50 8.928 ms of a 9.002 ms lifetime. The
components here sum to 8,323 + 436 + 55 (`notify → present`) + 88 (`segment`) = **8,902 µs** —
agreeing to 26 µs with a measurement taken by a different method on different runs. Two instruments,
same answer.

**What this changes, and what it does not.** It does **not** revise §2.9's conclusion: the acquire
is not the steady-state pacer either — 96.6 % of frames acquire in 62 µs, which is why the app sits
idle 92 % of the period and the cadence still comes from AppKit's display pass. What it establishes
is that the acquire is the **penalty** — the one place a frame pays for arriving at the wrong point
in the drawable rotation. The period-quantised stall length and the ~0.83-period p50 are both what
"waited for a buffer to come free" predicts, and `renderer.rs`'s own comment named this location
before it was measured.

**Correction owed to §2.9: it dismissed item 3 more strongly than the evidence supports.** §2.9
argued a `CVDisplayLink` "would not move the tail — the delay is downstream of the trigger it would
replace". That is true of the stall's *location* and not of its *cause*: waiting for a free drawable
is a **phase relationship** between the produce point and the display pass, and phase is precisely
what a display link changes. So the honest state is: §2.9 refuted item 3's stated premise ("no real
pacing source") and its steady-state case, but it did **not** show a display link cannot help this
tail, and this section supplies the mechanism by which it plausibly could. That is a hypothesis
about the fix, not a measured claim about it — and it is the first thing that makes item 3
*measurably* testable rather than a matter of taste.

**The next question, named rather than assumed:** *why* does a frame whose own work is ~0.7 ms land
in that phase 3.4 % of the time? That is not answerable from these traces and is not the same
question as which wait it is. It is the phase question above, and it is what a produce signal
phase-locked to the display (item 3) would be measured against.

### 2.11 The tail's cause is the swapchain pool width — measured, and removable (2026-09-17)

§2.10 attributed the ~9 ms stall to the acquire and left the phase question open. One fact made that
question cheap to answer instead: `desired_maximum_frame_latency` is pinned at **1**
(`renderer.rs:894`), deliberately, with a documented rationale — a latency of 2 "lets the present
queue hold a stale-size frame the compositor then stretches" during a live resize. A latency of 1 is
the tightest possible pool: one frame in flight, so any jitter in the produce point can find no
buffer free.

**Probe, not a change.** The literal was set to 2, measured, and reverted; the code comment at that
literal forbids widening it without its own resize-jitter regression test, and the tree is back at 1.

| run | acquire p50 | acquire p99 | acquire max | acquire > 4 ms | late frames (>11.5 ms) | inter-frame p50 |
|---|---|---|---|---|---|---|
| baseline A (latency 1) | 62 | 8,685 | 18,874 | **148 (3.46 %)** | 12.6 % | 9,988 |
| baseline B (latency 1) | 60 | 8,819 | 18,516 | **69 (3.19 %)** | 14.1 % | 9,996 |
| probe 1 (latency 2) | 49 | **144** | 4,380 | **1 (0.02 %)** | **0.1 %** | 9,994 |
| probe 2 (latency 2) | 51 | **127** | 9,903 | **1 (0.02 %)** | **0.1 %** | 9,991 |

**The cause is the pool width, not the app's phase and not its budget.** One extra buffer removes
147 of 148 waiting frames, at 0.02 % in both probe runs against 3.46 % / 3.19 % in both baselines,
and takes the late-frame rate from 12.6–14.1 % to 0.1 %. The steady-state pacing is untouched
(p50 9,991–9,996 ms, inside §2.8's 9.96–9.98 ms band), so latency 2 does not disturb what §2.9
established is pacing the loop — it only removes the penalty.

**This supersedes §2.10's own correction to §2.9, in the opposite direction.** §2.10 argued that
dismissing a display link for this tail was too strong, on the grounds that "waiting for a free
drawable is a phase relationship and phase is what a display link changes". The probe answers that:
the wait is removable by giving the pool one more buffer, with no produce signal of any kind. A
display link could in principle also avoid it — firing before the pass leaves the buffer free by the
time the frame arrives — but that is now the strictly more expensive of two remedies for a cause
that is a configuration constant. **§2.9's conclusion (a display link does not fix this tail) is
restored; its reasoning ("the delay is downstream of the trigger it would replace") remains wrong,
and this is the correct reason.** The trail is recorded rather than tidied: §2.9 → §2.10 → here,
three passes, the latter two each correcting the one before.

**Recorded rather than adopted, and why.** Widening the literal has a measured benefit (0.02 % vs
3.46 % acquire waits; 0.1 % vs ~13 % late frames) and a documented cost (the resize-jitter case the
comment names), and that comment says re-coupling them "needs its own resize-jitter regression test,
not something to slip in by widening this literal". **That test does not exist. So the remedy is
measured and the trade is not**, and what the next slice owes is the resize-jitter regression test —
not the literal, and not a display link.

### 2.12 The resize-jitter test was built, run, and it does not discriminate (2026-09-17)

§2.11 named the next slice as "the resize-jitter regression test". It was built the same day. It
works, it runs on this Mac, and **it cannot fail** — which is the whole result, and the reason it is
recorded here rather than announced as the owed test.

**What was built.** `examples/resize_jitter_probe.rs` plus `just macos-resize-jitter`, on the same
bundled-`.app` route as the frame-pump and close-path probes. It opens a real visible AppKit window,
builds a real `flui_engine::wgpu::Renderer` against it, drives a scripted burst of **40 real window
resizes** while rendering continuously into the Metal swapchain, and counts
`Renderer::warn_on_size_mismatch` — the acquired-texture/configured-size divergence, which is the
in-process signature of the stretched frame the literal's comment describes. It fails the run on a
non-zero count, and separately on too few frames or too few resizes so a burst that never happened
cannot pass vacuously. No new production instrumentation was needed beyond giving that warning its
own target (`flui.gpu.resize_transient`) so it can be counted by metadata instead of by grepping its
prose.

**The measurement.** Four runs, at both settings, under two probe designs:

| probe design | latency 1 | latency 2 |
|---|---|---|
| surface follows the window in the same frame | 0 stale / 432 frames / 40 resizes | 0 stale / 466 frames / 40 resizes |
| surface held **3 frames behind** the window | 0 stale / 434 frames / 40 resizes | 0 stale / 470 frames / 40 resizes |

The lagged design was added *because* the first passed at both: it puts the surface deliberately
out of step with the window for the whole burst, which is the regime where a stale-size acquire
should appear. It did not appear.

**Why, and it is structural rather than incidental.** `Renderer::render_scene` acquires and presents
inside a single call, and `Renderer::resize` reconfigures *before* that call — so a drawable is never
alive across a `Surface::configure`, and Metal allocates drawables at the layer's *current*
`drawableSize`, so a reconfigure cannot be followed by an older-size drawable. The hazard the
comment names is not reachable through this backend's frame pipeline. The probe's first design was
wrong in exactly the way that flatters a test — it reconfigured and acquired in the same callback,
making the invariant true by construction — and the lagged design rules that out too. **A test that
passes at both settings is not a regression test, so this is not presented as one.**

**Correction owed to §2.11, and to the literal's comment.** §2.11's closing sentence said "what the
next slice owes is the resize-jitter regression test". That is now sharper: the test **exists and is
vacuous**, so what is owed is not a test but a decision, and the decision cannot be closed by
measurement inside the process. The literal's comment in `renderer.rs` has been corrected in place:
the "present queue holds frames rendered for an older size" mechanism is stated as **not reproduced
on this backend**, with the structural reason and a pointer to the probe. What remains — and it is
the honest reason the literal stays at 1 — is the **compositor-side half**: at a latency of 2 the
window server may hold the previously presented frame for one extra display period after a resize,
so the window's content lags its own edge by ~10 ms during a live drag. That is real in principle
and **not observable from inside the process**; it would need pixels off a real display, which is a
different instrument on a different budget. It is a judgement call, and it is not reported as
measured in either direction.

**What the probe is kept for.** Not coverage of the literal — of the weaker invariant it does pin:
that the acquired swapchain texture always matches the configured surface size on this backend under
an aggressive resize burst. That is a real bug class (a driver or backend change that began pooling
drawables across a reconfigure would hand back a stale-size texture and present it stretched), this
is the only executable check of it on this backend, and its own module doc says plainly which of the
two claims it supports and which it does not.

### 2.13 The "different instrument" exists on this platform — and its gate is a user grant (2026-09-17)

§2.12 closes by saying the compositor-side half "would need pixels off a real display, which is a
different instrument on a different budget." One correction to the *availability* of that
instrument, because the phrasing generalises from a different platform's failure: the reason the
repo records OS screenshot tools as unusable is **Wayland** (`AGENTS.md`'s
visual-self-verification note — the wgpu/Vulkan surface never lands in the X11 framebuffer under
GNOME/Mutter, and `wlr-screencopy`/`grim` is unsupported). **macOS is not that case.** The window
server composites every window into a framebuffer that `screencapture` and
`CGWindowListCreateImage` read, so "capture the window's real pixels mid-resize" is a route that
exists here.

**What blocks it is not feasibility but permission, and that is the part worth recording.** Since
macOS 10.15 the window server gates window capture behind the **Screen Recording** TCC grant. A
probe cannot self-grant it and cannot assume it: without the grant the capture returns the desktop
picture in place of window contents (or nothing), which would read as "no lag" for exactly the
wrong reason. So the honest state is that this instrument is *reachable on request*, not
*available by default* — a slice that wants it must state the grant as a precondition, detect its
absence, and report **not driven** rather than pass.

**Nothing is claimed here about whether the lag is real.** The literal stays at 1 on §2.12's
reasoning, which this paragraph does not overturn — it only replaces "different instrument" with
"an instrument this platform has, behind a grant". The decision is still a judgement call until
someone runs it with the grant in place.

## 3. Review (four lenses) and the fixes it produced

Run after the fix landed, at the user's request — "run the review and make sure the change is
architecturally right for further work". Verdicts: unsafe-auditor **NEEDS WORK**, rust-reviewer
**REDO-TO-BAR**, chief-architect **NEEDS WORK**, concurrency-specialist **NEEDS WORK**. No UB, no
soundness defect, no deadlock, no lock-order inversion and no unbounded growth was found by any
lens. What they did find, and what was changed:

- **The deferral's lane precondition was prose only** on a safe `fn`. `set_needs_display` is now
  `unsafe fn` with a bulleted `# Safety` section and a `debug_assert` against the same predicate
  `route_on_owner` routes with, so an assert cannot drift from the decision it checks.
- **The deferred body was the one AppKit-messaging body no routing witness could see** (`all_on_lane()`
  quantifies over records, not call sites), which falsified the sweep's completeness claim. The
  deferred body is now wrapped in `route_on_owner` too — one routed throat, no second door.
- **No test pinned the deferral decision.** Added two always-run, AppKit-free tests on
  `dispatch_redraw_request` (below), which the free-standing shape exists to make possible.
- **The deferred turn re-checked nothing** (concurrency finding 1) — fixed by the bounded
  re-defer in §2.4.
- **A wrong doc claim**: the deferral was said to deadlock if routed synchronously. It would not
  hang on a main-lane owner — it would take the inline main-thread shortcut and issue the
  discarded call, i.e. reproduce the bug. Corrected (the self-dispatch hang applies only to a
  serial test lane).
- **The `FLUI_MACOS_REDRAW_POKE_MS` diagnostic hook was deleted** (rust-reviewer + chief-architect):
  it contradicts ADR-0058 ("the platform paces production; a sleep never does") and pre-empts the
  `PlatformProxy` redraw verb ADR-0045 decision 5 scopes to #559 by shipping a second, private
  frame source. Its instrument role lives in the probe now.
- **`#551` was cited zero times in any ADR** (chief-architect); the citation is `#559`.
- **Thread-grain over-deferral, the pointer-as-id ABA hazard, and map-membership limits** are now
  documented at the code that has them, rather than left to be rediscovered.

## 4. Verification: what is proven by what

Three tiers, because no single one reaches the claim.

| tier | artefact | what it pins | runs |
|---|---|---|---|
| marker | `display_pass.rs`, 4 tests | set inside a pass, restored on drop, nesting-safe, thread-grained | always (CI) |
| decision | `window.rs` tests `a_redraw_request_inside_a_display_pass_defers_and_never_sends_inline` + its complement | which arm a display-pass caller takes | always (CI) |
| behaviour | `examples/frame_pump_probe.rs`, `just macos-frame-pump` | frames keep arriving on a real window after the primer stops | real Mac only |

**Mutation-checked, not asserted:** the decision tier's claim "this fails without the branch" was
measured — with `dispatch_redraw_request`'s body replaced by an unconditional `send_inline()`,
`a_redraw_request_inside_a_display_pass_defers_and_never_sends_inline` fails and the other five
tests in scope (the marker's four plus the complement) pass, so the guard fires on exactly the
branch it pins and nothing else. Restored afterwards; the suite returns to 171 passed / 3 ignored.

The behavioural tier is the definition-of-done pin the earlier plan said was "not written yet". It drives the production launch path (`MacOSPlatform::new` → `Platform::run` → a visible window →
the real AppKit run loop), installs a frame callback that re-arms the way the engine's frame does,
and starts the pump with a primer that **stops at the first frame**, so the measurement window
cannot be explained by it. Both directions were run on 2026-09-17 (logs local, see §9; these are
their marker lines, verbatim):

```
with the deferral:    primer_frames=49  produced=301  seconds=3.010  fps=100.0  → PASS
deferral branch off:  primer_frames=1   produced=0    seconds=3.010  fps=0.0    → FAIL
```

301 frames in 3.010 s on a 100 Hz panel against **1 frame total** for the same probe with
`dispatch_redraw_request`'s deferral branch removed. That is the counterfactual the fix is about,
measured, not asserted.

Also landed with this slice:

- `crates/flui-platform/ARCHITECTURE.md` — a `## Mapping decisions` entry (the record Prime
  Directive #1 owes for a divergence), carrying the probe table, the alternatives and why each was
  rejected or deferred, and the three-tier coverage.
- `docs/runtime-contract.toml` — a CORRECTION on `raster-wake-relay-precedes-thread-spawn`: the
  entry described one `request_redraw` path where there are now two, and both are routed (the
  deferral changes *when* the call lands, never *which* thread issues it).

Gate results on this Mac: `cargo test -p flui-platform --lib` **171 passed, 3 ignored**;
`cargo clippy -p flui-platform --all-targets -- -D warnings` **0 errors**; `cargo fmt --check`
clean; `just port-check`, `just inventory-check` and `just runtime-conformance-check` (57
contracts) all clean. `just macos-frame-pump` exit 0 with the PASS marker.

**Reference availability, stated rather than assumed:** `.flutter/` and `.gpui/` are **both absent
on this machine** (`ls` confirms: no such file or directory). No Flutter cross-check was performed
or is claimed anywhere in this slice — the AppKit behaviour this rests on is measured by this
slice's own probes, not read from the reference. The gpui-ce/Zed finding in §2.6 came from
separate shallow clones under `/tmp`, which are not the workspace's `.gpui/` reference and will not
survive a reboot; the commit SHAs are recorded so it can be re-derived.

**Known CI gap, not a green claim:** `display_pass.rs`'s and the two decision tests run **nowhere
in CI**. CI has no macOS test job — `cross-typecheck` is `cargo clippy --target aarch64-apple-darwin`
on ubuntu-latest, lint only, no execution. So the always-run tier above is always-run *locally*;
on CI the macOS module is compiled and never executed. This is the same shape as the #1148 probe
and is named rather than implied.

## 5. Definition of Done accounting

- **Claimed:** the frame source defect is fixed and behaviourally verified (probe, both
  directions); the decision branch is pinned by an always-run test that fails without it; the
  divergence is recorded in `ARCHITECTURE.md` with its replacement coverage; the pacing evidence
  is real, recomputed from stored logs, and recorded here with machine, display, period, sample
  count and the honest (looser) ratio.
- **Claimed, and this retires the two open items that stood here:** the backend **does** sustain
  frames from a cold start. Three cold `animated_box_app` launches held ~96 fps for 45 s each, and
  the static cold start §2.6.1 measured stalling now presents and then parks (§2.8) — **with the
  attribution stated there, because it is not the mechanism the plan expected:** the fix is the
  `PresentDisposition` classification plus the retention rule (§2.6.5), *not* the wake pump, which
  armed once and correctly stopped without requesting a frame. The diagnostic poke those earlier
  runs depended on is deleted; nothing above rests on it.
- **Recorded, not merely derived:** `docs/ROADMAP.md` — `:261`'s App.1 caveat, `:303`'s Remains
  bullet, and the Exit criterion — plus `runtime-contract.toml`'s `fifo-default-present-mode` and
  ADR-0029's new "macOS native (AppKit)" subsection. ADR-0029's deferral line is **narrowed to
  Windows rather than deleted**.
- **Corrected, because the accounting above was right about the verdict and wrong about the
  mechanism.** §2.9 measured the present at 42 µs and the frame at 0.78 ms, and showed macOS is
  paced by AppKit's display-pass cadence, not by the `Fifo` present block. The four sites that
  repeated "the `Fifo` mechanism is confirmed backend-agnostic" no longer do; the *numbers* they
  carry are unchanged, because the cadence really is 0.9981 of the period. This is the §2.8
  evidence being held to its own standard rather than a new claim — an observation that was
  correct, with a mechanism attached that was not.
- **Not claimed:** Windows' native backend — unmeasured, and not measurable on this host (§7).
  The ROADMAP line says so explicitly rather than implying the platform set is closed.
- **Named, not explained away — and now diagnosed rather than left open:** the ~3–4 % of frames
  that miss their deadline on this backend (§2.8's tail). §2.8 recorded the cause as undetermined;
  §2.9 determined it. Those frames stall ~9 ms inside their own pipeline before the present
  (r = +0.90 between a frame's duration and the interval after it; 98.7 % / 100 % of the over-4 ms
  frames late, at exactly 2× period). What remains undetermined is narrower and named in §6 item 6:
  *which* wait inside the frame it is — acquire, submit, or CPU — which the current trace cannot
  separate.

## 6. Open items, in the chief-architect's order for the next slice

1. ~~`refresh_period()` on `MacOSPlatform` (ADR-0058 decision-2 conformance gap; today macOS reports
   the 16.667 ms default on a 100 Hz panel) + the `backingScaleFactor` read-order bug (§2.7).~~
   — **done 2026-09-17.** `refresh_period()` implemented and verified on the real display
   (`period_us=10000 hz=100.0`); the read-order item was retracted as disproved by measurement.
   See §2.7. The `set_wake_deadline_hook` half of the ADR-0058 gap is **not** done and stays in
   item 2.
2. **The cold-start stall, now split into its two real halves.** §2.6's mechanism question is
   settled in shape (§2.6, §2.6.1) and the fix is **two changes, ordered, not one**:
   **(a) the damage loss, first, and engine-side — landed 2026-09-17, as the protocol seam §2.6.4
   located rather than the local patch written below. The original mechanism sentence in this
   paragraph was refuted by §2.6.1's own measurement; both the refutation and the landing are
   recorded, and the scouting below is kept because the seam it describes is what shipped.**
   **What landed:** `render_scene` answers with `PresentDisposition` — `Presented` / `NoDamage` /
   `NotShown`, `flui-engine/src/raster.rs:53`, deliberately not `#[non_exhaustive]`, carrying
   `was_shown()` / `is_withheld()` — instead of `Result<bool, EngineError>`; the same type replaces
   `RasterCompletion.presented` on the way down to `SubmitVerdict::NotShown`
   (`flui-app/src/app/raster_lane.rs:94`); the realm's arm (`ui_realm.rs:2959`) commits the painted
   frame and arms a bounded retry (`retry_needed` **and** `retry_needs_repaint`, capped by
   `MAX_NOT_SHOWN_RETRIES`, `:103`), while `NoDamage` still clears. Four tests in
   `ui_realm/frame_commit_state_tests.rs`: `a_frame_the_surface_never_showed_is_retained_and_repainted`,
   `a_frame_with_nothing_owed_is_not_retained`, `the_withheld_retry_is_bounded_and_then_parks`,
   `a_presented_frame_clears_the_withheld_streak`. Contract
   `frame-disposition-distinguishes-withheld-from-idle` (`runtime-contract.toml:918`, state
   `partial`); `PresentDisposition` entered the root-export manifest in the same change; ADR-0068
   records it.
   **What was refuted, in this paragraph's own words, so the correction travels with it.** It said
   the rule was "one line in the same `NoPresent` arm: **when the cause was surface-unavailability,
   do not consume the damage** … No retry, no polling, no new pacing", and reasoned from "a frame
   that never reached the screen consumes the damage the next wake would have re-rendered". §2.6.1
   measured that there is no following wake to be starved: `wake_resolved` fires exactly twice, both
   `Render`, `retry_needed=false` on both frames, and the loop then parks with nothing scheduled —
   so the damage-loss sentence is wrong in the form it was written, and `retry_needed=false` is the
   *correct* answer for the `NoPresent` arm (arming one unconditionally would repaint a genuinely
   occluded window at ~62 Hz, the mistake rejected once already). What survived is the narrower
   question §2.6.1 states — *may a frame that was never shown clear `needs_redraw`?* — which §2.6.4
   then located as a discarded distinction at the trait boundary, and §2.6.5 landed. **The shape
   prediction was wrong in one further way the code now states:** marking the damage without the
   explicit repaint does not work, and `ui_realm.rs:2930-2935` says why — `wake_frame()` re-opens
   the segment gate but not `PipelineOwner`'s own dirty tracking, so a retry pump would find
   nothing to do, produce `Idle`, and clear the flag having never reached `render_scene`; the
   retention is only real because `retry_needs_repaint` gives the retry work. The cap is
   load-bearing for the same reason the paragraph's occlusion premise was too narrow: AppKit's
   `occlusionState` (reaching the frame loop as `windowDidChangeOcclusionState:` →
   `WindowVisibility` → `AppLifecycleState::Hidden` → `frames_enabled == false`) is **not** what
   withdraws the drawable, and the cold-start trace has it reporting the window visible throughout
   12 consecutive withheld frames — wherever the two disagree and stay disagreeing (a window on an
   inactive Space, a display asleep) an uncapped retry is an unbounded loop. A display link still
   leaves all of this exactly as it is, which is why (b) cannot cover it. The precondition is
   engine-side and the two causes
   are **already separated one line apart** — `render_scene` returns `Ok(false)` at
   `flui-engine/src/wgpu/renderer.rs:1907-1911` (`!has_damage && !needs_full_repaint`, i.e. the
   damage was legitimately consumed) and again at `:1915-1917` (`acquire_surface_texture` returned
   `None`, i.e. `Occluded`/`Released` — the damage was **lost**), and the trace at each already
   names which. Only the return type erases the difference, so the fix is to widen it rather than
   to invent a concept.
   **What the realm should then do with "surface unavailable" is *not* what it does with
   `SurfaceStale`.** The `retry_needed` route arms a retry on the next wake, and the steady state
   documented at `ui_realm.rs:3019-3029` — a full-tree repaint per wake, throttled only by
   `NO_PRESENT_FALLBACK_PACE` (~62 Hz), forever — is tolerable for a genuinely broken surface
   (a working retry beats silently giving up) and **not** tolerable for an occluded or minimized
   window, which is a normal steady state, not a fault. The cheaper and correct rule is one line
   in the same `NoPresent` arm: **when the cause was surface-unavailability, do not consume the
   damage** (do not reach `mark_rendered()` for it). Nothing else is needed for the cold start —
   display pass #2 *does* arrive (§2.6), and with the damage still marked it resolves
   `dirty == true` → `Render` → present. No retry, no polling, no new pacing, and the cost is
   bounded by the platform's own wake rate, which while occluded is zero. Re-arming on the
   occlusion-cleared edge stays a separate, later question (the same edge (b) wants as its
   start/stop switch).
   **Where the cause has to be threaded, scouted so the next slice does not rediscover it:** the
   same bool is collapsed three times on the way down — `render_scene` (`Result<bool, EngineError>`,
   `flui-engine/src/wgpu/renderer.rs:1907`/`:1915`), `RasterCompletion.presented: bool`
   (`raster_owner.rs:94-100`, the value the lane reads at `raster_lane.rs:350-356`), and
   `SubmitVerdict::NoPresent`'s own doc ("no damage, or an occluded surface",
   `raster_lane.rs:78-81`). The widening therefore starts at the **trait method**, not at the
   realm: `RasterBackend::render_scene` is an exported trait
   (`runtime-contract.toml:212`) with impls across `WgpuRenderer`, `DebugBackend`, three scripted
   test doubles (`frame_pacing.rs`, `device_recovery.rs` ×2, `raster_lane.rs:518`) and callers in
   `direct.rs`, `hot_reload.rs` and four examples — a real but bounded ripple, and the new outcome
   type must be added to the root-export manifest in the same change.
   `FrameDropReason` (`raster_owner.rs:659-668`) is **not** the carrier: it names mailbox
   supersession and render failure, not "the surface was unavailable".
   **(b) `set_wake_deadline_hook` on macOS with a real pacing source** — **the actuation half
   landed 2026-09-17** (`platforms/macos/wake_pump.rs`: a main-queue tick, armed from
   `request_redraw` and from the hook itself, with a `FOLLOW_UP` second look because a frame arms
   its fallback *after* the redraw request that triggered it, and an only-ever-earlier rule that
   stops a redraw burst from cancelling the ticks it just scheduled — `arm`/`tick_plan`
   unit-tested, and the self-limiting chain observed on a cold start in §2.8). **The "real pacing
   source" half is NOT done**, and it is item 3's subject: the pump *actuates* a deadline, it does
   not *originate* one, so the true vsync-driven tick this paragraph is about is still absent.
   Two concrete leads from the market pass in
   §2.6 worth reading before designing this: **occlusion is what should start and stop the tick**
   (gpui starts/stops its link on `NSWindowOcclusionState::Visible` and re-arms it on screen
   change — FLUI already tracks the same fact as `MacOSWindowState.occlusion_visible`), and
   **activation needs one synchronous frame** (gpui renders exactly one, gated on
   `activated_least_once`). On the API: both shipping Rust macOS stacks are on `CVDisplayLink`
   (hand-declared externs under `#[link(name = "CoreVideo", kind = "framework")]`, as winit does
   in its own `ffi.rs`), and `NSView.displayLink(target:selector:)` is macOS 14+ — **above this
   crate's 11.0 deployment floor** — so a guarded `respondsToSelector:` probe cannot rescue it and
   the deployment-target decision is the CVDisplayLink route, not a guarded CADisplayLink one.
3. **The frame-source decision — and §2.9 has now changed what it is a decision *about*.** The
   original framing ("whether `drawRect:` keeps dispatching frame requests once a display link
   exists") assumed the display link was the fix for the tail. §2.9 removed that: the tail is 3 %
   of frames stalling ~9 ms **inside** the frame pipeline before the present, and §2.11 then
   showed it to be a swapchain pool-width effect whose remedy is a configuration constant — so a
   display link that changes *when the produce call is issued* cannot move it, and the
   steady-state ratio it would compete with is already 0.9981 of the period.
   **What survives, stated as the narrower question it actually is:** a display link is the only
   proposal on the table for a produce signal that this backend *owns* rather than one AppKit
   schedules for it. Whether that is worth hand-declared CoreVideo externs is now open on
   grounds of ownership, not of pacing — and the two things that would decide it are (i) a case
   where AppKit's display-pass cadence is unavailable or wrong (a window whose content is drawn
   but never enters a display pass, or a presentation AppKit does not schedule), and (ii) the
   ~9 ms stall above, **which is a live lead in its own right and is not waiting behind this
   item** — it is the direct cause of the only measured quality gap on this backend.
   **Scouted 2026-09-17 so a future slice does not re-derive the shape** (read, not changed):
   the produce path is *one* AppKit call — `MacOSWindow::set_needs_display` (`window.rs:607`,
   `msg_send![content_view, setNeedsDisplay: YES]`) — reached from `PlatformWindow::request_redraw`
   (`:881`) as `arm_wake_pump()` + `dispatch_redraw_request(deferred, inline)`, where the deferred
   arm exists solely because `display_pass.rs`'s thread-local marker says an in-pass call is
   discarded (`MAX_DEFER_HOPS = 4`, `:813`; `view.rs`'s `draw_rect` is what enters the guard).
   `refresh_period()` (`:851`) already reads the current-mode `CGDisplayModeGetRefreshRate`, so a
   display link and the reported period would share one fact rather than two. **The one thing that
   makes this an FFI slice rather than a swap:** a `CVDisplayLink` callback runs on its *own*
   thread, so it is the second producer in this backend that is not already on the owner lane —
   and ADR-0045 decision 5 records that the wake relay is *not* end-to-end today (the hook calls
   `request_redraw` directly), so the callback must not call that path bare. It has to go through
   the same `route_on_owner` throat every other class-A body uses; the alternative — the
   `PlatformProxy` redraw relay — is already scoped to #559, and choosing between them is the
   first decision such a slice owes. ADR-0044's macOS row (`:90`, "`CADisplayLink` is not wired as
   a distinct signal") is confirmed present and is the amendment target.
   **Amend ADR-0044 in the same slice**: it names `CADisplayLink` as macOS's intended produce
   signal, and §2.6's market pass now shows both shipping Rust macOS stacks on `CVDisplayLink`
   instead. That is a produce-signal decision with a deployment-target consequence, so it belongs
   with the code that makes it, not as a standalone ADR edit now — but it must not be lost, and the
   market figure has one honest edge: the market's API is the one macOS 15 deprecates, and its
   leak-by-design registry exists only because `CVDisplayLinkStop` is asynchronous.
   **Partly done 2026-09-17, and the split is deliberate — read which half before crediting this.**
   The row was carrying two claims and only one of them was a *decision*. (i) **Refuted, corrected
   in place now:** its pacer cell read "Fifo present", which §2.9's measurement refutes for this
   backend (present = 42 µs; AppKit's display-pass cadence is the pacer) — the same correction §6
   item 4 applied at four record sites and eleven further shipped sites; this row was missed by that
   sweep and is now corrected with the measurement named, and the honesty note below the table —
   "not measured on those platforms" — now says macOS *has* been re-measured off-CI while Windows
   has not, so the note stays honest for the backend it still covers. (ii) **Still deferred, on
   purpose:** the `CADisplayLink` → `CVDisplayLink` naming is a produce-signal choice with a
   deployment-target consequence, so per this paragraph it travels with the code that makes it. The
   row now records the *fact* it was getting wrong — the API named is iOS's, macOS spells it
   `NSView.displayLink(target:selector:)` at 14+, above the 11.0 floor, so adoption goes through
   CoreVideo — **without** taking the decision, which stays here in item 3.
   **Decided 2026-09-17: no display link is adopted, and item 3 closes.** Both deciders named above
   came back negative, and neither was closed by argument. (i) The one known instance of "the
   display-pass cadence is wrong" was the cold-start stall, and item 2(a) fixed it with damage
   retention — no produce signal involved; the resize regime, the other candidate, was probed
   (§2.12) and the hazard is unreachable through this backend's frame pipeline at either latency
   setting. (ii) The ~9 ms stall was attributed (§2.10) and then shown to be a pool-width effect
   whose remedy is a configuration constant (§2.11) — `desired_maximum_frame_latency`, which §2.12
   left at 1 as a judgement call about the compositor-side half, not as an open produce-signal
   question. What remains is ownership alone, and the bar for paying it is now explicit: the only
   measured quality gap is removable by a constant, the steady-state cadence already equals the
   display period to 0.2 %, and the two available APIs are `displayLink(target:selector:)` (macOS
   14+, above the 11.0 floor) and `CVDisplayLink` (**deprecated as of macOS 15** — verified in this
   machine's SDK: `CVDisplayLink.h` wraps its whole API in `API_DEPRECATED_BEGIN(…,
   macos(10.4, 15.0))` naming the `displayLink(target:selector:)` family as the replacement). So the
   market's shape is the API Apple is retiring, for a problem that is already solved. Adoption would
   also add the second off-owner-lane producer to this backend, which ADR-0045 decision 5's
   not-yet-end-to-end wake relay cannot absorb unchanged.
   **The decision is recorded where it binds, not only here:** ADR-0044's honesty notes now carry it
   (the macOS row's "remains open on ownership grounds" became "decided: not adopted", with the
   grounds and the reopening trigger), and its macOS row keeps the API facts. **The trigger that
   reopens it:** a case where AppKit's display-pass cadence is unavailable or wrong — content drawn
   but never entering a display pass, or a presentation AppKit does not schedule. The route is then
   re-decided against the deprecation above rather than assumed from the market.
4. ~~Record the pacing evidence at `docs/ROADMAP.md:303` / `:261` and in `runtime-contract.toml`.~~
   — **done 2026-09-17.** The measurement itself is §2.8. It landed in ADR-0029 as a new
   "macOS native (AppKit)" subsection (with the old "native (non-winit) backends — out of scope"
   line **narrowed to Windows rather than deleted**), in the three `docs/ROADMAP.md` sites
   (`:261`'s App.1 caveat, `:303`'s Remains bullet, and the Exit criterion), and in
   `runtime-contract.toml`'s `fifo-default-present-mode` statement — which now names **both**
   measured platforms.
   ⚠️ **Redone the same day, because the mechanism it asserted was wrong.** All four sites were
   written from §2.8's verdict and attributed it to the `Fifo` present block; §2.9 measured the
   present at **42 µs** and shows macOS is paced by AppKit's display-pass cadence instead. The
   verdict stands and the four sites keep the *numbers*, but the sentence "the `Fifo` mechanism is
   confirmed backend-agnostic in practice" was removed from all of them rather than reworded
   around, and ADR-0029's macOS subsection now names the mechanism it actually measured.
5. **Lift the deferral out of AppKit — needs a decision, and partly outside this machine's reach.**
   Issue #1047 (open, P2) is the same contract failing on web with the opposite sign: there
   `request_redraw` dispatches the frame inline and `drain_events` drains synchronously, so a
   re-arming callback recurses inside the originating call (4 frames in one JS→Rust call, no RAF
   turn) where AppKit *drops* the re-arm. The invariant both halves want is one sentence — *a redraw
   request issued from inside a frame request must be deferred to the platform's next frame turn* —
   and it currently lives in `platforms/macos/display_pass.rs` only. Moving it to the
   `WindowCallbacks`/`request_redraw` layer would let both backends hold it, but it edits the web
   backend, which nothing on this host executes, so it is a decision to take deliberately rather
   than a cleanup to slide into this slice. Flagged in the `ARCHITECTURE.md` entry's Trade-off.

   **Decided 2026-09-17: the deferral stays in the AppKit backend, and this item does not close —
   it is handed to the cross-backend track, with the trigger that reopens it named below.**

   The grounds are not "it is inconvenient here". They are that the change is unverifiable on this
   machine, in the exact way this repository's Definition of Done warns about: the edit lands in
   `flui-platform`'s **web** backend, which `AGENTS.md` records as wasm32-only with *no executing
   coverage at all* (`just wasm-test` runs only the crates that declare a `wasm-bindgen-test`
   dev-dependency, and the web backend is not among them; the compile-only `wasm-check` and
   `wasm-link-check` steps run no instruction). Lifting an invariant into a shared layer on the
   strength of one host's behaviour, where the other host cannot be run to check it, is how a green
   gate becomes a claim rather than evidence — and the two backends fail in *opposite* directions
   (AppKit drops the re-arm, web recurses), so getting the shared shape wrong is the likely outcome
   rather than the unlikely one.

   The judgement behind that, stated as a judgement: the hoist wants a "next frame turn" notion the
   `WindowCallbacks` layer does not have — a public-surface shape — and it would be designed from a
   single working implementation whose AppKit half landed the same day. Neither is a reason to never
   do it; both are reasons not to do it blind.

   **What reopens this:** a web-side change to that backend being made for any other reason (then
   the lift rides along, and is worth a real attempt to execute under node); a decision to give the
   web backend executing coverage (tracked in #985), which removes the blocker entirely; or a second
   native backend needing the same invariant, which turns the shared shape from a guess into a
   requirement. Nothing here gates a Mac-only item, which is why it is a hand-off and not a
   blocker.

6. **The ~9 ms frame stall — new, and now the top-quality-item on this backend.** §2.9 measured
   it rather than inferring it: ~3 % of frames (3.64 % and 2.90 % across two runs) spend ~9 ms
   inside the frame pipeline *before* `present()`, and ~99–100 % of those frames miss their
   deadline and take exactly two periods. It accounts for the entire "3–4 % of frames miss their
   deadline" divergence §2.8 recorded as undetermined, and for the p90 ≈ 1.15× period. It is a
   frame-budget problem, not a pacing problem, which is why it is separate from items 2 and 3.
   **~~Not measured~~ — ATTRIBUTED 2026-09-17, §2.10, and it needed no new instrumentation.**
   Measured before: the stall's magnitude (~9 ms), its quantisation on the period (143 of 162
   over-2 ms frames in 8–12 ms, none in 12–18 ms), its position (inside `drawRect:` →
   `pre_present_notified`, p50 8.928 of 9.002 ms), and its even distribution through the run
   (14–24 per 500-frame bucket, not a burst). Now also measured, over two independent runs: **it is
   the swapchain acquire** (`get_current_texture()`): 3.46 % / 3.19 % of frames at p50
   8,323 / 8,347 µs, while on those same frames the frame's CPU phases stay at 75 / 77 µs and the
   encode-and-`queue.submit()` span stays at 436 / 451 µs — so **CPU work and `queue.submit()`
   backpressure are excluded, not merely outranked.** The split needed no new marker after all:
   `frame_telemetry`'s `segment_us` versus `produce_to_present_us` gives the CPU/rest boundary
   (because `clock_timestamp` *is* `segment_start`), and `surface_acquired` already carries
   `acquire_us`. A cross-check against §2.9's timestamp join agrees to 26 µs.
   **The phase question is answered too, and the answer is not "phase": §2.11.** Probing
   `desired_maximum_frame_latency` at 2 (measured, then reverted) removes 147 of the 148 waiting
   frames — 0.02 % against 3.46 % / 3.19 % in both baselines, late frames 12.6–14.1 % → 0.1 %, with
   the steady-state pacing unchanged. So the cause is the **pool width** pinned by that literal, not
   the app's phase relative to the display and not its budget. **What this item owed was a
   resize-jitter regression test — and §2.12 built it, ran it, and found it vacuous.** The probe
   (`examples/resize_jitter_probe.rs`) and `just macos-resize-jitter` exist; a real 40-resize burst
   produces **zero** size-mismatched acquires at latency 1 *and* at latency 2 — four runs, and again
   with the surface deliberately held three frames behind the window — so the test cannot fail and
   is **not** presented as the regression test the comment asked for. The mechanism the literal's
   comment named is not reachable through this backend's frame pipeline (acquire and present share
   one call, and `resize` reconfigures before it), that comment has been corrected in place, and
   what stays open is the **compositor-side** half — the window server holding the previous frame
   for one extra period, ~10 ms of content lag during a live drag — which no in-process instrument
   can observe. So the remedy is measured, the trade is a judgement call, and **the literal stays
   at 1**.
   §2.9's conclusion that a display link does not fix this tail therefore stands, on this better
   reason.
   **Independent of item 3 after all.** §2.10 briefly weakened that, and §2.11 restores it on
   evidence rather than on the original argument: the wait is removable at the swapchain's own
   configuration with no produce signal involved. The two items stay separate.

## 7. Out of scope

- Windows and Linux native backends (cannot build or test them on this machine).
- `UiRealm` per-realm pacing coordination (ADR-0027, explicitly out of ADR-0029's scope).
- The wall-clock Timer service noted in ADR-0029.
- The `MacOSWindow` unsafe `Send`/`Sync` audit ADR-0045 lists ahead of the relay — adjacent, not
  this slice.

## 8. Status log

- 2026-09-17: diagnosis complete via instrumentation + isolated probes. Two structural macOS gaps
  found (no self-sustaining frame source; `refresh_period()` unimplemented) plus a separable
  scale-factor read-order bug.
- 2026-09-17: root cause corrected by measurement. It is not "AppKit withholds visibility"
  (falsified — the visible bit arrives on activation) but "a `setNeedsDisplay:` issued inside the
  display pass is discarded", proven by four probe arms including the deferred variant that works.
- 2026-09-17: the deferral landed; the loop went from 1 frame ever to 657 presented frames.
- 2026-09-17: four-lens review; all blocking findings fixed (§3); the diagnostic poke deleted.
- 2026-09-17: the refuted pacing claim swept repo-wide, not just in the four record sites —
  eleven further shipped sites (public-API docs, module docs, a test comment, a README bullet)
  scope-qualified; three classes left standing with reasons (§2.9).
- 2026-09-17: the ~9 ms stall attributed, with **no new instrumentation** (§2.10) — it is the
  swapchain acquire (3.46 % / 3.19 % of frames at p50 8.3 ms), with CPU work (75/77 µs on those
  frames) and `queue.submit()` backpressure (436/451 µs) excluded rather than outranked. §2.9's
  open lead closed the same day it was written; §2.9's dismissal of a display link for this tail
  withdrawn as stronger than the evidence.
- 2026-09-17: the tail's **cause** attributed too, by one probe (§2.11) — `desired_maximum_frame_latency:
  1` is the tightest swapchain pool; probing it at 2 (measured, then reverted) removes 147 of 148
  waiting frames (0.02 % vs 3.46 %/3.19 %) and late frames fall 12.6–14.1 % → 0.1 %, pacing unchanged.
  So it is a pool-width effect, not phase and not budget — and §2.10's correction to §2.9 is itself
  superseded: §2.9's conclusion about a display link is restored, on this better reason.
- 2026-09-17: **the resize-jitter regression test built, run, and found vacuous (§2.12)** — and
  recorded as vacuous rather than shipped as coverage. `examples/resize_jitter_probe.rs` +
  `just macos-resize-jitter` drive a real 40-resize burst through a real visible AppKit window, with
  the surface deliberately held three frames behind it, and count
  `Renderer::warn_on_size_mismatch`: **zero at latency 1 and zero at latency 2**, four runs. The
  reason is structural — `render_scene` acquires and presents in one call and `resize` reconfigures
  before it, so no drawable is ever alive across a `Surface::configure`. The literal's comment on
  `desired_maximum_frame_latency` is corrected in place to say the mechanism it named is **not
  reproduced on this backend**; what stays open is the compositor-side half (~10 ms of content lag
  during a live drag at latency 2), unobservable from inside the process and recorded as a judgement
  call, not a measurement. **The literal stays at 1.**
- 2026-09-17: pacing re-measured over two long runs (3488 / 3720 presents) — median 10.189 /
  10.233 ms against a 10.000 ms native period (ratios 1.019 / 1.023), superseding the earlier
  small-window 1.0000.
- 2026-09-17: `examples/frame_pump_probe.rs` + `just macos-frame-pump` written and run in both
  directions; `ARCHITECTURE.md` mapping-decisions entry and the `runtime-contract.toml` CORRECTION
  landed.
- 2026-09-17: the decision tier's "fails without the branch" claim mutation-checked (branch
  replaced by an unconditional inline send → that test alone fails).
- 2026-09-17: market pass over gpui-ce + upstream Zed (`CVDisplayLink`, occlusion-switched,
  `displayLayer:` as the second entry, no `setNeedsDisplay`/`drawRect` anywhere) — the earlier
  `CADisplayLink` recollection corrected in §2.6.
- 2026-09-17: PR #1215 merged (`f4f111ea`); post-merge CI on `main` green end to end.
- 2026-09-17: `probes/scale_order.m` run — it **disproved** the recorded `backingScaleFactor`
  read-order claim (screen live at init; scale 1.00 at all three stages), which is retracted in
  §2.7 rather than softened; the staleness reading was refuted by code as well, so no change was
  made. Same probe measured the refresh-rate candidates (CG mode rate 100.000 Hz,
  `maximumFramesPerSecond` 100, both → 10.0000 ms), which chose the source below.
- 2026-09-17: `refresh_period()` implemented for macOS (§2.7) — current-mode rate via
  `CGDisplayModeGetRefreshRate`, cached beside `scale_factor` for owner-lane safety, three
  always-run tests on the extracted arithmetic (guard mutation-checked), a
  `## Mapping decisions` entry, and the probe extended to report the live value
  (`period_us=10000 hz=100.0` on this panel, matching the independent probe).
- 2026-09-17: **§2.6's mechanism re-read from the stored run-12 trace and corrected** — the
  not-displayable frame *does* re-arm (a redraw request 13 µs after it, which is what produces
  display pass #2) and activation is not the gate (pass #2 lands 6 ms *after* the window gained
  focus); the stall is the wake **after** that frame, which has nothing dirty because the
  `NoPresent` frame consumed the damage via `mark_rendered()` while `retry_needed` stays false for
  that arm. So the residual is a damage loss, not a missing re-arm — and the display link does not
  subsume it. §6 item 2 split into the two ordered changes this implies; the measurement that
  converts the `Skip` classification from inference to reading is specified in §2.6.1 and is
  **not yet run**.
- 2026-09-17: **the full gate is green** (`just ci`, `CI_EXIT=0`, cold, `target/` rebuilt from
  scratch after the earlier disk-exhaustion failure): `10385 passed, 4 skipped`, plus the facade
  feature run `40 passed, 0 skipped`. `typos`/`taplo` were skipped — not installed on this machine —
  so the local gate is weaker than CI's exactly there.
- 2026-09-17: **§2.6.1's measurement run, and it refutes the prediction.** Three cold runs of
  `sliver_demo`; instrumentation added permanently in the same pass (`flui.pace`/`wake_resolved` in
  `frame_pacing.rs`, `flui.pace`/`frame_tail` in `ui_realm.rs`). Both wakes are `Render` — there is
  no `Skip` and no third wake; both frames end `presented=false retry_needed=false`; the window is
  visible/key/active with `occlusion=8192`; the app parks in `_nextEventMatchingEventMask:` at 0 %
  CPU with no frame on the stack; and the single `fallback_armed` deadline is never actuated. The
  surface is `Occluded` only as a wgpu drawable transient (the stored run has 1 occlusion then 2593
  presents). Mechanism and consequences recorded in §2.6.1 — **§6 item 2(a) is withdrawn and 2(b)
  is the whole fix.** *(Superseded by the two entries below, and left standing so the reversal is
  visible: what was withdrawn is the damage-loss **mechanism** as §6 item 2 had written it, not the
  question. §2.6.1's own conclusion already says so — "2(a) is NOT withdrawn — but its
  justification inverts" — and §2.6.4 then located the surviving question as a discarded
  distinction at the trait boundary, which §2.6.5 landed as `PresentDisposition`.)*
- 2026-09-17: **§2.6.4's widening landed and its open tuning question is closed — see §2.6.5.**
  `PresentDisposition` replaced `Result<bool>`; the frame tail retains on `NotShown` and clears on
  `NoDamage`; ADR-0068 + the `frame-disposition-distinguishes-withheld-from-idle` registry entry
  record it. The bound is `MAX_NOT_SHOWN_RETRIES = 128` per presentation.
- 2026-09-17: **the "~105 Hz idle loop" the bound guards against was NOT what the traces showed,
  and saying so mattered.** Three runs with unbounded retention gave 240/137/150 presents (baseline
  1/0/0), but sampling them shows frames continue only while pointer events arrive — in the longest
  pointer-free gaps (269–415 ms) there are 1–2 wakes and zero presents, and the demo is a static
  104-line tree with no ticker. An A/B with retention OFF but the classification ON reproduced the
  baseline exactly (0 presents). So retention revives the window; the frame rate is ~89 Hz of real
  pointer traffic. The cap is therefore justified by mechanism and pinned by its own mutants, not
  by a trace that does not exercise it.
- 2026-09-17: **§2.6.4's working assumption that the platform bounds the retry is corrected in
  three places** (source comment, registry statement, §2.6.5): AppKit's occlusion gate keys off
  `occlusionState`, and the measured trace has it reporting the window VISIBLE (`8192`) for the
  whole ~132 ms the drawable was unavailable. Where the two disagree and stay disagreeing, the
  lifecycle gate never engages.
- 2026-09-17: cap calibrated by measurement and confirmed non-binding: with it in place, cold
  starts withdrew for 2, 2 and 3 attempts (`streak` peaked at 3, the give-up arm never fired) while
  the window still presented 275/334/449 frames. Three mutants, each killed by exactly one test.
- 2026-09-17: **§6 item 4's pacing evidence taken and recorded — §2.8 is the measurement.** Three
  cold `animated_box_app` runs on the native AppKit backend (Apple M1/Metal, 100.000 Hz panel):
  42 windows, 12,600 ticks, 12,896 presents, median-of-medians 9.97500 / 9.96046 / 9.96852 ms
  against a 10.000 ms period — ratios **0.99750 / 0.99605 / 0.99685**, the same band as Wayland's
  0.9988 on the same instrument. Two independent discriminators rule out fallback pacing: presents
  ≈ ticks, and the median sits **above** the 9.5 ms fallback deadline (`0.95 × period`) and below
  the panel period, which only the vsync block produces. An earlier batch of three runs is
  **discarded** as CI-contaminated (one presented 782 frames in 8.4 s then stalled 36 s) and that
  is recorded in §2.8 rather than quietly dropped.
- 2026-09-17: **the honest divergence from Wayland is recorded as a finding, not smoothed:**
  p90 ≈ 1.14× period and a routine worst frame of 2.0–3.3× (Wayland: ~1.005–1.014× and
  1.16–2.22×), i.e. ~3–4 % of frames miss their deadline at 95.72–96.93 fps. Cause is
  **undetermined** — host load, this backend's coarser wake actuator, or a `Fifo`-on-Metal
  interaction — and is written as undetermined, not attributed, in §2.8, §5 and the ROADMAP.
- 2026-09-17: **the static cold start §2.6.1 measured stalling now presents**, re-run on the same
  demo for a measured 12.028 s: three frames, the third presenting, with `retry_needed=true` on
  the two withheld ones where §2.6.1 recorded `false`. **Attribution stated because it is not what
  the plan expected: the fix is the `PresentDisposition` classification + retention rule, not the
  wake pump** — which on that run armed once, ticked once (71 ms late, behind the frames the main
  thread was running), found the deadline already cleared and stopped without requesting a frame.
  That is its self-limiting chain observed rather than asserted.
- 2026-09-17: **on-demand with a denominator:** the static full-Material app on this backend ran a
  measured **20.047 s** and produced **exactly 1 present** (283 log lines; an animated run emits
  ~4300 present lines), and a `screencapture` of the live native window shows the Material
  `AppBar`, `Card` rows and `FloatingActionButton` rendered on the real display.
- 2026-09-17: **a false warning closed in both instruments.** `animated_box_app` and
  `vertical_slice_demo`'s histogram built their controllers with `without_ticker`, which logs
  "the animation will not advance" on `repeat()` — false, since the ambient `VsyncScope` drives
  them. Both moved to `with_detached_ticker`, the constructor `AnimatedSize` chose for the same
  pattern. Verified inert: `has no ticker` occurrences drop to **0** in all three runs *while the
  animation still ticks 12,600 times*. The remaining `without_ticker` sites in `flui-widgets` are
  **recorded, not swept** — a `flui-widgets` question, not a macOS one.
- 2026-09-17: **§6 item 4 recorded at all four sites** — ADR-0029 (new "macOS native (AppKit)"
  subsection; the "native backends out of scope" line narrowed to Windows rather than deleted),
  `docs/ROADMAP.md` `:261` / `:303` / the Exit criterion, and `runtime-contract.toml`'s
  `fifo-default-present-mode` statement, which now names both measured platforms.
- 2026-09-17: **§2.9 — item 3's premise checked before it was built on, and refuted.** Two 45 s
  runs with the produce path joined end to end (`flui.gpu` + `flui.pace` +
  `flui_platform::platforms::macos`, no production code changed — both ends were already
  instrumented, the earlier runs had just not enabled the platform target). `queue.present()` is
  **42 µs p50** (0.42 % of the period), the frame is **0.78 ms** (7.8 %), and the app is idle
  **9.16 ms** (92 %) waiting for AppKit's next display pass. So **macOS is paced by AppKit's
  display-pass cadence, not by the blocking `Fifo` present** — the *verdict* §2.8 recorded stands,
  the *mechanism* it attributed does not, and the sentence "the `Fifo` mechanism is confirmed
  backend-agnostic in practice" was removed from all four record sites rather than reworded
  around. Second run: 0.41 % / 7.5 % / 92 %, and the produce-point p90 equals the tick p90 to
  −0.005 ms across 14 windows, so AppKit's display pass adds no delay of its own.
- 2026-09-17: **§2.9 — the 3–4 % missed-deadline tail is diagnosed, and it is not the actuator.**
  Pearson **r = +0.904 / +0.883** between a frame's own duration and the interval after it; frames
  over 4 ms (3.64 % / 2.90 % of them) are late **98.7 % / 100 %** of the time at p50 **20.36 /
  20.29 ms — exactly 2×**. The stall is quantised on the period (143 of 162 in 8–12 ms, none in
  12–18 ms) and sits inside the frame **before** the present (`drawRect:` → `pre_present_notified`
  p50 8.928 of a 9.002 ms lifetime, while present→present stays 0.061 ms). Host load, the coarse
  wake actuator and the `Fifo`-on-Metal guess are all ruled out; *which* in-frame wait it is stays
  named-but-unmeasured (§6 item 6). Item 3 loses its premise to this and item 6 is created by it.
- 2026-09-17: **the request stream measured, and it is not a cause.** 12,964 `setNeedsDisplay:`
  calls for 4,314 draws = 3 real produce calls per frame, 60 % of them within 0.25 ms of the
  previous — AppKit coalesces them (`needs_display=false` on every line). Requests-per-interval is
  3.00 on on-time frames and 3.04 on late ones, so the redundancy does not produce the tail. Zero
  deferral hops, zero drops: the deferral budget never engaged.
- 2026-09-17: **`just ci` went red on `inventory-check`, and the cause was the checker, not the
  change.** The new `macos-resize-jitter` recipe runs `cargo build -p flui …`, and the check
  reported `build-layered builds non-active crates: flui`. The recipe is not the defect:
  `scripts/check-workspace-inventory.sh` matched the recipe body with `(?ms)` — under DOTALL, `.`
  matches newlines, so `(?:^[ \t].*\n)+` swallowed **every** indented line to the end of the file
  instead of stopping at the first unindented one, making "the `build-layered` body" mean "the rest
  of the justfile". That over-collection had a second, quieter effect: `missing` could never fire,
  so the check could not have detected a crate genuinely dropped from `build-layered`. Fixed by
  dropping DOTALL (`(?m)`), with the reason in a comment. Mutation-checked after the fix rather
  than assumed: `['a','b']` for a healthy two-crate body, `['a']` when `b` is removed (so `missing`
  still fires), `NO BODY MATCH` for an empty body, and a later recipe's `cargo build -p facade` is
  no longer attributed to `build-layered`. **`just ci` re-run green: `CI_EXIT=0`, 10,391 tests
  passed / 4 skipped, plus the facade feature run 40/40.** (`typos`/`taplo` still skipped —
  not installed here — so the local gate remains weaker than CI's exactly there.)
- 2026-09-17: **ADR-0044's macOS row corrected — the measurement half only; the decision half stays
  in §6 item 3, deliberately.** The row's pacer cell read "Fifo present", which §2.9's measurement
  refutes for this backend (present = 42 µs; AppKit's display-pass cadence is the pacer). That is
  the same class of correction §6 item 4 applied at four record sites and eleven further shipped
  sites; this row was missed by that sweep. Corrected in place with the measurement named, in the
  style the table already uses for its Wayland row. The honesty note beneath the table — native
  backends "not measured on those platforms" — was likewise narrowed: macOS *has* been re-measured,
  off-CI, so its pacer cell is a measurement now, while the `on_request_frame` column and the whole
  Windows row stay architectural. The `CADisplayLink` → `CVDisplayLink` amendment is **not** taken
  here: §6 item 3 keeps it with the code that makes the produce-signal decision. The row now states
  the *fact* it was getting wrong (the API named is iOS's; macOS spells it
  `NSView.displayLink(target:selector:)` at 14+, above this crate's 11.0 floor) without taking that
  decision.

- **2026-09-17 — §2.13 added: the "different instrument" §2.12 says the residual needs is one this
  platform has, and the correction is about *availability*, not feasibility.** §2.12's closing
  phrasing generalises from Wayland (where the repo's `AGENTS.md` note about unusable OS screenshot
  tools is correct and stays correct). macOS composites every window into a framebuffer that
  `screencapture` / `CGWindowListCreateImage` read, so the instrument exists — but since 10.15 the
  window server gates it behind the **Screen Recording TCC grant**, which a probe cannot self-grant
  and must not assume: without it the capture returns desktop content in place of window content,
  which would read as "no lag" for exactly the wrong reason. So the state is *reachable on request*,
  not *available by default*; a slice wanting it must declare the grant as a precondition and report
  **not driven** when it is absent. **No claim about the lag itself is made here**, and the literal
  stays at 1 on §2.12's reasoning — this only replaces "different instrument" with "an instrument
  this platform has, behind a grant".

- **2026-09-17 — §6 item 3 DECIDED and closed: no display link is adopted; the decision, its
  grounds and its reopening trigger are recorded where they bind.** Both deciders §6 item 3 named
  came back negative and neither was closed by argument: (i) the one known instance of "AppKit's
  display-pass cadence is wrong" was the cold-start stall, which item 2(a)'s landed widening fixed
  with no produce signal involved, and the resize regime — the other candidate — was probed in
  §2.12 and is unreachable through this backend's frame pipeline at either latency setting; (ii) the
  ~9 ms stall was attributed in §2.10 and shown in §2.11 to be a pool-width effect whose remedy is a
  configuration constant. What remained was ownership alone, and the bar for paying it is now
  explicit and *verified on this machine*: `NSView`/`NSWindow`/`NSScreen.displayLink(target:selector:)`
  is macOS 14+ (above the 11.0 floor, so adoption means raising the floor or a dual path), and
  `CVDisplayLink` — the API both shipping Rust macOS stacks use, and the one that works on the
  floor — is **deprecated as of macOS 15**: `xcrun --show-sdk-path`'s `CoreVideo.framework/Headers/CVDisplayLink.h`
  wraps its entire API in `API_DEPRECATED_BEGIN(…, macos(10.4, 15.0))`, naming the
  `displayLink(target:selector:)` family as the replacement. So the market's shape is the API Apple
  is retiring, for a problem already solved; adoption would also add this backend's second
  off-owner-lane producer, which ADR-0045 decision 5's not-yet-end-to-end wake relay cannot absorb
  unchanged. **Recorded in ADR-0044** (its macOS row's "remains open on ownership grounds" is now
  "decided: not adopted", and a new honesty-note bullet carries the grounds) — and the row's
  `CADisplayLink` → `CVDisplayLink` amendment §6 item 3 held back is discharged with it. **The
  trigger that reopens:** content drawn but never entering a display pass, or a presentation AppKit
  does not schedule; the route is then re-decided against the deprecation rather than assumed.
- **2026-09-17 — §6 item 2(a) re-recorded as landed, and the paragraph's own refuted sentence kept
  beside the correction rather than deleted.** The landed artifact is the §2.6.4 seam /
  §2.6.5 widening (`PresentDisposition`, retention on `NotShown`, `MAX_NOT_SHOWN_RETRIES`), cited in
  item 2 with its tests, its registry key and its ADR; the item text had still read as unstarted
  work, which is the failure mode this log exists to catch. Two things were corrected in place
  rather than quietly: the item's "one line … do not consume the damage … no retry" prediction is
  marked refuted by §2.6.1's measurement (`wake_resolved` fires twice, both `Render`;
  `retry_needed=false` is *correct* for that arm), and the §8 entry that recorded "2(a) is
  withdrawn" now carries a supersession note, because §2.6.1's own text already said the opposite
  ("NOT withdrawn — but its justification inverts") and §2.6.4/§2.6.5 then landed it. The
  withdrawal was of the damage-loss **mechanism**, not of the question.
- **2026-09-17 — §6 item 3 closed: no display link is adopted.** The decision and its grounds (no
  measured gap calls for an owned produce signal; the modern `displayLink(target:selector:)` needs a
  macOS 14 floor this backend does not have; `CVDisplayLink` is deprecated as of macOS 15 per this
  machine's SDK; adoption would add a second off-owner-lane producer ADR-0045 decision 5 cannot
  absorb) are in the item itself, with the trigger that reopens it. ADR-0044's macOS row and honesty
  notes carry the same decision, so the ADR and this plan no longer disagree about whether a display
  link is coming.
- **2026-09-17 — §6 item 5 handed off, not closed, and the reason is verifiability rather than
  effort.** The deferral stays in the AppKit backend: lifting it into
  `WindowCallbacks`/`request_redraw` edits the **web** backend, which nothing on this host compiles
  or executes, while the two backends fail in opposite directions — so the shared shape would be
  designed from one working implementation and checked against none. The item records what reopens
  it (a web-side change made for another reason, executing coverage for that backend per #985, or a
  second native backend needing the same invariant) and that it blocks no Mac-only item.

## 9. Evidence inventory (this directory)

Committed with this slice: `plan.md`, `probes/*.m`.

**Every raw trace and log here is uncommitted, by two existing repo rules rather than by choice:**
`.gitignore:170` is a blanket `*.log`, and no `.gz` file is tracked anywhere in this repository
(`git ls-files '*.gz'` is empty). So the two probe verdicts and the three runs are local working
files. Nothing load-bearing is lost with them — every figure derived from them is transcribed into
§2.5 and §4, recomputed from the logs rather than remembered, and the counterfactual's two marker
lines are in §4 verbatim.

| file | what it is | tracked |
|---|---|---|
| `probes/probe.m`, `probe2.m`, `probe3.m` | the isolated AppKit probes (view/layer/visibility arms; the four re-arm arms of §2.2) | yes |
| `probes/refresh.m` | refresh-rate probe (100.000 Hz on this panel) | yes |
| `probe-logs/frame-pump-with-deferral.log` | `just macos-frame-pump`, deferral present → PASS | no (`*.log`) |
| `probe-logs/frame-pump-counterfactual-no-deferral.log` | same probe, deferral branch removed → FAIL | no (`*.log`) |
| `evidence-run1-frames.log.gz` | run-12 trace: requests / `DRAW` / `ACQ` / present ordering inside the pass | no (`.gz`) |
| `evidence-run2-presents.log.gz`, `evidence-run3-presents.log.gz` | the two long present traces behind §2.5 | no (`.gz`) |

The traces could not be reproduced from this tree even if they were committed: they came from a
build carrying the since-deleted diagnostic poke.

**§2.8's batch is a third kind — regenerable, and living outside this directory.** The three
pacing runs, the timed `material_demo` run and the cold-start re-run are in `/tmp/pace-quiet/`:
`anim{1,2,3}.log` plus `.stripped` copies (ANSI-stripped, and the files the statistics were
actually computed from), `material.log`, `material-timed.log` and `sliver-cold.log`, along with
`material-native.png` (the pixel capture). They are uncommitted for the same two repo rules as the
table above, but unlike the §2.5 traces they need **no deleted diagnostic build** to reproduce:
`RUST_LOG='info,flui.gpu=trace' FLUI_FRAME_HISTOGRAM=1 ./target/debug/examples/animated_box_app`
for 45 s, the same for `material_demo`/`sliver_demo` with `flui.pace=trace` added. Every figure
derived from them is transcribed in §2.8, recomputed from the logs rather than remembered. One
caveat to carry: `/tmp` does not survive a reboot, so re-derive rather than assume they are there.

**§2.9's batch, same directory, same rules.** `pump-trace.log`, `pump-both.log` and
`pump-both2.log` (plus a `.stripped` copy of each; the statistics were computed from the stripped
ones) are the two joined runs. Reproduce with the platform target **added** to the §2.8 command —
that is the only difference, and it is what makes the produce path readable:
`RUST_LOG='info,flui.pace=trace,flui.gpu=trace,flui_platform::platforms::macos=trace'
FLUI_FRAME_HISTOGRAM=1 ./target/debug/examples/animated_box_app` for 45 s. ~18 MB per run. The join
and the statistics are the four throwaway scripts `/tmp/pump_{join,requests,compare,tailshape,
budget,cause}.py` — not load-bearing, but every figure they produced is transcribed in §2.9.

