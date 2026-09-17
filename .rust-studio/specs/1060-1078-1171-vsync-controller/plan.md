# flui-animation Vsync / AnimationController batch — plan v4

(v2 folded concurrency-specialist, api-design-lead and the glm-5.3 outside lens; v3 folds
harsh-critic and the market survey in
`.rust-studio/research/animation-repeat-zero-duration-registry-market-survey.md` — Web Animations
spec, Android `ValueAnimator`, Compose `VectorizedRepeatableSpec`, GPUI, Bevy read at source; v4 folds
harsh-critic's recheck of v3 — three internal contradictions and two defects in the new decisions.)

Issues: #1060 (Vsync::tick_all quadratic lookup), #1078 (repeat sampling is partition-dependent),
#1171 (zero-duration run completes a frame late). Out of scope: #1110 (geometry tweens; blocked by #1108).

Flutter tracker facts folded in (bodies + comments read, 2026-09-16):
- flutter#67507 (open, docs): `repeat()` at 3.44.0 starts from the CURRENT value clamped into
  `[min,max]` (`_RepeatingSimulation._initialT`, `animation_controller.dart:1016-1019`); the old
  "min + value" formula the report describes is gone. Flutter's own tests pin it
  (`animation_controller_test.dart:873-918`: value 1.0 → `repeat(reverse:true)` → 0.75 at 25 ms).
  FLUI snaps `value = min` at `repeat_with` — an UNRECORDED divergence, and the reason a
  `repeat()`-every-build pattern (flutter#37685) would freeze in FLUI while it progresses in Flutter.
- flutter#158233 (open, proposal): the accepted workaround for "set a value with a direction" is
  `animateTo(x, duration: Duration.zero)` / `animateBack(...)` — it only works because Flutter's
  zero-duration path settles synchronously (`_animateToInternal`, `:674-684`). That is #1171.
- flutter#106277 (open, engine): Android 120 Hz Choreographer delivers frame timestamps that go
  BACKWARDS; Flutter asserted `elapsedInSeconds >= 0` and later clamped in the engine. FLUI's
  production clocks are `Instant`-based (`Ticker::start_time`, `UiRealm::now_secs`) and so monotonic;
  the only exposure is `Vsync::tick_all(now_secs)`'s public f64 — document the non-decreasing
  precondition; no clamp (`tick_at` already floors its run-relative time at 0, and a backwards step
  re-samples the pure time function — it does not "hold").
- flutter#1913 / #1911 (status coalescing / re-entrant `value=` recursion): FLUI already delivers
  intermediate statuses synchronously and dedups via `last_reported_status`; no change, a pin test
  is welcome if missing.

Three PRs, sequential, each its own worktree, each `just ci` + `cargo doc -D warnings` +
5a/5b + one outside lens (ollama-lens) before squash-merge. Review mode: full (public API +
concurrency + perf). Order: V → Z → R. Z no longer touches `repeat_with` (the finite zero-period
synchronous settle lives in R, where the landing helper is), so V, Z, R are independent; R is cut
after Z so its settle path inherits Z's `is_running()` ticker-stop guard.

---------------------------------------------------------------------------------------------------
## Slice V — #1060: indexed registry, cursor walk, no per-frame id snapshot

Files: `crates/flui-animation/src/vsync.rs`, new `crates/flui-animation/benches/vsync_registry.rs`
(+ `[[bench]]` in `Cargo.toml`), `crates/flui-animation/docs/PERFORMANCE.md` (numbers).

Design
- `VsyncInner.controllers: BTreeMap<u64, RegisteredController>` keyed by the registration id.
  Ids are `next_id` post-increments and were never reused before either — no-aliasing is NOT a
  new property, the win is O(log N) register/unregister/lookup replacing the linear `find`/`retain`.
  Key order == registration order, which is what lets a cursor replace the id snapshot.
  `RegisteredController` loses its `id` field.
  `register` → `insert` O(log N); `unregister` → `remove` O(log N); `len`/`is_empty` → map.
- `tick_all`: ONE lock to read `(fence = inner.next_id, children clones, muted)`. Then a cursor
  walk: `loop { let due = { lock; let Some((&id, reg)) = inner.controllers.range_mut(cursor..fence).next()
  else break; cursor = id + 1; …anchor bookkeeping exactly as today…; running ? Some((clone, elapsed)) : None };
  if let Some(..) = due { tick_at } }`. Properties, each pinned by a test:
  * lock never held across `tick_at` (unchanged);
  * a controller REGISTERED during the call has id ≥ fence → not ticked until next frame;
  * a controller UNREGISTERED during the call (by an earlier listener) is gone from the map → skipped;
  * an earlier listener STARTING an already-registered later controller → its generation is read
    at its own turn → anchored and ticked THIS frame (same-frame scroll→fling handoff);
  * unregister + re-register of a later controller from an earlier listener → the new id ≥ fence,
    not ticked this frame, anchor `None` (fresh) — no aliasing.
  * a listener that restarts an EARLIER (already visited) controller: not re-ticked this frame
    (the cursor never moves back) — same as today's snapshot list; doc it, no test needed.

  * NO Vsync-level clamp: `tick_at` already clamps the run-relative time at 0
    (`cycle = (dilated - run_epoch_secs).max(0.0)`), so a `.max(0.0)` here changes no value, and
    "holds the run" would be a false claim (below the anchor a run samples t = 0 = its start value;
    above it, it re-samples). Doc only: `now_secs` is a non-decreasing virtual clock; a backwards
    step re-samples the pure function, never below t = 0 (flutter#106277 is an engine-clock
    problem FLUI's `Instant`-based clocks do not have).
  * `muted` is RE-READ under the per-iteration lock (the cursor walk takes it anyway): a listener
    that mutes the registry stops the remaining entries of THIS frame — Flutter honors a mid-frame
    `Ticker.muted = true` (`unscheduleTick` → `_removedIds`, `scheduler/binding.dart`). `children`
    stay sampled once at entry (a child attached mid-walk is first ticked next call — a ticker
    started mid-frame schedules for the next frame in Flutter too). Both pinned by tests.
  * The `tick_all` doc sentence "a disposed-but-not-unregistered controller is simply not ticked"
    is false today (`dispose` leaves `status` running; `tick_at` has no `disposed` check) — V drops
    the sentence; Z makes it true (see Z) and restores it.
- Per-frame allocation: the id `Vec` is gone; the children `Vec` clone stays (few entries, and the
  recursion into a child must not hold the parent lock).
- Complexity: O(N log N) per pump, O(log N) per stopped controller. NOT an active set — the issue
  defers that; `has_running` stays O(N) (doc says so). Follow-up issue if measurements say the
  remaining O(N) idle tax matters (needs start/stop notifications from the controller).
- Remove the `ponytail:` linear-scan note.

Bench (criterion, `harness = false`, construct outside timing, `sample_size(10)` like the issue):
`stopped_vsync_registry` N ∈ {100, 1k, 5k, 10k} (the issue's baseline), `running_vsync_registry`
N ∈ {100, 1k}, `mixed` (10% running), and `unregister_all` N ∈ {1k, 10k} (teardown of a 10k-widget
subtree is N `unregister`s — O(N²) with `retain`, the second quadratic this change removes;
`iter_batched` with a fresh registry per iteration). Record before/after on this host in
`docs/PERFORMANCE.md` as a distribution (criterion low/est/high), no "x-times faster" claim beyond
the measured table.

Acceptance: stated as a SCALING RATIO, not a wall time on one host — 10k/1k estimate ratio ≈ 13
(N log N) rather than ≈ 100 (N²) for both `stopped` and `unregister_all`; all listed reentrancy
tests green; `just ci`.

---------------------------------------------------------------------------------------------------
## Slice R — #1078 (+ flutter#67507 phase parity): repeat value is a pure function of elapsed time

Files: `crates/flui-animation/src/controller.rs`, `crates/flui-animation/docs/ARCHITECTURE.md`
(`## Mapping decisions` new entry), `README.md`/`docs/GUIDE.md` (stale `repeat_with_reverse` → real API).

Model (Flutter's `_RepeatingSimulation`, re-derived, not transcribed):
- State: `repeat_initial_ns: u128` (nanoseconds of phase the current value sits at when the run
  starts: `round((value_clamped - min) / (max - min) * period_ns)`) replaces `repeat_done`, which
  is REMOVED (no incremental cycle bookkeeping survives; the zero-period case never ticks).
- `repeat_with`: calls `clear_run_modes()` first (today it does not, so a leftover
  `animate_to_curved` curve leaks into the repeat — `tick_repeat` applies NO curve; Flutter's
  `repeat` takes none). Then the value at the call is the pure function SAMPLED AT CYCLE 0 (NOT
  `= lo`, and NOT a bare clamp — see the model bullets: from value == max in restart mode that is
  `min`), stored with `ValueChange::Unchanged` — exact Flutter parity: `_startSimulation` sets
  `_value = clamp(simulation.x(0.0))` WITHOUT `notifyListeners()` (`animation_controller.dart:861-871`);
  the existing `ValueChange` doc already records this posture. The same sample yields the initial
  direction/status (`Reverse` iff `reverse && i0 odd`) — ALSO Flutter parity: `_startSimulation`
  calls `x(0.0)` (which runs `directionSetter`) BEFORE it computes `_status`, so bounce-from-max
  reports `reverse` at the call there too.
- New `tick_repeat(inner, cycle)` branch dispatched from `tick_at` when `is_repeating` (the
  time-based branch loses its repeat sub-branch):
  * ONE shared private helper owns the landing rule and is the only place the parity math lives:
    `fn repeat_leg(reverse: bool, index: u64) -> AnimationDirection` (Forward unless `reverse && index odd`)
    and `fn repeat_landing(&self, index) -> (value, direction)` on `AnimationControllerInner`
    = the END of cycle `index` (`Reverse` leg → `repeat_min`, `Forward` leg → `repeat_max`). R's
    exhaustion branch AND Z's synchronous finite-zero-period settle both call it — two hand-copies
    of the modular arithmetic is how one path ships wrong.
  * All repeat arithmetic in INTEGER NANOSECONDS (Compose `VectorizedRepeatableSpec` and GPUI
    both reduce modulo the period in nanos before any float conversion): `period_ns = period.as_nanos()`,
    `initial_ns = ((f64::from(value) - f64::from(min))/(f64::from(max) - f64::from(min)) * period_ns as f64).round() as u128`
    (widen the f32s BEFORE subtracting — near `max` the f32 difference loses bits; ~60 ns quantization
    at a 1 s period, harmless, say so in a comment), `total_ns =
    Duration::try_from_secs_f64(cycle.max(0.0)).map_or(0, |d| d.as_nanos()) + initial_ns`;
    `i = total_ns / period_ns`; `phase = (total_ns % period_ns) as f64 / period_ns as f64`.
    The f64 predicate `total >= c * period` is NOT acceptable: `0.3 >= 3.0 * 0.1` is false, so a
    3-count 100 ms repeat sampled at exactly 300 ms would stay `Forward` one more frame — the
    "frame late" class this batch closes. Boundary test: c = 3, period 100 ms, `tick_at(0.3)` → exhausted.
    The `period_ns == 0` case never reaches the tick path (it settles at the call, below) — the tick
    path still guards `period_ns == 0` with an early return BEFORE `total_ns / period_ns` (u128
    division by zero panics), never an `expect`.
  * `period > 0`: exhausted iff `count.is_some_and(|c| total_ns >= c * period_ns)` → `repeat_landing(c-1)` sets
    `start_value/target_value/direction/value` for the final leg, status = that leg's
    `direction.settled_status()` (the ONE run-end rule Z introduces; R is cut after Z),
    `active_run.take().map(complete)`, ticker stop, `is_repeating = false`,
    `finish(.., Notify, delivery)`. Not exhausted: `direction = repeat_leg(reverse, i)`,
    `start_value/target_value` = that cycle's endpoints (keeps `velocity()` right and signed),
    `value = lerp(start, target, phase)`, `status = direction.running_status()`, `finish(.., Notify, None)`
    — `take_status_change` dedups, so a flip fires exactly one status change.
  * `run_epoch_secs` is never advanced by a repeat any more, which leaves it a constant 0
    (`restart_ticker` is its only other writer, to 0.0) — DELETE the field and the subtraction in
    `cycle_elapsed_secs()`/`tick_at`; the ARCHITECTURE.md state listing drops it too.
  * Value AT THE CALL = the pure function sampled at cycle 0 (`total_ns = initial_ns`), not a bare
    clamp: from value == max in RESTART mode that is `min` (phase wraps), exactly Flutter's
    `_startSimulation` `_value = x(0.0)`; in bounce mode it is `max` on the reverse leg. The same
    sample yields the initial direction/status, so `repeat_leg` is the single source for both.
  * ZERO EFFECTIVE PERIOD, any count → SYNCHRONOUS settle at the call (early return BEFORE
    `restart_ticker`, `run_generation` untouched): `repeat_landing(count-1)` for a finite count,
    `repeat_landing(0)` (= `max`, the end of the only cycle) for an infinite one; then the same
    settle path the zero-distance/zero-duration runs use (directed status once, displaced run
    canceled, `TickerFuture::complete()`). This is Android's rule ("0 duration animator, ignore the
    repeat count and skip to the end", `ValueAnimator.animateBasedOnTime`) and the Web spec's
    arithmetic (zero iteration duration → overall progress = iteration count); Compose throws
    (`InfiniteRepeatableSpec`: "cannot have a 0-duration"), Flutter asserts. Repair-not-reject is
    the house rule and a zero-period infinite repeat that ticked once per frame would hold the
    frame loop open forever doing nothing — so it settles. Documented exception to "an infinite
    repeat's future resolves only by cancellation". No `zero_period_cycles` state; the tick path
    never sees `period_ns == 0` (guard with an early return, not an `expect`).
- `min == max` stays rejected (`InvalidBounds`; Flutter permits the degenerate range, `assert(max >= min)`).
  Rationale to record: a repeat that structurally can never change value is a caller error (it would
  hold a frame loop open forever doing nothing); failing fast beats a silent no-op animation, and it
  is the same contract `with_bounds` already applies to an empty range.
- Period resolved ONCE at `repeat_with`: `repeat_period = Some(period.unwrap_or(inner.duration))`.
  Today `current_duration()` falls through to `reverse_duration` on the reverse leg of a bounce,
  contradicting `repeat_with`'s own doc ("defaults to the forward duration") and Flutter
  (`period ??= duration`, captured by the simulation; a later `duration=` does not reach a running
  repeat). One period for both legs keeps the modulo arithmetic exact; `set_duration` mid-repeat
  no longer changes the running repeat (Flutter parity) — record both in the mapping entry, and PIN
  the second with a test (`set_duration` during an active repeat: the current cycle's period is
  unchanged) — today `current_duration()` reads the live `duration`, so this is a real change.
- `velocity()` on a reverse leg is SIGNED (range/period of the current leg) — a deliberate
  divergence from Flutter's `_RepeatingSimulation.dx = (max-min)/period` (always positive, `:1053`);
  it matches FLUI's non-repeat velocity contract and is ALREADY the behavior on main (not a red
  test — a pin). Record it, don't call it "right".
- Exhaustion STATUS diverges from Flutter in bounce mode too, not only the restart value: Flutter's
  exit `x()` runs `directionSetter` for the NEXT leg and `_tick` derives status from `_direction`
  (`:948-950`), so `repeat(reverse: true, count: 1)` ends `dismissed` at value 1.0 and `count: 4`
  ends `completed` at 0.0. FLUI: `Completed` at `max`, `Dismissed` at `min` (the final leg's own
  direction, settled). Record; the exhaustion tests pin STATUS as well as value.
- Docs: `repeat`/`repeat_with` doc states the phase model + "count is measured from the phase
  origin: a run started mid-cycle ends `count` boundaries later, not `count` full periods (Flutter
  parity, `_exitTimeInSeconds = count*period - _initialT`)". Mapping entry "Repeat sampling":
  (a) partition invariance (FLUI defect fixed), (b) initial phase = Flutter parity + why (no jump,
  `repeat()`-per-build progresses), (c) IMPROVEMENT over Flutter: exhaustion lands on the last
  cycle's endpoint, where Flutter's `% 1.0` wraps a 1-count restart repeat to `min` at `completed`
  (`animation_controller_test.dart:1293-1329` expects 0 at 100 ms) — replaced oracle = FLUI test,
  (d) `min == max` rejected, (e) zero period settles at the call (Android's skip-to-end; Compose rejects, Flutter asserts).

Tests (RED first, each fails on `main`):
- partition invariance: `tick_at(1.25)` == `tick_at(1.0); tick_at(1.25)` for restart and bounce,
  1.25 / 2.25 / 3.25 (odd/even skipped cycles), custom min/max; same timestamp twice is idempotent.
- Flutter oracles ported as citation-doc'd tests: value 1.0 → bounce → 0.75 @25 ms, 0.25 @125 ms;
  value 0.5 → bounce → 1.0 @50 ms, 0.0 @150 ms; `min .5 max 1` cases (assert the AT-CALL value is
  0.5 — the silent clamp, Flutter's `x(0.0)`); `min 1 max 3` on `[-1,3]`.
- status at the call: value == max, bounce → status `Reverse` before any tick (Flutter parity, see design).
- finite count with initial phase: value 0.5, count 1 restart → completes at 0.5 period with
  value max; bounce count 4 from 0 → `1.0 @100 ms`, `0.4 @160 ms` (cycle 1, phase 0.6, reverse
  leg), exhausted at 400 ms on min. NOTE the Flutter oracle (`animation_controller_test.dart:1331-1355`)
  reads `tick(100)` → 1 then `tick(60)` → 0.6: its `tick(d)` is `handleBeginFrame(d)`, an ABSOLUTE
  timestamp (`test/scheduler/scheduler_tester.dart`), so that 0.6 is elapsed 60 ms — a REWIND, not
  160 ms. Port it as `tick_at(0.10)` → 1.0 then `tick_at(0.06)` → 0.6 (pure sampling makes the
  rewind exact) and separately assert `tick_at(0.16)` → 0.4.
- `set_duration` mid-repeat leaves the running repeat's period unchanged (see design).
- a leftover `animate_to_curved` curve does not shape a following `repeat` (red today).
- status/value notification counts across a multi-cycle frame: bounce even skip → 0 status
  changes, odd → 1; restart → 0.
- zero period: finite and infinite both settle synchronously at the call (complete future, settled status, landing value); no tick is needed and a later `tick_at` changes nothing.
- pins (green on main, kept as guards, NOT claimed red): `velocity()` negative on a reverse leg;
  `repeat_with_finite_count_stops`, `repeat_consumes_all_cycles_in_one_long_frame`,
  `finite_repeat_future_completes_when_the_count_is_exhausted`, `infinite_repeat_future_only_resolves_via_stop`.
- red: value at the call from value == max in restart mode is `min` (today it is `min` too by the
  snap — so assert the BOUNCE case instead: value == max, bounce → at-call value `max`, status `Reverse`).
- red: zero-period `repeat_with` (finite AND infinite) returns a complete future and a settled status at the call.
- red: exhaustion c = 3 at exactly 300 ms (f64 boundary).
- `repeat_with_clamps_range_into_bounds` message updated.

---------------------------------------------------------------------------------------------------
## Slice Z — #1171: zero-duration runs settle synchronously at the call

Files: `controller.rs` (`forward_from`, `reverse_from`, `drive_to`, `settle_at_target`, `dispose`,
`tick_at`, new `pub(crate) is_live_and_running`), `vsync.rs` (`has_running` + walk predicate, one doc
sentence), ARCHITECTURE.md (mapping table row + delete the
"Recorded gap" paragraph + new entries), `forward`/`animate_to`/`animate_back` docs, CHANGELOG.

Design
- In `forward_from`/`reverse_from`/`drive_to`: compute the run duration FIRST, then
  `if distance < BOUND_EPSILON || run_duration.is_zero() { return Ok(self.settle_at_target(inner)); }`
  — exactly Flutter's `simulationDuration == Duration.zero` gate covering zero distance, zero base
  duration, and an explicit `Some(Duration::ZERO)`. `settle_at_target` snaps value to target,
  cancels the displaced run, reports the directed settled status once, returns
  `TickerFuture::complete()`; `run_generation` untouched (no `restart_ticker`) — extend its doc's
  list of settle paths.
- `settle_at_target`'s ticker stop is guarded by `can_tick()` (Active only). A MUTED ticker
  (`is_running()` = Active | Muted) keeps its run through a settle and would resume scheduling
  ticks against a settled controller on unmute. Align the guard to `is_running()` — the same
  lesson `restart_ticker`'s doc records. `AnimationController` exposes no mute today, so the
  shape is unreachable through the public API and has no red test; say so at the guard.
- DIRECTION IS CHOSEN BY THE METHOD, not by travel (align to the reference, which is the only
  status-bearing controller on the market — Compose/SwiftUI/Web have no status). Flutter documents
  it: `animateTo` → "status is reported as forward regardless of whether target > value or not …
  completed at the end"; `animateBack` → reverse/dismissed (`animation_controller.dart:566-636`).
  FLUI's `drive_to` today derives direction from `target >= value` — an unrecorded divergence that
  makes `animate_to`/`animate_back` differ only in the default duration and throws away the one
  bit of information the caller has (flutter#158233's whole complaint). Fix: `drive_to`'s
  `direction` = `Forward` for `animate_to`/`animate_to_curved`, `Reverse` for `animate_back`/
  `animate_back_curved` (the existing `prefer_reverse_duration` flag IS the method — rename it to
  the direction). Callers checked: `scroll_controller.rs` reads only `is_running()`; the navigator
  back-gesture and cupertino button call `animate_to_curved` toward 1.0 and `animate_back_curved`
  toward 0.0, where method and travel agree.
- RUN-END STATUS = the run's direction, settled (`AnimationDirection::settled_status`: Forward →
  Completed, Reverse → Dismissed), for every run end: `tick_time_based` non-repeat end,
  `tick_simulation`, `settle_at_target`, repeat exhaustion (final leg). Flutter's `_tick` rule
  (`:948-950`), no bound check — so `animate_to(lower_bound)` from mid-range ends `Completed`.
  `stop()`/`set_value` KEEP the bounds-first `settled_status_directed()` (FLUI's documented
  frame-driver contract; Flutter's `stop()` changes no status at all).
- With that, `animate_to(x, Some(Duration::ZERO))` / `animate_back(x, Some(Duration::ZERO))` are
  the documented "set with a direction" — the workaround flutter#158233's thread settled on, now
  with the same meaning as in the reference.
- `settle_at_target` notifies value listeners only when the value actually moved — measured against
  the value at METHOD ENTRY, before `forward_from(Some(x))`/`reverse_from(Some(x))` apply `from`
  (`controller.rs:523-525, 586-588` assign `inner.value = x` silently; a settle-time comparison
  against `target` would make `forward_from(Some(1.0))` from 0.3 a silent jump). Pass the entry
  value into `settle_at_target`. Flutter: `if (value != target) { …; notifyListeners(); }`
  (`:675-678`), and its `forward(from:)` notifies through the `value=` setter. The zero-distance
  settle stops firing a spurious value notification. Record in the mapping table.
- `dispose()` leaves `status` UNTOUCHED (Flutter parity: `dispose` disposes the ticker and clears
  listeners only, `animation_controller.dart:909-930`; a manufactured settled status would be
  visible through every `Animation` wrapper — `hero_flight.rs:896-898` reads `proxy.status()` on
  replay). The frame-loop leak is closed on the two consumers instead: `tick_at` returns early when
  `disposed`, and `Vsync` reads a crate-private probe under ONE controller lock —
  `pub(crate) fn walk_probe(&self) -> WalkProbe { generation: u64, live_running: bool }` where
  `live_running = !disposed && status.is_running()` — in BOTH `has_running` (`live_running`) and the
  tick walk (replacing today's two locks `run_generation()` + `status()`), so a
  disposed-but-not-unregistered controller neither ticks nor holds the frame loop open. The single
  lock is also the perf audit's follow-up from V: the two controller-lock pairs were ~28 % of the
  ~54 ns per stopped controller per pump and a one-lock prototype measured ~11 ns (~20 %) less —
  re-run `cargo bench -p flui-animation --bench vsync_registry` and append the stopped rows to
  PERFORMANCE.md's Vsync section. Restore the
  `vsync.rs` doc sentence V removed, worded for this shape. Test: `Vsync::has_running()` is false for
  a registry holding a controller disposed mid-run, and `status()` after `dispose()` still reads the
  status it had (pin the parity).

Tests
- invert `zero_duration_forward_completes_on_the_first_scheduler_driven_frame` → "…before
  `forward()` returns": future `is_complete()`, value == upper bound, status `Completed`, value AND
  status listeners fired synchronously exactly once, `execute_frame()` afterwards changes nothing.
- `reverse()` on a zero-duration controller → `Dismissed` synchronously; `animate_to(0.3, ZERO)`
  from 0.7 → `Completed` (method-chosen!), `animate_back(0.3, ZERO)` from 0.1 → `Dismissed`;
  `animate_to(lower_bound)` from 0.5 over a real duration ends `Completed` (run-end rule); a
  zero-duration run displaces a live run: new status observed BEFORE the displaced run's
  `TickerCanceled` (existing order contract).
- `when_complete_or_cancel_chaining_ticks_once_per_frame_and_stop_fully_stops_it` uses a
  zero-duration `reverse()` as its deterministic first leg: under Z it completes synchronously, the
  continuation fires at registration, and the chain starts before frame 1 — its tick-count
  assertions change; rewrite it to drive the first leg over a real duration (or assert the new
  synchronous shape explicitly), state which.
- `dispose()` mid-run: `status()` is unchanged (parity pin), `tick_at` is a no-op,
  `Vsync::has_running()` on a registry holding it is false and `tick_all` skips it.
- flutter#1913 pin if absent: `forward()` at 0 then `reverse()` at 0 with no tick fires `Forward`
  then `Dismissed`.
- flui-widgets has NO implicit-animation test with a zero animation DURATION today (every
  `Duration::ZERO` there is a pump dt) — add one: an `AnimatedContainer`/`AnimatedOpacity` retarget
  with `Duration::ZERO` in `did_update_widget` (`ImplicitController::restart_from_zero` →
  `forward_from(Some(0.0))` now notifies value + status listeners synchronously inside the update
  phase); assert the new target is laid out on the same pump and the frame loop quiesces.
- Navigator blast radius, named: `navigator_tests.rs` routes with `.transition_duration(Duration::ZERO)`
  and `page_with_handle(&sink, Duration::ZERO)` (~L1688-2060) — PUSH now completes at `did_push`
  instead of the first driven frame (`TransitionRoute::install` handles an already-complete
  controller; the continuation path is pinned for `Animating(TickerFuture::complete())`,
  `navigator/tests.rs:642`); POP: `did_pop` → `reverse()` is `Dismissed` AT the call, so
  `handle_pop` (`navigator/history.rs:327-362`) takes the synchronous `finished_when_popped` →
  `Dispose` arm instead of parking in `Popping` — Flutter-correct (`routes.dart:173-176`); the later
  drain's second `finalize` is idempotent (pinned by
  `an_already_dismissed_controller_finalizes_synchronously_without_double_finalize`). The status
  listener touches only `pending_statuses`/`status_wake`, so `did_push` holding the route's
  controller lock across `forward()` cannot deadlock. Run all of them and report; add one explicit
  end-to-end zero-`transition_duration` push+pop through a REAL `TransitionRoute` asserting the
  synchronous settle ordering if none of the existing ones does.

---------------------------------------------------------------------------------------------------
## Cross-cutting constraints (house rules that bit last batch)
- Lock discipline: no user code under `inner`; every run-ending/starting site funnels through
  `finish`; `active_run` only via `.replace()/.take()` bound to a name (source-guard test pins it);
  `LockDiscipline/StatementDrop` port-check trigger is scoped to flui-scheduler + flui-foundation
  today — keep flui-animation clean anyway.
- No process markers (issue numbers OK as `Refs`, no "PR-R"/"slice" in shipped code/docs).
- Panic policy: `expect("BUG: …")` only for invariants; `clippy::unwrap_used` gate.
- `cargo doc --document-private-items -D warnings` after every rebase (PR-B lesson).
- Each PR body: `Closes #N`, what the outside lens found, measured numbers for V.
- Behavior-only breaks are invisible to `cargo public-api`/semver tooling: R and Z each add a
  `crates/flui-animation/CHANGELOG.md` entry (repeat phase origin + one period + exhaustion landing +
  `set_duration` no longer reaching a running repeat, zero-period repeats settle at the call; zero-duration synchronous settle,
  method-chosen direction for `animate_to`/`animate_back` — name the one production-visible change:
  the unbounded scroll controller's `animate_to` toward SMALLER pixels now reports `Forward`/
  `Completed` instead of `Reverse`/`Dismissed`, consumers verified indifferent; run-end status by
  direction; a disposed controller is skipped by `Vsync`; zero-distance settle no longer notifies
  value listeners).
