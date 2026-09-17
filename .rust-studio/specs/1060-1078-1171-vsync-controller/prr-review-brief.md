# Review brief — repeat sampling is a pure function of elapsed time (closes #1078)

Worktree: /mnt/data/dev/flui-worktrees/anim-repeat — READ-ONLY for every reviewer (an outside lens runs
`cargo test` there concurrently; any build/mutation probe goes in your own detached checkout:
`git -C /mnt/data/dev/flui-worktrees/anim-repeat worktree add --detach /mnt/data/dev/flui-worktrees-scratch/<name> HEAD`,
own `CARGO_TARGET_DIR`, removed when done). Never `just ci` there.
Base: origin/main (57118981). One commit `cf1c75e7`; `git diff origin/main...HEAD` (+873/−181, 5 files).
Plan (Slice R, v4): /mnt/data/dev/flui/.rust-studio/specs/1060-1078-1171-vsync-controller/plan.md
Builder brief (operative spec): /mnt/data/dev/flui/.rust-studio/specs/1060-1078-1171-vsync-controller/brief-R.md
Market survey (Q1 is the model's provenance — Web Animations §4.8.3.2-4.9.1, Android `ValueAnimator`
`getCurrentIteration`, Compose `VectorizedRepeatableSpec`):
/mnt/data/dev/flui/.rust-studio/research/animation-repeat-zero-duration-registry-market-survey.md
Issue: `gh issue view 1078` (its three failing probes are the acceptance floor). flutter/flutter#67507 for
the phase-continuity contract. Flutter reference pinned 3.44.0 (`git -C /mnt/data/dev/flui/.flutter describe --tags`):
`animation_controller.dart` `repeat` L717-745, `_startSimulation` L861-872, `_RepeatingSimulation` L1005-1061;
`test/animation/animation_controller_test.dart` L873-1000, L1269-1355; `test/scheduler/scheduler_tester.dart`
(`tick(d)` = `handleBeginFrame(d)`, an ABSOLUTE timestamp — `tick(100)` then `tick(60)` rewinds).
House rules: /mnt/data/dev/flui/AGENTS.md; `controller.rs`'s own lock-discipline docs (`finish`,
`restart_ticker`, `active_run`, the source-guard test); no process markers; panic policy
(`expect("BUG: …")` only for internal invariants).

Builder-reported deviations to judge: (1) `repeat_bounce_flutter_oracle_interior_range` got a
second `jumped` sub-case because the sequential port passed on the old code too; (2) the CHANGELOG
entry landed in the ROOT `CHANGELOG.md`, not `crates/flui-animation/CHANGELOG.md` where the two
previous changes put theirs — decide which is right for this repo and whether both are needed;
(3) two new `expect("BUG: …")` calls in `tick_repeat` (`repeat_period` is `Some` while repeating;
`repeat_count` is `Some` when `exhausted`) — genuine invariants or a shape that should not need them?

## 5a — spec compliance (item by item vs brief-R.md Model 1-7, Docs, Tests)
MET / NOT MET / EXTRA with file:line. In particular: `run_epoch_secs` and `repeat_done` gone
everywhere (grep the crate AND `docs/ARCHITECTURE.md`'s state listing); `repeat_initial_ns` computed
from the CLAMPED current value with the f32→f64 widening before the subtraction; integer-ns
`total_ns`/`i`/`phase`/exhaustion with the `period_ns == 0` early return BEFORE the division and no
`expect` there; `repeat_leg`/`repeat_landing` the ONLY place parity lives (grep for `% 2` and
`is_odd`-like arithmetic elsewhere); `tick_time_based` has no repeat branch left; `repeat_with`
calls `clear_run_modes()` first, resolves the period once, keeps the `lo >= hi` rejection, samples
the at-call value/direction at cycle 0 with `ValueChange::Unchanged`, settles the zero-period case
BEFORE `restart_ticker` through `settle_at_target`; every listed test present with a plausible
red-check (name any missing); README/GUIDE fixed; the mapping entry covers (a)–(g) and every Flutter
claim in it is TRUE at the tag.

## 5b — code quality (attack)
1. Partition invariance — construct a timestamp pair where direct and partitioned sampling could
   still differ: time dilation changing between ticks; `set_duration` mid-repeat (must be inert);
   a `tick_at` with a NEGATIVE `raw_elapsed_secs` (clamped how?); `Duration::try_from_secs_f64` on a
   huge or NaN `cycle` (what does `map_or(0, …)` do to a NaN — is a NaN elapsed silently phase 0?);
   `u128::from(count) * period_ns` overflow (`u32::MAX × 584 years` fits? say so).
2. Exhaustion: `total_ns >= count * period_ns` at EXACTLY the boundary lands on cycle `count-1`'s end
   — verify against the Web spec's "hold the endpoint of the final iteration" and Android's
   `getCurrentIteration` decrement; check `count.saturating_sub(1)` for `count == 0` (is `Some(0)`
   reachable? Flutter asserts `count > 0`; FLUI's public `repeat_with(count: Some(0))` — what happens,
   and is it documented?).
3. Status/value notification counts: exactly one value notification per tick; a leg flip fires one
   status change; an even bounce skip fires none. Verify by reading `finish`/`take_status_change`
   and by the tests. Does `repeat_with` at the call fire a status change when the at-call leg is
   already the current status (e.g. `Forward` → `Forward`)? Flutter parity?
4. Lock discipline: `tick_repeat` runs no user code under `inner`; the zero-period settle path in
   `repeat_with` — is `entry_value` plumbed so the value notification fires iff the value moved?
   Does the settle path cancel the displaced run AFTER the new status (existing `finish` order)?
   Source-guard test still covers every `TickerCompleter`/`TickerDelivery` site.
5. `velocity()` under the new model: `cycle_elapsed_secs()` lost its epoch subtraction — is the
   reverse-leg sign still right, and what does `velocity()` return for a repeat at the exact
   exhaustion tick (settled → 0)?
6. `restart_ticker`'s doc and `run_generation`'s doc: still true after the `run_epoch_secs` deletion?
   Any doc in `vsync.rs`/`ARCHITECTURE.md` that still describes the retire-a-cycle model?
7. Tests: mutate in your scratch checkout — (a) drop the `+ repeat_initial_ns` → which tests
   redden; (b) `>` instead of `>=` in the exhaustion predicate → the c = 3 boundary test only?;
   (c) f64 predicate instead of integer-ns → which; (d) skip `clear_run_modes()` → the curve test.
   For the two Flutter-oracle ports that carry a rewind (`tick_at(0.10)` then `tick_at(0.06)` → 0.6),
   confirm the assertion is on the REWOUND sample, not a typo.
8. Docs: mapping entry — process narrative, false claims, stale line numbers; the two `expect`s'
   messages; the README/GUIDE snippets compile as `rust,ignore` or run as doctests?
9. Anything the change makes worse: `u128` arithmetic on the hot tick path (cost vs the f64 it
   replaced — negligible, but say so), the `Duration::try_from_secs_f64` per tick.

Output: `5a: PASS/FAIL`, `5b: APPROVE / NEEDS WORK / BLOCK`, numbered findings each with file:line,
severity (blocking / should-fix / nit), the concrete failing scenario or wrong claim, and the fix.
No praise. Under 140 lines. "no finding" for an area checked and sound. Commands run, at the end.
