# Review brief — #1183: unboundedness is a constructor fact; bound-targeting runs refused; no path reads NaN

Worktree: /mnt/data/dev/flui-worktrees/anim-unbounded — READ-ONLY for every reviewer (an outside lens runs read-only
`cargo test` there; probes/mutations go in your own detached checkout under /mnt/data/dev/flui-worktrees-scratch/ with its
own CARGO_TARGET_DIR, removed when done; never `just ci` there). Base: origin/main 864b5798; one commit `0a85b933`
(`git diff origin/main...HEAD`, 9 files, +1504/−105).
Plan (v2 + v3 deltas, the contract): /mnt/data/dev/flui/.rust-studio/specs/1183-unbounded-controller/plan.md
Builder brief: /mnt/data/dev/flui/.rust-studio/specs/1183-unbounded-controller/brief.md
Issue: `gh issue view 1183`; flutter/flutter#76014. Flutter at 3.44.0 (`git -C /mnt/data/dev/flui/.flutter describe --tags`):
`AnimationController.unbounded`, `_internalSetValue`, `_animateToInternal`, `fling`, `_InterpolationSimulation.x`.
House rules: /mnt/data/dev/flui/AGENTS.md; `controller.rs`'s lock-discipline docs (`finish` only deliverer; warnings after
the guard drops; `active_run` via named `.replace()/.take()`; the source-guard test); panic policy.

Builder-reported deviations to judge: (1) `repeat_with`: caller NaN in `min`/`max` or an inverted/equal resolved range →
`InvalidBounds`; a resolved range non-finite only through an unbounded controller's defaulted bound → `NonFiniteTarget`
(two ordered checks). (2) `scroll_controller.rs` `service_pending_command` gates `is_scrolling(true)` on `Ok &&
fling.status().is_running()` (a synchronous settle inside `animate_to_curved` already cleared it via its own listener).
(3) The `set_value` warn latch has no `tracing` capture test (no `tracing-subscriber` dev-dep) — verified by reading.
(4) `docs/panic-policy-allowlist.txt` lost the `scrollable.rs` line (its only `.expect` went away).

## 5a — spec compliance (plan Decisions 1–6 + v3 deltas 1–8 + Tests list): MET / NOT MET / EXTRA with file:line.
In particular: every refusal BEFORE any mutation (walk each: `forward_from`, `reverse_from`, `drive_to`, `fling_with`,
`drive_simulation`, `repeat_with`); `fling_with` direction/target as locals, `InvalidSpring` ordering fixed;
controller-side `tracing::warn!` after the guard drops on every `NonFiniteTarget` return; ±∞ `target`/`from` CLAMP to a
finite bound and are REFUSED toward an infinite one, NaN always refused; span-overflow refusal; `set_value` rule (bounded
NaN → lower, bounded ∞ → bound, unbounded non-finite → full no-op, latched warn); simulation: non-finite `x(0)` refused,
mid-run non-finite sample ENDS the run (settled by direction, future `Ok`, ticker stopped, latched warn) — not "continues";
`tick_time_based` endpoint special-case; initial status per `settled_status_keep_direction` + `last_reported_status`
initialized to it; `reset()` → 0.0 on unbounded; constructors; `with_bounds*` + `builder.rs` reject non-finite/NaN/half-open;
three production sites migrated; `is_scrolling` ordering; the two inverted oracles rewritten and explained; mapping entry
covers everything the plan lists (incl. `Forward`-at-0 costs, path-dependent status, `reset` divergence, the
`InvalidBounds`/`NonFiniteTarget` split's why); CHANGELOG `### Added`/`### Changed`.

## 5b — code quality (attack)
1. Mutation probes in your scratch checkout: (a) move the `NonFiniteTarget` check in `drive_to` after `clear_run_modes`
   → which test reddens (the "untouched after refusal" one must); (b) restore `fling_with`'s early `inner.direction`
   write → the `stop()`-probe test; (c) drop the span-overflow check → its test; (d) make `tick_simulation` continue on a
   non-finite sample → the immortal-run test; (e) `last_reported_status = Dismissed` hard-coded again → the counting
   status-listener test; (f) `set_value` unbounded NaN → call `stop_running()` → the "live fling survives" test.
2. Lock discipline: any new user code under `inner`? The new warns — all after the guard drop? `tick_simulation`'s
   mid-run end path: delivery published before unlock, delivered via `finish` only? Source-guard test still green and
   still covering the new `take()` site.
3. Production consumers: `scrollable.rs`, `refresh_indicator.rs`, `scroll_controller.rs` — read every call on the fling
   controller and confirm none is now refused (`animate_to_curved(finite)`, `animate_with(sim)`, `set_value(finite)`,
   `stop`, `jump_to`'s stop hook); the `Forward`-at-0 initial status: does any consumer read `status()` before the first
   run and change behavior (the `dismissible`/`scrollable` status listeners)?
4. Deviation 2's `is_running()` gate — is there a case where `Ok` + not running is WRONG to leave `is_scrolling` false
   (a zero-distance settle is fine; anything else)?
5. NaN/∞ census: `rg -n 'clamp\(' crates/flui-animation/src/controller.rs` — every remaining `clamp` whose bound or input
   can be non-finite; `velocity()` on unbounded at 0 (start/target now 0 → 0.0?); `scaled_run_duration` with a
   non-finite range (unreachable now? say why).
6. Docs: mapping entry accuracy against the code; false Flutter claims; process markers; the `panic-policy-allowlist`
   removal is the right direction (ratchet shrink-only).

Output: `5a: PASS/FAIL`, `5b: APPROVE / NEEDS WORK / BLOCK`, numbered findings (file:line, severity, failing scenario,
fix), no praise, under 140 lines, commands at the end.
