# Repair round 1 — slice Z (worktree /mnt/data/dev/flui-worktrees/anim-zero, branch fix/1171-zero-duration-synchronous-settle)

Three reviewers (rust-reviewer with mutation probes M1–M8; an outside glm-5.3 lens; api-design-lead
with `cargo public-api`/`semver-checks` = 0 diff, so the doc/test trail IS the gate). 5a FAIL on two
items (a false consumer claim; a load-bearing line with no test), 5b NEEDS WORK. Fold ALL of the
items below into ONE commit on the branch, rerun the crate gates + `just ci`, report the tails and
red evidence. Line numbers are the reviewers' at 8e6aebdb.

## A. Cupertino button chains its release fade on `Completed` — now re-enters once per tap (should-fix)
`crates/flui-cupertino/src/button.rs` ~L511-527: the status listener starts the release fade on
`status == Completed` (doc: "Completed == reached the upper bound"), and the release IS
`animate_to_curved(0.0, …)` — the Forward method toward a lower value, which under the method rule
now ends `Completed` at 0.0 → the listener fires again → `animate_to_curved(0.0)` from 0.0 → a
zero-distance settle (terminates; probe: statuses `[Forward, Completed, Forward, Completed]`,
listener fired 2×). Fix to the oracle's own shape (`button.dart` `_animate`: `ticker.then(…)`):
chain the release on the press fade's returned `TickerFuture` via `when_complete_or_cancel`
(ADR-0064), only on `Ok` (a canceled press fade must not start a release), keep
`animate_to_curved(0.0, …)` for the release (parity: the oracle uses `animateTo(0.0)`). Drop the
status-listener chain. Update `start_press_fade`/`init_state` docs and the doc comment of
`tests/button.rs` `press_opacity_fades_out_then_back_in_over_the_oracle_durations` (it explains the
reentrant restart "from inside the status listener" — now from the future's continuation, which
`finish` delivers after the lock drop). Add a test in `crates/flui-cupertino/tests/button.rs`
pinning "the release fade starts exactly once per tap": after the press+release sequence
`run_generation()` advanced by exactly 2 (press, release) and the controller is idle — or an
equivalent observable; red-check: the old status-listener chain gives 3.

## B. Repeat-exhaustion direction sync ships with zero coverage (should-fix, both reviewers + API gate)
Your deviation (1) is right, and must be pinned NOW, not by the next change: add
`bounce_repeat_over_an_interior_range_exhausts_on_the_final_legs_status`:
`repeat_with(Some(0.2), Some(0.8), true, Some(Duration::from_millis(100)), Some(2))`, `tick_at(0.25)`
→ value 0.2, status `Dismissed`. Red-check (verified by the reviewer): revert the `inner.direction`
assignment before `settled_status()` → `Completed`. Show the red run.

## C. `set_value` mid-run on a `Vsync`-driven controller is overwritten by the next `tick_all` (should-fix, pre-existing, same predicate as this change)
Probe: `forward()`, `tick_all(0.0)`, `tick_all(0.05)` → 0.5, `set_value(0.2)`, `tick_all(0.10)` → 0.8.
`set_value` calls `stop_running()` (no run left, `active_run` None) but reports a directional
running status (Flutter parity), so `status().is_running()` is the WRONG "is a run installed"
predicate — `dismissible.rs` works around it by unregistering during drags. The precise predicate
is `active_run.is_some()`: every run starter installs a completer; every run end, `stop_running`
(`stop`/`set_value`/`reset`) and `dispose` take it. Fix: `WalkProbe.live_running = !disposed &&
active_run.is_some()` (document that `status().is_running()` can be true with no run installed,
and why); `tick_at`'s guard becomes "disposed or no run installed → return" (keep the
`status.is_running()` check only if something still needs it — say which). Tests: the probe above
as a `vsync.rs` test (`set_value` mid-run holds; `has_running()` false afterwards); a controller.rs
test that `tick_at` after `set_value` mid-run is a no-op. `is_animating()` stays ticker-based
(unchanged). Record in the mapping entry "`dispose` does not settle the status" (rename it to cover
"`Vsync` polls for an installed run, not a running status") and in CHANGELOG.

## D. Non-settling `forward_from(Some(x))`/`reverse_from(Some(x))` apply `from` silently (should-fix, small)
Flutter's `forward(from:)` goes through the `value=` setter, which notifies. The settle path now
handles it via `entry_value`; the real-run path passes `ValueChange::Unchanged`. Fix: at a
non-settling run start, `ValueChange::Notify` iff `from` moved the value (compare the clamped `from`
against the entry value). Test: `forward_from(Some(0.5))` from 0.0 over a real duration → exactly
one value notification at the call, none from `forward()` without `from`. Note the entry-value rule
in the mapping entry: `forward_from(Some(0.0))` from 1.0 with zero duration ends at 1.0 and fires
NO value notification (net unchanged; Flutter fires twice, both observers would read 1.0 anyway).

## E. Widgets test claims quiescence it did not measure (should-fix)
`crates/flui-widgets/tests/parity/animated_container_test.rs` ~L280 asserts only
`!vsync.has_running()`. Measured after that pump: `build_owner.pending_external_builds() == 1`,
`has_dirty_elements() == true` — the synchronous value notify inside `did_update_view` lands in the
external inbox mid-drain, `drain_build_scope` defers it one frame and `ExternalBuildScheduler`
requests a frame (Flutter builds the descendant in the same `buildScope`). Not this change's defect
(before it the same rebuild came from the first tick a frame later), but the test must assert the
true shape: same-pump layout of the new target; `pending_external_builds() == 1` after that pump
(name it as the redundant `AnimatedBuilder` rebuild the synchronous notify schedules through the
external inbox); one more pump → `== 0`, layout unchanged, `!has_running()`. If the parity harness
does not expose the owner's counters, say so and assert through what it does expose. Also MOVE the
test: it is a FLUI-specific regression pin, not a Flutter-oracle port — it does not belong in the
parity file with a `tests = 3` manifest bump. Put it with the `animated` module's own tests (or
`tests/widgets_it`) and revert `manifest.toml`. A follow-up issue for the same-scope build will be
filed by the coordinator; do not fix the BuildOwner here.

## F. Wrong Flutter line citations (should-fix)
`controller.rs` ~L820 cites `:585`/`:635` for the `_direction` assignments (actual `animateTo`
`:599` / `animateBack` `:636`); ~L828 cites `:657-661` for `directionDuration` (actual `:664-668`);
~L1433 cites `:940-944` for the `_tick` rule (actual `:948-950`; the `settled_status` doc already
has it right). Cite by SYMBOL (`animateTo`, `animateBack`, `_animateToInternal`'s `directionDuration`,
`_tick`) with the file, not bare line numbers, per the repo rule.

## G. Docs/CHANGELOG accounting (should-fix)
1. `docs/ARCHITECTURE.md` "Direction is chosen by the method…" ~L318-326: the sentence claiming the
   cupertino button calls toward 1.0/0.0 "where method and travel already agreed" is FALSE (item A);
   rewrite to name the button as the second consumer and what changed there; delete "Consumers
   checked before removing it" (process narrative). Extend the run-end rule to name
   `tick_simulation`: `animate_with(sim)` landing on a bounded controller's lower bound now ends
   `Completed`; `fling`'s end is by velocity sign.
2. `CHANGELOG.md` ~L75-83: two production-visible consequences (scroll controller; cupertino
   button — fixed in the same change), plus the `tick_simulation`/`animate_with`/`fling` run-end
   rule, plus items C and D.
3. `crates/flui-widgets/CHANGELOG.md`: one entry for the widget-visible consequences (zero-duration
   implicit retarget lays out on the same pump; zero-duration route pop finalizes synchronously in
   `handle_pop`; push still settles one pump later).

## H. Nits — do them
- `#[must_use]` on `walk_probe` (its sibling `run_generation` has it).
- Change-history narrative in shipped comments → state the invariant: `controller.rs` ~L69-72
  (`WalkProbe` doc: "instead of the two separate locks … measured at ~28%"), the chaining test's
  "now snaps … instead of installing a ticker run", `transition_route_tests.rs` ~L379-380,
  `animated_container_test.rs` ~L238-239 (wherever it lands after E). "now"/"previously"/"instead of"
  belong in CHANGELOG (present there).
- The rewritten chaining test uses `sleep(5ms)` + a 1 ms real run: acceptable (its sibling does the
  same); if a deterministic shape is cheap (`without_ticker` + `tick_at`), prefer it, else leave.

## Gates after the fix (paste tails)
`cargo nextest run -p flui-animation`, `-p flui-widgets`, `-p flui-material`, `-p flui-cupertino`,
`cargo clippy -p flui-animation -p flui-widgets -p flui-cupertino --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, `RUSTDOCFLAGS="-D warnings" cargo doc -p flui-animation --no-deps --document-private-items`,
`typos` on changed files, `cargo test -p flui-widgets --test parity_inventory`, then `just ci` once
(production code changes in A, C, D).

## Report back
Diff summary; red→green evidence for A, B, C, D, E; gate tails; anything you disagree with (say why
rather than skipping); `MEMORY:` lines.
