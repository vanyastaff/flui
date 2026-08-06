# ADR-0044: The three-owner frame split and the driver-loop hybrid

*Logical update scheduling, physical per-presentation pacing, and raster capacity are three separate owners (`UpdateScheduler`, `FrameClock`, and the raster owner). This record's own scope, so far, is the first two owners plus the actuator that connects `FrameClock`'s demand mask to the platform: a compositor-paced `RedrawRequested` is a targeted, pacing-aware produce trigger where the platform genuinely delivers one; the pre-existing wake channel is retained as the logical-pass driver everywhere and as the produce path's fallback wherever compositor pacing is absent. Raster backpressure (PR-E) and hidden-surface/wall-clock gating (PR-D) extend this record; neither has landed as of this draft.*

---

- **Status:** Proposed (draft — extended by issue #556's remaining slices)
- **Date:** 2026-08-06
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-scheduler/src/frame_clock.rs` (`FrameClock`, `DemandMask`, `DemandKind`, `PollDecision`); `crates/flui-app/src/app/ui_realm.rs` (`UiRealm::draw_frame_entered`'s per-presentation segment gate and vsync-continuation loop); `crates/flui-app/src/app/runner.rs` (`bootstrap_desktop`'s `on_request_frame` frame-pump closure); the per-platform actuator table below
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §1 (sanctioned leapfrog zone: runtime/scheduling topology is not Flutter-loyal by the Prime Directive's own carve-out); [ADR-0029](ADR-0029-frame-pacing-swapchain-block-with-fallback-throttle.md) (the blocking-Fifo-present pacing mechanism this record's "real pacer" column still defers to on every platform until PR-E); `docs/runtime-contract.toml`'s `scheduling-splits-by-level` contract (the checked, evidence-linked mechanical counterpart to this record)
- **Issue:** #556 — Realm `UpdateScheduler`, presentation `FrameClock`, raster backpressure

---

## Context

Before this record, one component (`Scheduler`, since renamed `UpdateScheduler`) combined logical phase/callback/task scheduling, a fixed 60fps assumption, and the frame-drive loop's own dirty predicate. Splitting this into owners that "know" different things — logical time, physical per-surface time, and raster capacity — needed each piece decided before wiring it into the runner, per the ownership rule:

| Owner | Owns | Answers | Never knows |
|---|---|---|---|
| `UpdateScheduler` (realm) | logical time: phases, transient/persistent/post-frame callbacks, the task queue, async drive | "what runs, in what order, within the granted slice" | a refresh rate, a display, a surface, a GPU |
| `FrameClock` (one per presentation) | physical time for one surface: demand coalescing, actuator-edge coalescing, compositor pacing feedback, first-frame deferral, visibility gating, a produce-capacity threshold, frame timestamps | "does this surface produce a frame now, and by when" | phases, callbacks, element trees |
| Raster scheduling (engine seam, PR-E) | capacity: latest-frame-wins mailbox, an in-flight counter, GPU backpressure | "will the GPU accept another frame" | demand, ordering, widget state |

Flow rule (normative): demand flows up (clock → realm aggregate wake → platform); frames flow down (logical pass → clock-gated segment → raster submit); backpressure flows up (retire → decrement + owner wake → clock re-fires, PR-E). No cross-level reads.

An earlier design pass split this into two PRs specifically because the driver loop — *who calls `FrameClock::poll`, and what actually drives the pump between polls* — was underspecified. `UpdateScheduler`'s reshape and `FrameClock` itself landed first, deliberately keeping the pump's own driver mechanism untouched and behavior-identical at the single-presentation topology this workspace still ships (proven by an executable equivalence table over the `woken`/`has_pending_work` predicate `FrameClock::poll` replaced). This record is what that first pass explicitly deferred: the actuator.

### What already existed, verified against this tree before deciding anything

- **The pump's own frame-production call site is `bootstrap_desktop`'s `window.on_request_frame` closure** (`crates/flui-app/src/app/runner.rs`), which every desktop backend (winit-backed X11/Wayland, and the native Win32/AppKit backends sharing the same `run_desktop` bootstrap) wires identically. `PlatformWindow::request_redraw()` — called from `UiRealm::wake_frame`'s underlying `FrameWakeHandle` — is what the winit backend's `WinitWindowEvent::RedrawRequested` handler responds to by calling `win.callbacks().dispatch_request_frame()`, which is exactly this closure. There is a **second, separate** `WindowEvent::RedrawRequested` the winit backend also dispatches through its generic `lease_window_event_handler()` path — but `runner.rs`'s `PlatformToUi` enum has no variant for it, so that specific plumbing genuinely drives nothing, matching the literal claim this record's planning stage made. The functional actuator — what a caller means by "the wake channel" — has always been `on_request_frame`/`dispatch_request_frame`, under a name that does not mention `RedrawRequested` anywhere in `runner.rs` outside a comment. **This record does not add a second, parallel produce path off the same native event** — doing so would double-produce every delivered `RedrawRequested`. Instead, the existing `on_request_frame` closure gains the two capabilities below.
- **`request_redraw()` (the native platform call) fires unconditionally today, once per `wake()` invocation**, from several call sites (`WidgetsBinding::set_on_need_frame`, `BuildOwner::set_on_build_scheduled`, and — before this record — `draw_frame_entered`'s vsync-continuation check on every tick a controller was running). On real winit this is harmless (the platform's own `request_redraw()` already coalesces to at most one pending event), but nothing in FLUI's own layer modeled or tested that coalescing discipline, and a deterministic test double has no such coalescing for free.
- **Wayland's frame-callback pacing is not a free-running signal.** `WinitWindowEvent::RedrawRequested` on Wayland fires only in response to a prior `request_redraw()` call, at the compositor's next frame-callback slot, and only while the surface is visible (see ADR-0029's own occlusion-semantics section). X11 delivers the equivalent event immediately, with no compositor pacing at all — "compositor-paced" is not a universal property of `RedrawRequested`, it is a **per-platform fact**, which is why the table below exists as a checked deliverable rather than an assumption baked into one code path.
- **`Vsync::has_running()`, sampled to decide whether to keep the pump alive across an animation's duration, was never wired to mark `FrameClock`'s own `DemandKind::Animation`.** A running controller with no other tree-visible side effect (no listener dirtying a widget) would keep the platform loop alive (`wake_frame()` fired every tick) but the segment gate itself would never see nonzero demand from that fact alone — silently diverging from Flutter's own `Ticker`-driven `scheduleFrame`, where the running ticker alone is sufficient to schedule a frame.

## Decision

### 1. The demand mark: Animation, sampled before the tick

`draw_frame_entered`'s vsync-continuation loop now samples `vsync.has_running()` **before** `tick_all(now)`, not only after. If a controller was running entering this tick, `presentation.clock().mark_demand(DemandKind::Animation)` is marked unconditionally — this is what makes a running controller with zero other dirty state still flush its own segment every tick, and it is what makes the tick that completes a controller still flush that completion's final value and status change, mirroring `.flutter/packages/flutter/lib/src/scheduler/ticker.dart`'s `_tick` (:272-285): `_onTick` runs unconditionally before `shouldScheduleTick` is even consulted. The separate "should the NEXT pump be scheduled" question is unchanged — checked **after** the tick, exactly as before this record — so a completing controller's own settle-and-close-the-gate behavior (a pinned invariant) is untouched.

### 2. The actuator edge: `FrameClock::try_arm_redraw_request`

A new, pure (`Cell`-backed) latch on `FrameClock`: test-and-set semantics, `true` at most once per pending demand mask, `false` on every following call until the next granted `poll` produce clears both the mask and the latch. `draw_frame_entered`'s continuation check consults it before calling `UiRealm::wake_frame()`:

```rust
if vsync.has_running() && presentation.clock().try_arm_redraw_request() {
    self.wake_frame();
}
```

N ticks that land before a produce actually clears the mask — GPU backpressure (PR-E, once wired), a configured throttle window, or several ticks in a row that all skip for the same reason — now collapse into exactly one platform-facing wake instead of one per tick. This is the "no request storm" half of the driver-loop hybrid, and it is testable entirely inside `flui-scheduler` (no realm, no platform, no live window) as well as end to end through `draw_frame_entered` itself using a counting wake handle.

### 3. Pacing feedback: `FrameClock::record_compositor_tick`

A second new method, called unconditionally from `bootstrap_desktop`'s `on_request_frame` closure on every fire, before the existing dirty/`wake_action` gate decides whether anything actually runs this pump — recording pacing feedback is about observing the *platform's own delivery timing*, independent of whether this particular delivery turns out idle. It does two things: records the interval between consecutive ticks (`last_compositor_tick_interval`, diagnostic data — no produce decision reads it in this record), and marks `DemandKind::Host`, matching that variant's own pre-existing doc ("the host platform asked for a frame directly ... with no framework-side dirty state of its own"). This is a safety net, not the primary demand source for most produces: the compositor only ever delivers this tick in response to an earlier `request_redraw()` call the caller already made for its own `Dirty`/`Animation` reason, which is why marking `Host` here rarely changes the produce decision on its own, and why it cannot regress the demand-driven-idle invariant (§15 of the design record this ADR formalizes) — a genuinely idle realm never calls `wake()`, so `request_redraw()` is never invoked, so this tick is never recorded, so `Host` demand is never marked.

### 4. The per-platform table (this record's own checked deliverable)

| Platform | Does `RedrawRequested` carry compositor pacing? | What actually triggers a produce | The real pacer today (pre-PR-E) |
|---|---|---|---|
| Wayland (winit) | Yes — per-surface frame callbacks, delivered only while visible, only in response to a prior `request_redraw()` | `on_request_frame` (fed by the compositor-paced native event) | Compositor cadence + the blocking Fifo present (ADR-0029) |
| X11 (winit) | No — delivered immediately, no compositor pacing (self-wake, degenerate case) | `on_request_frame` (fed immediately) | Blocking Fifo present + the no-present fallback throttle (ADR-0029) |
| Windows native | Not run in CI (`cross-typecheck` type-checks only; see AGENTS.md's CI section) | `on_request_frame` (same `bootstrap_desktop` bootstrap as winit) | Fifo present; DWM's own vsync feed is not wired as a distinct signal — named future work, not part of this record |
| macOS (AppKit) native | Not run in CI (same `cross-typecheck`-only coverage) | `on_request_frame` (same `bootstrap_desktop` bootstrap) | Fifo present; `CADisplayLink` is not wired as a distinct signal — named future work, not part of this record |
| Headless (`flui-testing`) | N/A — no native window at all | A scripted pacing feed calling `FrameClock::poll` (or, for the multi-presentation registry, `HeadlessBinding::pump_presentation`/`pump_all`) directly | The test script itself |
| wasm32/web | RAF-shaped in principle, but not wired this record — see the honesty note below | The existing wake-channel path (unchanged) | Browser `requestAnimationFrame` throttling, informally; not measured or asserted here |
| Android | Suspend is *surface loss*, not occlusion — a different lifecycle transition entirely (`Suspended`, presentation.rs) | The existing wake-channel path (unchanged) | Fifo present |

**Honesty notes, not silently assumed:**
- wasm32/web keeps the wake-channel driver exactly as before this record. `WaitUntil` on web is `setTimeout`-emulated (a 4 ms nested-timer clamp; background tabs throttled to ≥1 s), and the web renderer is `Arc<Mutex<Option<Renderer>>>` (a registry-documented gap, #559's cleanup) — wiring RAF as a genuine compositor-paced actuator is deferred, not attempted here.
- Android's suspend transition tears down the `SurfaceView`'s swapchain outright; there is no occlusion-shaped `RedrawRequested` to make targeted in the first place. Clock gating for that transition is PR-D's job, not this record's.
- Windows/macOS native backends are compiled (typechecked) but never linked or executed by CI — this record's claims about them are architectural (the code path is identical to winit's `bootstrap_desktop`), not measured on those platforms.

### 5. What this is not

- **Not a second produce mechanism.** Every produce still traces back to exactly one call to `FrameClock::poll` per presentation per pump, from `draw_frame_entered`'s existing segment loop. The generic `WindowEvent::RedrawRequested` platform-event path (§ above, "context") remains genuinely unconsumed — this record does not wire it, to avoid a second trigger for the identical native event `on_request_frame` already handles.
- **Not a change to raster backpressure or hidden-surface gating.** `FrameClock::set_max_in_flight`/`record_submit`/`record_retire`/`set_min_produce_interval` and `set_hidden`/`is_hidden` are unit-tested capabilities with no live wiring yet — PR-E and PR-D respectively. This record's own invariant (below) does not depend on either being wired.
- **Not a claim that X11/native/headless/web/Android behavior changed.** The demand-marking and edge-triggering additions (§1, §2) are platform-agnostic — they run identically regardless of which backend eventually calls `on_request_frame` — so a platform whose `RedrawRequested` was never compositor-paced to begin with sees the identical produce cadence it had before this record; only the redundant-wake bookkeeping changes (fewer, not more, native `request_redraw()` calls under sustained demand).

## End-state invariant

Every produce has a named trigger — a compositor-paced tick (recorded via `record_compositor_tick`) or a wake-channel pump (armed via `try_arm_redraw_request`, consumed by `wake_frame()`) — and no `request_redraw()`-shaped actuator exists whose delivered event feeds nothing: the generic `WindowEvent::RedrawRequested` path is the one remaining dangling wire, named and left unconsumed deliberately (§5), not silently forgotten. Platforms whose `RedrawRequested` carries no compositor pacing (X11, and every platform in the table's fallback column) are byte-identical, in produce cadence, to the pre-this-record baseline.

## Consequences

**Positive:**
- The driver loop is now a named, documented mechanism instead of an implicit consequence of how `wake()`/`request_redraw()` happen to interact with winit's own event delivery.
- A running animation with no other tree-visible effect now correctly flushes every tick (a real behavioral fix, Flutter-faithful, verified against `.flutter/`), closing a latent gap the previous slice explicitly deferred rather than silently left in place.
- The actuator-edge and pacing-feedback primitives are pure, `Cell`-backed, and unit-tested entirely inside `flui-scheduler` — no live window, no winit event loop, and no flakiness risk, satisfying this workspace's stated limits on what a live winit smoke test can prove here.

**Negative / deferred:**
- The generic `WindowEvent::RedrawRequested` platform-event path stays unconsumed. A future embedder-facing use for it (an app author wanting to observe raw platform redraw requests) would need its own design, not an accidental side effect of this record.
- `last_compositor_tick_interval`'s cadence data has no consumer yet — §14 (exportable frame telemetry, PR-F) is where a jank-analysis consumer would read it.
- The table's Windows/macOS/Android/web rows are architectural claims, not measured ones, for the reasons stated inline above.

## Follow-on (tracked, not part of this draft)

- **PR-D** (hidden-surface gating + wall-clock wake) extends this record with the visibility axis and the measured-idle/instant-response headline (§15's "D half").
- **PR-E** (raster backpressure) extends this record with the third owner's real wiring — `FrameClock`'s in-flight/throttle knobs gain a live raster-owner counterpart, and the retire→wake edge this record's demand model already accommodates structurally.
- **PR-F** (config/observability) adds this record's "What this buys" section (deterministic per-presentation test clock, exportable per-input-attributed telemetry, demand-driven idle as a headline capability) and flips this ADR's status once every slice above has landed.
