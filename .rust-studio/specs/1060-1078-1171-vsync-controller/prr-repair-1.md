# Repair round 1 — slice R (worktree /mnt/data/dev/flui-worktrees/anim-repeat, branch fix/1078-repeat-pure-time-sampling)

Reviewers: api-design-lead (API-GATE: PASS WITH FIXES), an outside deepseek lens (5a FAIL on three
clauses, 5b NEEDS WORK), rust-reviewer (5a FAIL, 5b NEEDS WORK; probes M(a)–(d) run in a scratch checkout). Fold ALL items into ONE commit,
rerun the crate gates + `just ci` (production code changes), report tails and red evidence.
Line numbers are the reviewers' at cf1c75e7.

## Production code (should-fix; item 0 is the shape the others land in)
0. **Typed repeat state (the reviewer's REDO-shape item — take it now, bounded to the touched
   fields).** Replace `is_repeating: bool` + `repeat_reverse/min/max/period/count/initial_ns` with
   `repeat: Option<RepeatRun>` where `struct RepeatRun { reverse: bool, min: f32, max: f32,
   period_ns: u128 /* > 0 by construction */, count: Option<u32>, initial_ns: u128 }`.
   `is_repeating` becomes `repeat.is_some()`; `clear_run_modes` sets `repeat = None`;
   `current_duration()` reads the period from it; `RepeatRun` is constructed only AFTER the
   zero-period and zero-count early returns, so `tick_repeat` needs neither `expect` nor the
   `period_ns == 0` guard (delete all three). ONE sampler owns the math: `fn repeat_sample(&self,
   total_ns: u128) -> RepeatSample { value, direction, start, target }` on `AnimationControllerInner`
   (built on `repeat_leg`/`repeat_landing`), called by `repeat_with` for the at-call state AND by
   `tick_repeat` — deleting the hand-copied `Forward => (min, max) / Reverse => (max, min)` match
   that exists twice today. Update `docs/ARCHITECTURE.md`'s state listing accordingly.
1. **At-call value for a restart repeat from `max` contradicts the model, its own comment and
   Flutter** (both reviewers, BLOCKING). `repeat_with` stores the clamped current value (`inner.value = v`, ~L1085) while the
   comment (~L1037-1044) and the model say the at-call value is the pure function SAMPLED at cycle 0
   — from `value == max` in restart mode that is `min` (phase wraps; Flutter `_value = x(0.0)`).
   Today `set_value(1.0); repeat(false)` reads 1.0 at the call and 0.0 at `tick_at(0.0)` — a
   one-frame discontinuity the whole change exists to remove. Fix: compute `(start, target, phase)`
   with `repeat_sample(initial_ns)` (item 0) and store its value (still `ValueChange::Unchanged`).
   Red test: `set_value(1.0); repeat(false)` → `value() == 0.0` at the call and `tick_at(0.0)` fires
   NO value notification (no discontinuity); bounce from `max` still reads `max` (its existing test).
2. **`clear_run_modes()` in `repeat_with` is pinned by nothing** — `tick_repeat` never reads
   `run_curve`, so `a_leftover_curve_does_not_shape_a_following_repeat` stays green with the call
   deleted (its 0.5 comes from the linear rewrite). The observable the clear protects is
   `velocity()`: a leftover `simulation` short-circuits it (`sim.dx`), a leftover `run_duration`
   changes `current_duration()`. Red test: `fling(1.0)` (installs a simulation) then
   `repeat(false)` with a 1 s period → `tick_at(0.25)` → `value() == 0.25` AND `velocity() == 1.0`
   (range/period), red with the clear removed. Keep the curve test but restate its purpose in its doc
   (it pins that the repeat is linear, not that the clear happens).
3. **`velocity()` on a reverse leg is asserted nowhere** — the mapping entry's (g) cites
   `reverse_mid_flight_keeps_full_range_velocity`'s "sibling repeat coverage", which does not exist
   (`grep "\.velocity()"` in the crate's tests: none). Add `repeat(true)` → `tick_at(1.5)` (reverse
   leg of a 1 s period) → `velocity() == -1.0`; fix the entry's citation to name the new test.
4. **`repeat_with(count: Some(0))` is reachable and undocumented.** Today it installs a running run
   that exhausts on the FIRST tick (`total_ns >= 0`), landing on cycle 0's end via `saturating_sub` —
   i.e. it behaves like `Some(1)` but holds the frame loop open until a frame arrives. Flutter asserts
   `count > 0`; Compose throws for `iterations < 1`; the Web spec runs zero iterations as an empty
   active interval that finishes at once. Take the Web arithmetic (repair-not-reject is the house
   rule): `Some(0)` = zero cycles → settle synchronously at the call through the same settle path as
   the zero-period case, with the value the CLAMPED current value (no cycle ran, so no landing jump),
   status `Forward.settled_status()` (`Completed`), a complete future. Document it in the `count`
   bullet of `repeat_with`'s doc and in the mapping entry; test it (value unchanged, status
   `Completed`, future complete, `run_generation` unchanged, a later `tick_at` changes nothing).
5. Second `expect` in `tick_repeat` (~L1509-1511) is avoidable: fold the exhaustion predicate into a
   let-chain `if let Some(count) = inner.repeat_count && total_ns >= u128::from(count) * period_ns`
   (the file already uses that form) — one panic path fewer. The first `expect`
   (`repeat_period` is `Some` while repeating) is a genuine invariant; keep it.
6. `Duration::try_from_secs_f64(cycle.max(0.0)).map_or(0, …)` (~L1501) saturates the WRONG way:
   an `Err` — `f64::INFINITY`, or `raw / time_dilation` overflowing (`set_time_dilation` accepts any
   positive finite, e.g. `1e-300`) — becomes `elapsed_ns = 0`, a rewind to the phase origin, so a
   finite repeat ticked with `tick_at(f64::INFINITY)` never exhausts while `forward()` on the same
   input completes. Fix: `.unwrap_or(Duration::MAX).as_nanos()` (the sum with `initial_ns` still fits
   u128 — say so); drop the redundant inner `.max(0.0)` (`tick_at` already clamps). Red test: finite
   `count: Some(2)` repeat, `tick_at(f64::INFINITY)` → exhausted, `Completed`/landing value.
   NaN cannot reach `cycle` (`f64::max` returns the non-NaN operand) — note it in the comment.
6b. `repeat_with(Some(0.5), Some(0.5), …)` → `InvalidBounds`: add the equality pin —
   `repeat_with_rejects_inverted_range` tests only `0.8 > 0.2`, yet the mapping entry and the
   interior-range oracle doc cite it as the replacement for Flutter's dropped `min: 1.0, max: 1.0`
   sub-case (rule 1(b)).

## Docs (should-fix; api-lead's item 1 is BLOCKING for sign-off)
7. `pub fn repeat`'s rustdoc (~L954-963) still says "An infinite repeat's `TickerFuture` resolves
   only by cancellation — it has no natural end." False for a zero effective period (an infinite
   repeat on a `Duration::ZERO` controller settles at the call with a complete future — reachable,
   verified). Add the exception to `repeat`'s doc and cross-reference it from `repeat_with`'s
   `period` bullet: "except that a zero effective period settles synchronously at the call — see
   `repeat_with`". Also document `count: Some(0)` (item 4) in the `count` bullet.
8. CHANGELOG placement: the entry landed in the ROOT `CHANGELOG.md`; the two previous
   flui-animation changes (#1179, #1181) live ONLY in `crates/flui-animation/CHANGELOG.md` under
   `### Changed`. Move it there (same category); no root duplicate; drop the private-field
   sentence (`AnimationControllerInner drops repeat_done/run_epoch_secs …` is not consumer-visible).
   Add one clause that `cargo public-api`/`semver-checks` report no diff for these behavior-only
   breaks (the tooling's green did not cover them).
9. `docs/ARCHITECTURE.md`: (a) stale run-end attribution at ~L277 and ~L312-313 — the repeat-exhausted
   end is `tick_repeat`'s now, not `tick_time_based`'s; (b) the mapping entry cites bare Flutter line
   numbers three times (`:861-871`, `:717-745`, `:1053`) — cite by symbol (`AnimationController.repeat`,
   `_startSimulation`, `_RepeatingSimulation.dx`) with the file; (c) ~L474-475 states Compose's
   `InfiniteRepeatableSpec` zero-duration rejection as fact while the repo's survey marks it
   unverified — verify at source (`curl -sL https://raw.githubusercontent.com/androidx/androidx/androidx-main/compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/AnimationSpec.kt | grep -n "0-duration"`)
   and cite the class, or soften to "Compose's docs say…"; (d) fix the (g) citation per item 3;
   (e) note the `Some(0)` rule per item 4; (f) entry (a) MIS-DESCRIBES the old defect — the old
   code advanced the epoch by whole cycles and got the state right; the bug was that a
   boundary-crossing tick REPORTED the boundary and deferred the remainder to the next tick
   (`tick_at(1.25)` → 0.0 vs `tick_at(1.0); tick_at(1.25)` → 0.25, #1078's probes) — rewrite (a)
   to say that and drop "only one of them was right"; (g) ~L467 uses rustdoc intra-link syntax
   (`[`with_bounds`](AnimationController::with_bounds)`) in a Markdown file — plain backticks.
10. Test-doc and residual prose: `bounce_repeat_over_an_interior_range_exhausts_on_the_final_legs_status`'s
    red-check (~L2596-2599) names the deleted `tick_time_based` assignment — re-point it to
    `repeat_landing` (e.g. "make `repeat_landing` ignore parity"); `controller.rs` ~L1272-1275
    (`run_generation` doc "re-zeros the run epoch"), ~L1286-1288 (the settle-without-restart list —
    add `repeat_with`'s zero-period and zero-count settles), `set_duration` ~L464 ("re-establishes
    the run epoch") describe the deleted field — say "begins the run at elapsed zero" instead.
    `vsync.rs` ~L23-25 speaks of the registry's own anchor and is fine.

## Gates after the fix (paste tails)
`cargo nextest run -p flui-animation`, `-p flui-widgets`, `-p flui-material`,
`cargo clippy -p flui-animation --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
`RUSTDOCFLAGS="-D warnings" cargo doc -p flui-animation --no-deps --document-private-items`,
`typos` on changed files, `cargo check -p flui --examples`, then `just ci` once.

## Report back
Diff summary; red→green evidence for items 1, 2, 3, 4, 6, 6b; gate tails; anything you disagree with (say
why rather than skipping); `MEMORY:` lines.
