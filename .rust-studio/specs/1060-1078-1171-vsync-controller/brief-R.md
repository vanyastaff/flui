# Builder brief — Slice R: repeat sampling is a pure function of elapsed time (closes #1078)

Worktree (yours alone): /mnt/data/dev/flui-worktrees/anim-repeat
Branch: fix/1078-repeat-pure-time-sampling (from origin/main 57118981, which contains the
zero-duration/direction change: `settle_at_target(inner, entry_value)`,
`AnimationDirection::settled_status`, the `is_running()` ticker-stop guard, `walk_probe` with
`live_running = !disposed && active_run.is_some()`, and the repeat-exhaustion branch's direction
assignment that your `repeat_landing` helper absorbs). Do not touch /mnt/data/dev/flui.
Plan (Slice R + cross-cutting, v4): /mnt/data/dev/flui/.rust-studio/specs/1060-1078-1171-vsync-controller/plan.md
Market survey (read Q1 — the model is the Web Animations timing model, Android `ValueAnimator`,
Compose `VectorizedRepeatableSpec`; Flutter's `% 1.0` wrap is the outlier):
/mnt/data/dev/flui/.rust-studio/research/animation-repeat-zero-duration-registry-market-survey.md
Issue: `gh issue view 1078` (its reproduction values are the first RED tests). Also read flutter#67507
(`gh issue view 67507 -R flutter/flutter`) — the phase-continuity contract this adopts.
Flutter reference at 3.44.0 (`git -C /mnt/data/dev/flui/.flutter describe --tags`):
`animation_controller.dart` `repeat` (L717-745), `_startSimulation` (L861-872), `_RepeatingSimulation`
(L1005-1061); tests `animation_controller_test.dart` L873-1000 and L1269-1355; the harness
`test/scheduler/scheduler_tester.dart` — its `tick(d)` is an ABSOLUTE frame timestamp (`handleBeginFrame(d)`),
so `tick(100ms)` then `tick(60ms)` REWINDS to 60 ms (the 0.6 sample is elapsed 60 ms, not 160 ms).
House rules: /mnt/data/dev/flui/AGENTS.md; the lock-discipline docs in `controller.rs` (`finish`,
`restart_ticker`, `active_run`, the source-guard test); no process markers; panic policy.

## Goal (observable)
For a repeating run, `value`/`status`/`direction` at any `tick_at(t)` are a pure function of
(elapsed since the run started, period, min, max, reverse, the value at the call, count) — the frame
partition never changes the answer: `tick_at(1.25)` == `tick_at(1.0); tick_at(1.25)`, for restart and
bounce, for odd and even numbers of skipped cycles, and `tick_at(2.25); tick_at(2.25)` is idempotent.
The issue's three failing probes become green.

## Model (decided; re-derived from the Web Animations timing model, not transcribed from Dart)
1. State: REMOVE `repeat_done` and `run_epoch_secs` (after this change nothing writes it but 0 —
   `restart_ticker` sets it to 0.0; delete the field, the subtraction in `tick_at`/`cycle_elapsed_secs`,
   and its line in `docs/ARCHITECTURE.md`'s state listing). ADD `repeat_initial_ns: u128` — the phase
   offset the value at the call sits at: `round((f64::from(v) - f64::from(min)) / (f64::from(max) - f64::from(min)) * period_ns as f64)`
   with `v = value.clamp(min, max)`; widen the f32s BEFORE subtracting (near `max` the f32 difference
   loses bits; ~60 ns quantization at a 1 s period, say so in a comment).
2. Integer-nanosecond arithmetic everywhere (Compose and GPUI reduce modulo the period in nanos
   before any float conversion; the f64 predicate `total >= c * period` is wrong: `0.3 >= 3.0 * 0.1`
   is FALSE): `period_ns = period.as_nanos()`; `elapsed_ns = Duration::try_from_secs_f64(cycle.max(0.0)).map_or(0, |d| d.as_nanos())`
   (`cycle` = dilated seconds since the run started, as `tick_at` already computes); `total_ns =
   elapsed_ns + repeat_initial_ns`; `i = total_ns / period_ns` (guard `period_ns == 0` with an early
   return BEFORE the division — that case never reaches a tick, see 6 — never an `expect`);
   `phase = (total_ns % period_ns) as f64 / period_ns as f64`.
3. ONE place owns the leg/landing math — two private helpers on `AnimationControllerInner`:
   `fn repeat_leg(reverse: bool, index: u128) -> AnimationDirection` (`Forward` unless `reverse && index odd`)
   and `fn repeat_landing(&self, index: u128) -> (f32, AnimationDirection)` = the END of cycle `index`
   (`Reverse` leg → `repeat_min`, `Forward` leg → `repeat_max`). Every other site calls these; no
   hand-copied parity arithmetic anywhere else.
4. New `fn tick_repeat(&self, inner, cycle: f64)` dispatched from `tick_at` when `is_repeating`;
   `tick_time_based` loses its repeat sub-branch entirely (delete it, including the direction-sync
   assignment the previous change added there — it moves into `repeat_landing`).
   * exhausted iff `count.is_some_and(|c| total_ns >= u128::from(c) * period_ns)` → `repeat_landing(c-1)`
     sets `value`/`start_value`/`target_value`/`direction`, status = `direction.settled_status()`,
     `is_repeating = false`, ticker stop (`is_running()` guard, as `settle_at_target` does),
     `active_run.take().map(TickerCompleter::complete)`, `finish(status, Notify, delivery, inner)`.
   * not exhausted: `direction = repeat_leg(reverse, i)`; `start_value/target_value` = that leg's
     endpoints (keeps `velocity()` SIGNED — a recorded divergence from Flutter's unsigned
     `(max-min)/period`); `value = lerp(start, target, phase)`; `status = direction.running_status()`;
     `finish(status, Notify, None, inner)` — `take_status_change` dedups, so a flip fires exactly one
     status change and an even number of skipped bounce cycles fires none. No curve is applied.
5. `repeat_with`: (a) call `clear_run_modes()` FIRST (today it does not — a leftover
   `animate_to_curved` curve leaks into the repeat); (b) resolve the period ONCE:
   `repeat_period = Some(period.unwrap_or(inner.duration))` — one period for both legs, `set_duration`
   during a repeat no longer reaches it (Flutter: `period ??= duration`, captured by the simulation);
   (c) keep the `lo >= hi` → `InvalidBounds` rejection (Flutter permits `min == max`; record why we
   don't: a repeat that can never change value is a caller error and would hold the frame loop open
   doing nothing — the same contract `with_bounds` applies); (d) the value AT THE CALL is the pure
   function sampled at cycle 0 (`total_ns = repeat_initial_ns`): from `value == max` in RESTART mode
   that is `min` (phase wraps — Flutter's `_startSimulation` sets `_value = x(0.0)`); in bounce mode
   `max` on the reverse leg; stored with `ValueChange::Unchanged` (Flutter sets `_value` without
   `notifyListeners()`; the existing `ValueChange` doc already records this) — the same sample gives
   the initial direction/status (`Reverse` iff `reverse && i0 odd`; Flutter reports `reverse` at the
   call too, because `x(0.0)` runs `directionSetter` before `_status` is computed).
6. ZERO EFFECTIVE PERIOD, any count → synchronous settle at the call, early return BEFORE
   `restart_ticker` (`run_generation` untouched): `repeat_landing(count-1)` for a finite count,
   `repeat_landing(0)` (= `max`) for an infinite one; then the same settle the zero-duration runs
   use (`settle_at_target(inner, entry_value)`-style: directed status once, displaced run canceled,
   `TickerFuture::complete()`). Android's rule ("0 duration animator, ignore the repeat count and skip
   to the end"); Compose throws, Flutter asserts; repair-not-reject is the house rule and a
   zero-period infinite repeat ticking once per frame would hold the frame loop open doing nothing.
   Document the exception to "an infinite repeat's future resolves only by cancellation".
7. `repeat`/`repeat_with` docs: phase-continuity ("starts from the current value clamped into
   [min, max]; to start at `min`, `set_value(min)` first" — flutter#67507's confusion, answered);
   "`count` boundaries are measured from the phase origin, so a run started mid-cycle ends `count`
   boundaries later, not `count` full periods" (Flutter `_exitTimeInSeconds = count*period − _initialT`;
   Compose `iterations*duration − initialOffset`); finite runs land on the END of the last cycle.

## Docs
- `docs/ARCHITECTURE.md` `## Mapping decisions`: new entry "Repeat sampling is a pure function of
  elapsed time" recording: (a) the FLUI defect fixed (partition dependence, #1078); (b) initial phase
  from the current value = Flutter parity, and why (no jump; a `repeat()`-per-build pattern
  progresses instead of freezing); (c) IMPROVEMENT over Flutter: exhaustion lands on the last cycle's
  endpoint with that leg's settled status — Flutter's `% 1.0` wraps a 1-count restart repeat to `min`
  at `completed` (`animation_controller_test.dart` 'calling repeat by setting count as valid with
  reverse as false' expects 0 at 100 ms) and its bounce exhaustion reports the NEXT leg's direction
  (`count: 1` bounce ends `dismissed` at 1.0); the Web Animations spec ("holding the endpoint of the
  final iteration rather than the start of the next"), Android and Compose all land on the end —
  replaced oracle = the FLUI tests named below; (d) `min == max` rejected, with the why; (e) zero
  period settles at the call (Android; Compose rejects, Flutter asserts); (f) one period for both
  legs and `set_duration` inert mid-repeat (Flutter parity; the old `reverse_duration` fall-through
  contradicted `repeat_with`'s own doc); (g) `velocity()` signed on a reverse leg (deliberate
  divergence from Flutter's unsigned `dx`). Cite file + symbol, not bare line numbers.
- `README.md` L~84 and `docs/GUIDE.md` L~91 call a non-existent `repeat_with_reverse(true)` — replace
  with the real API (`repeat(true)` / `repeat_with(..)`).
- `CHANGELOG.md`: one `### Changed` entry listing every behavior change (phase origin, one period,
  `set_duration` inert mid-repeat, exhaustion landing/status, zero period settles at the call, curve
  no longer leaks into a repeat).

## Tests (RED first where marked; show the failing run before the production change)
- RED, the issue's own probes: 1 s period — restart `tick_at(1.25)` == `tick_at(1.0); tick_at(1.25)`
  (0.25); bounce (0.75); `tick_at(2.25)` twice → unchanged; odd/even skipped cycles (1.25 / 2.25 /
  3.25), custom `min/max`.
- RED, Flutter oracles ported with a doc citation of the oracle case name (`animation_controller_test.dart`):
  value 1.0 → `repeat(reverse: true)` → 0.75 @ 25 ms, 0.25 @ 125 ms; value 0.5 → 1.0 @ 50 ms, 0.0 @ 150 ms;
  `min 0.5 max 1.0` from 0.0 → at-call value 0.5 (the silent clamp), 0.75 @ 50 ms, 1.0 @ 100 ms,
  0.5 @ 200 ms; `min 0.2 max 0.6` from 0.2 → 0.4 @ 50 ms; `[-1, 3]` bounds: `repeat(min: 1, max: 3)`
  from 1.0 → 1 @ 0, 2 @ 50 ms; `repeat(min: -1, max: 3)` from 0.0 → 0 @ 0, 1 @ 25 ms; bounce
  `count: 4` from 0 → 0.25 @ 25, 0.5 @ 50, 0.99 @ 99, 1.0 @ 100, then `tick_at(0.06)` → 0.6 (the
  oracle's rewind, exact under pure sampling) and separately `tick_at(0.16)` → 0.4; exhausted at
  400 ms on `min` with status `Dismissed`.
- RED: value == max, bounce → at-call value `max`, status `Reverse`, no tick.
- RED: exhaustion boundary c = 3, period 100 ms, `tick_at(0.3)` → exhausted (f64 would be a frame late).
- RED: finite count with initial phase: value 0.5, `count: 1` restart → completes at 0.5 period on `max`, `Completed`.
- RED: zero period, finite AND infinite → complete future + settled status + landing value at the
  call; a later `tick_at` changes nothing; `run_generation` unchanged.
- RED: a leftover `animate_to_curved` curve does not shape a following repeat.
- RED: `set_duration` during an active repeat leaves the running period unchanged.
- Status/value notification counts across a multi-cycle frame: bounce even skip → 0 status changes,
  odd → 1; restart → 0; exactly one value notification per tick.
- Pins (green today, keep): `velocity()` negative on a reverse leg; `repeat_with_finite_count_stops`,
  `repeat_consumes_all_cycles_in_one_long_frame`, `finite_repeat_future_completes_when_the_count_is_exhausted`,
  `infinite_repeat_future_only_resolves_via_stop`, `repeat_with_rejects_inverted_range`,
  `bounce_repeat_over_an_interior_range_exhausts_on_the_final_legs_status` (from the previous change —
  keep it green through the rewrite); `repeat_with_clamps_range_into_bounds` (update its message —
  the run starts at the current value clamped into the range, not "at the clamped min").
- The source-guard test must still pass (new `TickerCompleter`/`TickerDelivery` shapes go through
  `finish`).

## Gates (in the worktree; paste tails)
`cargo nextest run -p flui-animation`, `cargo nextest run -p flui-widgets`, `cargo nextest run -p flui-material`,
`cargo clippy -p flui-animation --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
`RUSTDOCFLAGS="-D warnings" cargo doc -p flui-animation --no-deps --document-private-items`, `typos` on
changed files, then `just ci` once at the end. Also run the two `examples/` callers of `.repeat(true)`
(`examples/animated_box_app.rs`, `examples/vertical_slice_demo/frame_histogram.rs`) through `cargo check --examples`.

## Constraints
- No user code under `inner`; every delivery through `finish`; `active_run` only via `.replace()/.take()`
  bound to a name; no process markers; no `unwrap()` on production paths; no `expect` for the
  `period_ns == 0` guard.
- Public signatures unchanged (`repeat`, `repeat_with` keep their shapes). Commit on the branch
  (`fix(animation): …`), explicit paths, no push.

## Report back
Diff summary; red→green evidence per RED test; gate tails; deviations and why; `NOTE:` lines;
`MEMORY:` lines for durable non-obvious facts.
