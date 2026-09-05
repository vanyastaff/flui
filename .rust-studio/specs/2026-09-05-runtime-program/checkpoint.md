# Runtime program — session checkpoint (2026-09-05)

Compact state for the standing mandate "reshape weak foundations, finish the
mandatory subsystems, ship a working product, prove it". Updated at every
unit boundary; read it back against the code before trusting any line.

## Scope decided this session

- **Priority rule:** correctness/safety blockers with a real product symptom
  first, then production wiring of shipped-but-unreached seams (the repo's
  dominant defect class — see `AGENTS.md` §Definition of Done), then the
  Runtime.1 ladder (#559 raster lane → #560 host-driven runtime → #561
  transactional recovery), then measurement.
- The 2026-08-01 runtime study's "critical contradictions" 1–7 are closed on
  `main` (#551–#556 shipped); 8–12 are the live remainder (#557 executors
  partially, #558 lifecycles partially, #559 raster lane inline-adopted but not yet
  threaded (see Next units),
  #561 recovery not started, platform verification for Win32/AppKit still
  type-check-only — #653/#654).

## Unit 1 — #919 programmatic close never exits (MERGED — PR #922, main 59290266)

- **Contract changed:** `PlatformWindow::close` on winit is owner-deferred —
  teardown (`on_close`, map removal, callback clear, `WindowEvent::Closed`,
  exit-policy consult) runs on the owner's next turn, never synchronously in
  the call; a quit never overtakes a posted close. Trait doc states the
  Win32/AppKit thread caveats and the headless double's synchrony honestly.
- **Structure changed:** `WinitWindow` moved from `flui_platform::traits` to
  `flui_platform::platforms::winit`; constructor backend-private (needs the
  owner lane). Sole public-surface delta, confirmed by `cargo public-api` +
  `cargo semver-checks` (breaking, pre-1.0, zero downstream consumers).
- **Why deferred (load-bearing, now written at the teardown):** a `close()`
  from inside a window callback runs under `CallbackLease`, whose `Drop`
  restores the leased callback over a synchronous `clear()`.
- **Evidence:** real-event-loop test red on the pre-fix body
  (`window_id_map.len() = 1, expected 0`), green after; #713 test pins
  `[CloseRequested, Closed]`; two control-lane tests; live-smoke X11 six
  checks green with route markers; pre-fix demo hangs (exit 124) on the
  programmatic route. Wayland programmatic cycles CI-only (no weston here).
- **Gates:** `just ci` green (9569 + 40 + 269), `taplo`, `typos`,
  `runtime-conformance-check`. Two traps hit and recorded in memory: the
  `just ci` wrapper reports exit 0 while the recipe failed (read the log's
  own marker); `clippy --all-features` hid an unused import that the
  default-feature recipe caught.
- **Review:** rust-studio reviewer (REDO-TO-BAR → addressed), harsh-critic
  (NEEDS WORK → 5 blockers addressed), API lead (PASS-WITH-NOTES →
  addressed). Follow-ups filed: #923 (split real-loop tests out of
  `platform.rs`), #924 (headless double parity: no global `Closed`, sync
  `on_close`).

## Unit 2 — PR #920 (#558 close veto) to green (MERGED — main d6be4912)

- wasm32 failure fixed locally on the branch (worktree
  `/tmp/claude-1000/-mnt-data-dev-flui/c85dfaf1-30e5-463b-87f9-2419e33d9d59/scratchpad/wt-558`,
  commit `be85f101`): the `PendingCompletion` static lacked the alias's cfg;
  the consult side of the close-request router is dead on wasm32/Android and
  now carries the crate's standard `expect(dead_code)` ratchet.
- Rebased onto `main` (only conflict: the trait-file doc hunk on the block
  #922 moved — took main's); the five doc sites that described the #919 gap
  as open are rewritten (`3544d8ea`); all five earlier review threads on the
  PR verified addressed on the tree and answered. Local gate (`just ci` +
  `just wasm-check`) green; CI green on the rebased head; squash-merged.

## Serial-lane pacing baseline (captured 2026-09-05, ADR-0029 method)

Method: `FLUI_FRAME_HISTOGRAM=1 FLUI_SELF_CLOSE_AFTER_MS=20000 target/debug/examples/animated_box_app`
(dev profile, as ADR-0029), real Wayland session, NVIDIA RTX 3070 Ti driver
595.84, primary panel 164.89 Hz (a second panel at 143.91 Hz is attached;
the window's placement was not controlled). Logs (ANSI-stripped) beside this
file: `baseline-serial-lane.log` (histogram only) and
`baseline-serial-lane-gpu.log` (plus `flui.gpu` per-present trace).

**It does not reproduce ADR-0029's July picture (median 6.058 ms tracking
the panel, p90 within 0.1 ms).** Three findings, each a hypothesis until a
controlled reproduction exists:

- **(A) The `Fifo` block is not pacing presents.** Inter-present intervals
  after a 2 s startup: n=1117, median 1.69 ms, p90 17.4 ms, p99 52.7 ms,
  max 183 ms; buckets: ~630 presents at 0–2 ms, ~340 at 16–18 ms, only 37
  near the 6 ms panel period. Some whole seconds exceed the panel rate
  (175, 183 presents/s). Config is unchanged since July (`Fifo`,
  `desired_maximum_frame_latency: 1`, `renderer.rs:853`), so the candidates
  are the driver/compositor stack (driver 595 vs July's), the inline
  `RasterLane` pump (#792), or window placement/occlusion. ADR-0029's own
  Plan B (`WaitUntil(next_vsync)` from `PlatformDisplay::refresh_rate`) is
  the designed answer if `Fifo` is confirmed not to block on this stack.
- **(B) Presents stop at ~+11 s while the animation keeps rebuilding.**
  Presents per second: 56, 87, 132, 136, 175, 148, 183, 106, 103, 100, 66,
  6, then 0 for the remaining 9 s; the rebuild histogram keeps logging
  windows to the end. The layout-invariant warning below stops in the same
  second. Hypothesis: once the layout cache turns consistent and
  short-circuits, nothing marks paint for the colour change, so every pump
  ends `presented=false` (16 ms fallback pace) and the box freezes on
  screen. Occlusion by another window is the alternative and was not
  controlled. Needs a headless reproduction first (pump N ticks, assert the
  scene's colour changes every tick), then a live re-run with the window
  forced on top (no always-on-top option exists in `WindowOptions` today).
- **(C) A per-frame layout invariant violation.** `subtree_arena.rs:937`
  warns "clean constraints cache but missing geometry; proceeding with
  layout (invariant violation)" for render nodes 1 and 2 — 1630 times in
  9 s, ~2 per frame — then never again. A defect in its own right whichever
  way (B) resolves.
- **(D) The rebuild histogram measures builds, not presents,** and the box
  rebuilds more than once per pump (`reasons=parent_update|animation_tick`;
  tick medians of ~1.2 ms with p90 = 16 ms are multiple rebuilds per 16 ms
  pump). `parent_update` comes from `layout_builder.rs:891` /
  `id_reconcile.rs` — the parent rebuilding each tick. Build-efficiency
  finding; the present trace (`flui.gpu` `present_submitted`) is the honest
  pacing oracle and should be what the histogram example reports.
- **Instrumentation gap:** the engine emits only `present_submitted`; a
  skipped frame has no `flui.gpu` event naming why (no damage / occluded /
  outdated / timeout). Adding that is the first step of the reproduction.

## Next units (ordered)

0. ~~Pacing/freeze investigation~~ — **DONE, PR #926 (ADR-0058).** Findings
   (A) and (B) were one defect with two faces: the 16 ms `no_present`
   `thread::sleep` was the actual pacer (a 165 Hz panel ran at 60 fps and
   input was blocked 16 ms per cycle), and on Wayland the compositor was
   never asked to pace at all because `pre_present_notify` was never called.
   Fixed by arming the platform signal before every present and replacing
   the sleep with a wake deadline anchored to the last present. X11 median
   16.49 → 6.83 ms; Wayland 1298 presents stopping at +11.1 s → 2852 across
   the full run. Numbers in `adr-0058-measurements.md` beside this file.
   **Correction to this file's own earlier reading:** the premise "the Fifo
   block never engages" was wrong — it was measured with the sleep in place,
   and the sleep drained the two-image swapchain before an acquire could
   block. Still open, untouched: (D), the box rebuilding more than once per
   pump; and (C), the layout `clean constraints cache but missing geometry`
   invariant warning — it stopped appearing once the cadence was fixed,
   which makes it a symptom of the frame rate rather than a cause, but the
   violation itself is unexplained.

   **Re-measured after the pacing fix, before investigating either** — which
   is what kept (C) from becoming a wild goose chase a second time:
   - **(C) is closed as a diagnostic defect, not a rendering one.** The
     warning conflated three states (nothing cached / constraints changed /
     geometry cleared under matching constraints) and asserted the third for
     all of them; an ordinary relayout was being reported as an invariant
     violation. Split into a pure `classify_cache_miss` with an exhaustive
     test; only the real violation warns now. In the same commit as the
     wasm-build fix on PR #926.
   - **(D) is closed by the pacing fix.** The `parent_update` rebuild reason
     is gone entirely, and the build cadence now tracks the panel: tick
     median 6.05 ms against a 6.065 ms period, versus 1.2-3.0 ms (several
     builds per 16 ms frame) before. **Residual, stated not swept:** builds
     still outnumber presents, between 1.23:1 and 1.48:1 over a 12 s run
     (1500 logged builds — the histogram only logs in completed 300-sample
     windows, so the true count is up to 1799 — against 1218 presents). Not
     diagnosed; a build that produces no present is the shape to look at.

1. #559 — the inline lane IS the shipping path (desktop + Android runners
   construct `RasterLane`; verified 2026-09-05, the issue thread's "5 of 14"
   is stale). Open registry entries owned by #559:
   `raster-pacing-baseline-precedes-async-surface-acquire` and
   `raster-wake-relay-precedes-thread-spawn` (both `partial`) — the baseline
   measurement on the serial lane, then the wake relay, then the thread.
2. #561 — transactional frame-failure containment (last-known-good commit
   boundary, per the runtime study §12).
3. Baseline measurements (p50/p95/p99 frame time, input-to-present, idle
   wakeups) on the six workloads named in the mandate, before any
   optimisation claim.

## Visual verification found a defect no green test could

After the engine change, the three demos were rendered offscreen
(`cargo run -p flui --features material,cupertino --example screenshot`) to
confirm nothing about what renders had changed. Nothing had — but the
Cupertino demo showed a plainly wrong space in its navigation title, and
measuring the rendered pixels put a number on it: a 22 px space at font
size 17 (~1.3 em), against 6 px for a Material string through the same
renderer. `CupertinoSystemText` does not resolve on this machine, so every
Cupertino run goes through fallback, and the fallback's metrics are wrong.
Filed as **#927** with the measurements.

Two things worth keeping from how it was found: the layer snapshot holds a
single correct span, so every display-list assertion in the suite passes
while the pixels are wrong — the exact "MVP reported as parity" shape
`AGENTS.md` names; and the defect surfaced only because something actually
looked at the output.

## Working-tree hazard, hit for real on 2026-09-05

**Another session shares this checkout.** At 14:49 the reflog records a
`checkout: moving from fix/animated-frame-pacing-freeze to main` plus a
fast-forward pull that this session did not run (it was mid-capture), landing
PR #925. Everything committed afterwards went onto local `main` instead of the
feature branch, and the branch's own earlier commit was left orphaned. Nothing
was lost — the commit was recovered by pointing the branch at it — but the
sequence is silent: no error, no conflict, and `git branch --show-current`
answers with whatever the other session left behind.

Rules that follow: re-read `git branch --show-current` immediately before every
commit rather than trusting the branch from earlier in the session; give any
concurrent agent its own worktree; and if a commit lands on `main`, recover by
pointing the feature branch at it (`git checkout -B <branch> <sha>`) rather
than by resetting anything.

## Environment limits

- No weston: Wayland smoke cycles verified only in CI.
- Win32/AppKit/Android: clippy-only in CI; no device or VM here.
- `gh pr checks` has no `--json`; monitors parse the text table.

## Commands that mean something

```
just ci                      # read the log's own EXIT= marker, not the wrapper's
FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --all-features --no-fail-fast
just live-smoke              # X11; six checks incl. the programmatic-close launch
just wasm-check              # the only lint over wasm32 paths
```
