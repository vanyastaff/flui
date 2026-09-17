# Builder brief — #1183: unboundedness is a constructor fact; bound-targeting runs on it are refused; no path reads NaN

Worktree (yours alone): /mnt/data/dev/flui-worktrees/anim-unbounded, branch fix/1183-unbounded-controller-refuses-bound-targets
(from origin/main 864b5798). Never edit /mnt/data/dev/flui. Scratch files under /mnt/data/dev/flui-worktrees-scratch/.
The operative spec is the PLAN, v2: /mnt/data/dev/flui/.rust-studio/specs/1183-unbounded-controller/plan.md — read it
fully; its Decisions 1–6, its Tests list AND its `## v3 deltas` section are the contract (RED first for everything not marked pin; show the failing
run before the production change). Issue: `gh issue view 1183`; flutter/flutter#76014. Flutter reference pinned at
3.44.0 (`git -C /mnt/data/dev/flui/.flutter describe --tags`): `AnimationController.unbounded`, `_internalSetValue`,
`_animateToInternal`, `fling`, `_InterpolationSimulation.x` in `animation_controller.dart`.
House rules: /mnt/data/dev/flui/AGENTS.md (no process markers; `tracing` only; panic policy — no `expect` on caller
input, `expect("BUG: …")` only for internal invariants); `controller.rs`'s own lock-discipline docs (`finish` is the only
deliverer; `active_run` via named `.replace()/.take()`; the source-guard test; warnings only after the guard drops —
`warn_if_no_ticker`'s pattern); the `LockDiscipline/StatementDrop` port-check trigger.

## Where
- `crates/flui-animation/src/controller.rs`: new constructors `unbounded(duration, &UpdateScheduler)`,
  `unbounded_without_ticker(duration)`, `unbounded_with_detached_ticker(duration)` (a private `unbounded_inner(duration,
  ticker)`); `with_bounds_inner` rejects `!(lower < upper) || !lower.is_finite() || !upper.is_finite()`; initial
  status = `settled_status_keep_direction()` of the initial value (bounded at `lower` stays `Dismissed`); `start_value`/
  `target_value` = the initial value; `forward_from`/`reverse_from`/`drive_to`/`fling_with`/`repeat_with` refusals
  BEFORE any mutation (`fling_with`: direction into a local; the `InvalidSpring` check moves before the direction write);
  `reset()` on unbounded → 0.0/`Dismissed`; `set_value` canonicalization + `non_finite_warned: bool` latch;
  `tick_time_based` endpoint special-case (`t == 0` → `start_value`, `t >= 1` → `target_value`, interior lerp);
  `tick_simulation`/`drive_simulation` non-finite sample → value unchanged + latched warn; a `warn_non_finite_target`
  emitted after the guard drops on every `NonFiniteTarget` return (production callers discard the `Result`).
- `crates/flui-animation/src/error.rs`: `NonFiniteTarget(String)` with doc naming the methods that return it.
- `crates/flui-animation/src/builder.rs` (~L124): the same bounds validation.
- `crates/flui-widgets/src/scroll/{scrollable.rs,scroll_controller.rs,refresh_indicator.rs}`: migrate to
  `unbounded_without_ticker(Duration::from_millis(1))` (the `.expect(..)` and its comment go away);
  `scroll_controller.rs` `service_pending_command` (~L492-501): `set_is_scrolling(true)` only after the run start
  returned `Ok` (leave it clear on `Err`).
- `crates/flui-animation/docs/ARCHITECTURE.md` `## Mapping decisions`: the entry named in the plan's Decision 6, citing
  by symbol; state listing if fields change. `crates/flui-animation/CHANGELOG.md`: `### Added` + `### Changed` per the plan.
  Constructor and method rustdoc per the plan (`# Errors` sections).

## Tests
Exactly the plan's list. The two existing tests `without_ticker_bounds_rejects_invalid_bounds_and_accepts_wide_open_ones`
and `with_detached_ticker_bounds_rejects_invalid_bounds_and_accepts_wide_open_ones` are deliberately INVERTED oracles —
rewrite them to "rejects wide-open bounds; `unbounded_*` starts at 0.0" and say so in their docs; do not preserve the
`NEG_INFINITY` assertion. For "nothing fired" use a value-listener counter AND a status-listener counter (status is
deduplicated, so `status()` equality proves nothing). For "direction untouched" probe through `stop()`'s reported status
against a fresh twin controller.

## Gates (paste tails)
`cargo nextest run -p flui-animation`, `-p flui-widgets`, `-p flui-material`, `cargo clippy -p flui-animation -p flui-widgets
--all-targets -- -D warnings`, `cargo fmt --all -- --check`, `RUSTDOCFLAGS="-D warnings" cargo doc -p flui-animation --no-deps
--document-private-items`, `typos` on changed files, `cargo check -p flui --examples`, `bash scripts/port-check.sh`, then
`just ci` once at the end.

## Constraints
Public additions only (`unbounded*`, the variant); `with_bounds*` behavior change (rejecting non-finite) is declared in
CHANGELOG. No `unwrap()`/`expect` on caller input anywhere in the new paths. Commit on the branch (`fix(animation): …`),
explicit paths, no push.

## Report back
Diff summary; red→green evidence per RED test (name the pins honestly); gate tails; deviations and why; `NOTE:` lines;
`MEMORY:` lines.
