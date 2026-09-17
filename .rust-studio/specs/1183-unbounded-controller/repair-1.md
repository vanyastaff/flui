# Repair round 1 — #1183 (worktree /mnt/data/dev/flui-worktrees/anim-unbounded, branch fix/1183-unbounded-controller-refuses-bound-targets)

Three reviewers at 0a85b933: rust-reviewer (5a FAIL on four items, 5b NEEDS WORK; mutation probes a–g run), glm-5.3
outside lens (5a PASS minus one item, 5b NEEDS WORK, all minor), api-design-lead (API-GATE PASS WITH FIXES). Fold ALL of
it into ONE commit; rerun the crate gates + `just ci`; report tails, red evidence for A–C, and disagreements in writing.

## A. Code — correctness (should-fix)
1. **Finite endpoints do not make a finite RANGE.** `without_ticker_bounds(100 ms, -f32::MAX, f32::MAX)` is accepted, its
   span is `inf`, `forward()` is `Ok`, and `tick_at(0.05)` publishes `value = inf` (probe verified). Add the rule at the
   constructor — `with_bounds_inner` and `builder.rs::bounds` reject `!(upper - lower).is_finite()` with `InvalidBounds`
   ("the range must fit in f32") — one rule closing every run start on a bounded controller; keep `drive_to`'s span check
   (still reachable on an unbounded controller). RED test: `(-f32::MAX, f32::MAX)` rejected in both places.
2. **`simulation.x(0.0)` is caller-supplied code and runs under the `inner` guard in `drive_simulation`.** Evaluate
   `initial` BEFORE `self.inner.lock()` (keep `check_disposed` first for error precedence — lock, check, unlock, evaluate,
   re-lock is not needed: evaluate first, then lock and check disposed; state the precedence you chose).
3. Extract `end_simulation_run(inner, value_change)` shared by `tick_simulation`'s `is_done` end and its non-finite-sample
   end (today two blocks differing by one literal).
4. `warn_non_finite_value`'s text says "canonicalized or ignored"; the simulation path ENDS the run — pass the outcome
   (or split the message).

## B. Tests (gaps the probes exposed)
1. Probe (a) — moving `drive_to`'s `NonFiniteTarget` check after `clear_run_modes()` — SURVIVES: the refusal battery
   `unbounded_refusals_leave_the_controller_completely_untouched` has no `animate_to`/`animate_back` attempt and its live
   run (`animate_to(200.0, None)`) carries nothing `clear_run_modes` clears. Add `animate_to(NaN)`, `animate_to(∞)`,
   `animate_back(NaN)` attempts, make the live run one with a clearable mode (`animate_to_curved(.., Some(d), curve)` or
   `animate_with(sim)`), then tick both the controller and its untouched twin once and compare values. Show the mutation
   reddening it.
2. The warn latch: add a `#[cfg(test)]` accessor for `non_finite_warned` beside the existing test-only listener-count
   accessors and pin "three non-finite `set_value` calls → latched after the first"; a hand-rolled `tracing::Subscriber`
   (`flui-scheduler/src/ticker/future_tests.rs` does it with `with_default`) is the alternative if you prefer to count
   emissions; pin the per-call `NonFiniteTarget` warn the same way if cheap, else say it is unpinned.
3. Event pins: `unbounded_first_stop_on_a_fresh_controller_reports_completed` and the scroll_controller sibling assert
   `status()` only — add a COUNTING status listener asserting exactly one `Completed` emission.
4. `reverse_from(Some(-∞))` on a bounded controller clamps to `lower` — add the mirror case.
5. `builder.rs::bounds_rejects_non_finite_endpoints` covers 3 of the controller test's 6 cases (+ the new range case) —
   use the same list.
6. Rename the two inverted oracles: `…_rejects_invalid_bounds_and_accepts_wide_open_ones` → `…_rejects_wide_open_ones`
   (their bodies now reject wide-open bounds).

## C. Docs — false statements (must-fix) and declarations
1. "today `repeat_with(Some(f32::NAN), ..)` installs a NaN run" (controller.rs ~L1259 comment, ~L3353 test doc) is FALSE —
   on main it PANICS inside `f32::clamp` (`min > max, or either was NaN`); CHANGELOG/ARCHITECTURE say panic. Fix both texts.
2. ARCHITECTURE.md: `_internalSetValue` cited at `:759-765` (that is `fling`'s doc; it is ~L410-421) — cite the SYMBOL;
   "Flutter's constructor accepts any pair, including … inverted ones" is false (the default ctor has
   `assert(upperBound >= lowerBound)`: accepts non-finite and equal, rejects inverted in debug) — fix here and in
   CHANGELOG; "`0.0` is the value every other non-finite-input path canonicalizes toward" is false (bounded
   `set_value(NaN)` → lower bound; unbounded → no-op) — fix; add the missing cost (2) "status at 0.0 is path-dependent
   (`reset()` → `Dismissed`; construction/`set_value(0.0)` → `Forward`)".
3. CHANGELOG: "a running simulation's sample keeps its existing NaN canonicalization on a BOUNDED controller (pinned)" is
   false — main passed NaN through `clamp` unchanged and the new behavior ends the run on ANY controller; state that.
   Put the `with_bounds*`/builder bound-rejection under the existing `### Breaking` heading (semver tooling cannot see
   it; the heading is the only mechanized signal), keep the rest under `### Changed`; split the ~45-line paragraph into
   per-topic paragraphs (one em-dash per paragraph).
4. The three PUBLIC `*_bounds` constructors' `# Errors` still read "if `lower_bound >= upper_bound`" — copy the finite/
   range rule from `with_bounds_inner`'s doc into all three; `AnimationError::InvalidBounds`'s variant doc names the finite
   rule, the builder, and its `repeat_with` range use (to the standard of the `NonFiniteTarget` doc beside it).
5. `canonicalize_value_target`'s `NonFiniteTarget` messages never name the method or parameter (every other site does);
   thread caller identity in (the callers already know it) — production reads only the warn text.
6. Stale facts: `is_at_upper_bound`/`is_at_lower_bound` docs ("when the fling controller is created with
   `(NEG_INFINITY, INFINITY)` bounds") and `scrollable.rs` ~L504-505 "(built via `without_ticker_bounds`)" → the
   `unbounded*` constructors.
7. Process markers: "Decision 2", "Decision 4/v3 delta 1", "v3 delta 6's …", "the plan's own repro" in production comments
   (~L1292, 1561, 1735) and ~17 test docs/assert strings (~L3018-3525), scroll_controller.rs ~L1110 — replace each with the
   invariant in plain English (AGENTS.md: planning-artifact citations are banned in shipped code). Same family:
   ~L1455 "(fixing the pre-existing bug …)" history; error.rs ~L76 "every production caller … discards the `Result`
   today" → state the invariant ("every refusal is also emitted as a `tracing::warn!`"), keep the why in ARCHITECTURE.md.
8. Mapping entry prose: several paragraphs exceed one em-dash — split.

## D. Structure (declinable but recommended)
`controller.rs` is 5.7k lines, 56 % tests. Move `mod tests` to a sibling file via `#[cfg(test)] #[path = "controller_tests.rs"] mod tests;`
(the source-guard test's `include_str!("controller.rs")` keeps working and its production/tests split becomes moot). If you
decline, say why in one line.

## Gates (paste tails)
`cargo nextest run -p flui-animation -p flui-widgets -p flui-material`, `cargo clippy -p flui-animation -p flui-widgets --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, `RUSTDOCFLAGS="-D warnings" cargo doc -p flui-animation --no-deps --document-private-items`, `typos`,
`cargo check -p flui --examples`, `bash scripts/port-check.sh`, `bash scripts/check-panic-policy.sh`, then `just ci` once.
Note origin/main advanced by one unrelated commit (#1184, semantics/material) — do NOT rebase; the PR merges via squash.

## Report back
Diff summary; red evidence for A1, B1 (the mutation reddening), B2–B4; gate tails; disagreements in writing; `MEMORY:` lines.
