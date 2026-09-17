# Plan — macOS native (AppKit) vsync-pacing evidence, and the frame-source defect blocking it

Status: **fix landed, reviewed, and behaviourally verified on a real Mac (2026-09-17).** One
residual defect named, not fixed (§2.6); two further macOS gaps named, not fixed (§2.7). Apple
layer only (macOS/AppKit), per the standing constraint that this machine cannot build or test the
Windows/Linux backends.

## 1. Task and where it comes from

`docs/ROADMAP.md:303` (the App.1/Cross.P named gap):

> Native (non-winit) backend vsync-pacing evidence (Windows/macOS) — deferred to Cross.P; the
> wgpu-side `Fifo` mechanism is backend-agnostic, but only the Linux/Wayland path has been
> measured so far.

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
perfectly reproducible. The chain: the first frames run while the window is still not displayable
(the app has not activated yet), such a frame is *skipped* (no present, and therefore no re-arm),
and with demand generated only by the frame itself nothing restarts it. One external nudge fixes
it permanently for the life of the process, because from then on every frame is displayable and
every frame re-arms.

So the deferral is necessary and proven, but not sufficient: **a frame that ends without
re-arming is the last frame, and only an external tick can break that.** Two honest limits on that
statement, both open:

1. The stated mechanism (a skipped frame) is narrower than the evidence. A poke that lands while
   frames are disabled (`PumpAsync`/`Skip`) is equally the last poke, and the experiments above do
   not separate the two. Settling it needs one no-poke run instrumented on the frame-skip path.
2. A display link — a tick delivered every refresh regardless of window state — is the fix for
   this half, and is also the "true vsync-driven tick" the backend does not have today. ADR-0044
   names `CADisplayLink` as macOS's intended produce signal; ADR-0045 is **Proposed** and keeps
   macOS on `RasterMode::Inline` until the locked `wgpu-hal`'s Metal path stops messaging
   `NSView`/`NSWindow` from the calling thread, so this half is partly upstream-gated.
   `NSView.displayLink(target:selector:)` (macOS 14+) is the modern, non-deprecated spelling and
   needs a guarded `respondsToSelector:` probe plus an explicit `MACOSX_DEPLOYMENT_TARGET`
   decision.

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

### 2.7 Two further macOS gaps found on the way

1. **`refresh_period()` is not implemented for macOS** — trait default `None`
   (`traits/window.rs:290`); only the winit backend overrides it. The runner therefore logs
   `frame-pacing fallback period period_us=16667 reported=false`, i.e. it assumes 60 Hz on a
   100 Hz panel. The *achieved* cadence is right (§2.5) but the *reported* period is wrong, and
   ADR-0058 decision 2 names `Platform::set_wake_deadline_hook` as the tick's home — implemented
   by winit and android, **not** by macOS (trait default no-op).
2. **`backingScaleFactor` is read before the window is on a screen** (`window.rs:292`, before
   `makeKeyAndOrderFront:` at `:296`) → `scale: 1` on a Retina Mac.

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
- **Not claimed:** that the backend sustains frames from a cold start. It does not — §2.6 stands,
  and the pacing runs above started from a diagnostic poke. "MVP reported as parity" is exactly
  what this section exists to prevent.
- **Not yet recorded at `docs/ROADMAP.md:303` / `:261`.** The edit belongs with the §2.6 decision:
  what the roadmap line should say depends on whether the tick source lands in this slice or is
  deferred to Cross.P, and writing it now would claim a liveness the backend does not have.

## 6. Open items, in the chief-architect's order for the next slice

1. `refresh_period()` on `MacOSPlatform` (ADR-0058 decision-2 conformance gap; today macOS reports
   the 16.667 ms default on a 100 Hz panel) + the `backingScaleFactor` read-order bug (§2.7).
2. Actuate `set_wake_deadline_hook` on macOS with a real pacing source — `CADisplayLink` /
   `NSView.displayLink(target:selector:)` preferred, guarded with `respondsToSelector:` and an
   explicit deployment-target decision. Settle §2.6's mechanism question (nested-loop vs disabled
   frames) with one no-poke instrumented run first. Two concrete leads from the market pass in
   §2.6 worth reading before designing this: **occlusion is what should start and stop the tick**
   (gpui starts/stops its link on `NSWindowOcclusionState::Visible` and re-arms it on screen
   change), and **activation needs one synchronous frame** (gpui renders exactly one, gated on
   `activated_least_once`) — that second one is the shape of the §2.6 stall, arrived at
   independently by the same stack.
3. Decide whether `drawRect:` keeps dispatching frame requests once a display link exists — this
   is the frame-source decision, and the ADR it needs (`drawRect` appears in none of the 61 ADRs).
   **Amend ADR-0044 in the same slice**: it names `CADisplayLink` as macOS's intended produce
   signal, and §2.6's market pass now shows both shipping Rust macOS stacks on `CVDisplayLink`
   instead. That is a produce-signal decision with a deployment-target consequence, so it belongs
   with the code that makes it, not as a standalone ADR edit now — but it must not be lost, and the
   market figure has one honest edge: the market's API is the one macOS 15 deprecates, and its
   leak-by-design registry exists only because `CVDisplayLinkStop` is asynchronous.
4. Record the pacing evidence at `docs/ROADMAP.md:303` / `:261` and in `runtime-contract.toml`.
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

