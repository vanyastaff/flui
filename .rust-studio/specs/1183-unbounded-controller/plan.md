# #1183 — unboundedness is a constructor fact; bound-targeting runs on it are refused; no path reads NaN — plan v2

(v1 was reshaped by harsh-critic and api-design-lead: NaN bounds were accepted and v1 would have panicked on
them; the "finite endpoints" invariant v1 asserted was not established; refusals reached production only as a
silent no-op because the fling controller's callers discard the `Result`; half-open bounds made the rule
incidental complexity nobody uses. v2 takes the critic's ALT-1.)

Worktree: /mnt/data/dev/flui-worktrees/anim-unbounded (branch fix/1183-unbounded-controller-refuses-bound-targets,
from origin/main 864b5798). Issue: `gh issue view 1183`; flutter/flutter#76014 (open — `unbounded` behavior
"simply not defined" for methods other than `animateWith`). Reference at 3.44.0: `AnimationController.unbounded`
(fixed ±∞, `value = 0.0`, `_internalSetValue` → status `forward` at 0.0), `_internalSetValue`,
`_animateToInternal`, `fling`, `_InterpolationSimulation.x` (returns `_begin`/`_end` exactly at the endpoints).

## Facts (verified by the critic with a `rustc -O` scratch)
- `lower >= upper` accepts NaN bounds (NaN compares false): `with_bounds(NaN, 1.0)` is `Ok` today with `value = NaN`;
  `f32::clamp` PANICS on a NaN bound even in release; `∞ * 0.0 = NaN`; `(-∞).clamp(-∞, 5) = -∞`.
- `forward_from(Some(NaN))`/`animate_to(NaN)` pass NaN through `clamp` on ANY controller (bounded too).
- `fling_with` writes `inner.direction` BEFORE its `InvalidSpring` refusal — a refused fling already corrupts
  direction (observable via a later `stop()`).
- The three production unbounded sites (`scrollable.rs`, `scroll_controller.rs`, `refresh_indicator.rs`) all spell
  `without_ticker_bounds(1 ms, -∞, ∞).expect(..)`; no workspace site uses a half-open pair; their run starts
  discard the `Result` (`let _ = fling.animate_to_curved(..)`, `let _ = fc_fling.animate_with(sim)`).
- `scroll_controller.rs` sets `set_is_scrolling(true)` BEFORE `fling.animate_to_curved(..)` and ignores `Err`.
- Two existing tests assert `value() == NEG_INFINITY` for a wide-open controller
  (`without_ticker_bounds_rejects_invalid_bounds_and_accepts_wide_open_ones`, its detached twin) — deliberately
  inverted oracles under this plan, not tests to "fix".

## Decisions
1. **Constructors.** `AnimationController::unbounded(duration, &scheduler)`, `unbounded_without_ticker(duration)`,
   `unbounded_with_detached_ticker(duration)` → `Self` (infallible): bounds fixed at `(-∞, ∞)`, initial
   `value = 0.0`, `start_value = target_value = 0.0` (today ±∞ leak into `velocity()`), initial status per
   Flutter's `_internalSetValue` rule = `settled_status_keep_direction()` of the initial value (`Forward` at
   0.0 — the same rule `set_value` applies; bounded controllers at `lower` stay `Dismissed`, unchanged).
   `with_bounds`/`without_ticker_bounds`/`with_detached_ticker_bounds` (and `builder.rs`'s bounds) REJECT any
   bound that is not finite, and NaN, with `InvalidBounds` (`!(lower < upper) || !lower.is_finite() ||
   !upper.is_finite()`): bounded means finite; unbounded is the constructor above, not a bound value. The three
   production sites migrate to `unbounded_without_ticker` (their `.expect` goes away).
2. **Refusals on an unbounded controller** (derived: `!lower_bound.is_finite()`, which only the `unbounded*`
   constructors can produce), new `AnimationError::NonFiniteTarget(String)` (`#[non_exhaustive]` enum, additive):
   `forward`/`forward_from`, `reverse`/`reverse_from`, `fling`/`fling_with`, and `repeat`/`repeat_with` whose
   EFFECTIVE range is non-finite (`repeat_with(Some(0.0), Some(1.0), ..)` on an unbounded controller WORKS).
   `reset()` on an unbounded controller resets to 0.0 (the defined beginning — flutter#76014's reporter asks for
   exactly this; `reset()` never fails for non-finiteness).
   **On ANY controller**: `animate_to*`/`animate_back*` refuse a non-finite `target` (NaN/±∞) and
   `forward_from`/`reverse_from` refuse a non-finite `from` — the same variant; a declared change for bounded
   controllers too (today NaN passes `clamp`).
   Every refusal runs BEFORE any state mutation (before `clear_run_modes`, before `from` is applied, before
   `fling_with` touches `direction` — compute the direction into a local and assign after the checks; fix the
   pre-existing `InvalidSpring` ordering the same way). A refused call leaves value/status/direction/run
   generation/the live run untouched and fires nothing. Because the production callers discard the `Result`,
   the controller emits `tracing::warn!` itself for `NonFiniteTarget` — after the guard drops, like
   `warn_if_no_ticker`.
   Why refuse, not repair: there is no finite value to repair toward; a silent no-op hides the caller bug the
   way Flutter's ∞ jump does; every one of these methods already returns `Result` (the repair-not-reject rule
   is about widget constructors inside `build`, which cannot propagate a `Result`).
3. **`set_value(non-finite)`** stays infallible: NaN → the lower bound on a bounded controller (today's pin),
   UNCHANGED on an unbounded one (keep the last real position — a poisoned drag must not snap the list to 0);
   ±∞ → the bound when that bound is finite (today's clamp), else unchanged. One `tracing::warn!` after the
   unlock, LATCHED (`non_finite_warned: bool` in the inner state) — a NaN-producing drag warns once, not every
   frame. Cost on the hot path: one `is_finite()`.
4. **No path reads NaN** (the issue's criterion): `tick_time_based` adopts Flutter's endpoint special-case
   structurally — `t == 0` → `start_value`, `t >= 1` → `target_value`, interior → lerp — so `∞ * 0.0` cannot
   occur (no `debug_assert`); `tick_simulation`/`drive_simulation`: a non-finite `sim.x(t)` sample leaves the
   value unchanged with the same latched warn (a user simulation emitting NaN must not poison the controller).
5. **Consumer**: `scroll_controller.rs`'s activity flag — set `is_scrolling(true)` only after the run start
   returned `Ok`, else leave it clear (a refused start must not park the scrollable in "scrolling" forever);
   `refresh_indicator`'s `animate_with(sim)` is unaffected (sims are not refused).
6. **Docs**: constructor docs (the initial-value/status rule, what an unbounded controller may be driven with:
   `animate_to`/`animate_back` with finite targets, `animate_with`, `set_value`, `repeat_with` with a finite
   range); each refusing method's `# Errors` gains `NonFiniteTarget`; `## Mapping decisions` entry "Unbounded is a
   constructor fact; bound-targeting runs on it are refused; no path reads NaN" recording: Flutter's `unbounded`
   (fixed ±∞, value 0.0, status `forward`) = parity; the refusals = FLUI's rule where Flutter is undefined
   (flutter#76014); bounded constructors rejecting non-finite bounds = FLUI's rule (Flutter accepts anything);
   `reset()` → 0.0 on unbounded (Flutter → −∞); the NaN canonicalizations; why `repeat_with`'s non-finite range
   is `InvalidBounds` (a range-shape error) while the rest is `NonFiniteTarget`. CHANGELOG (crate): `### Added`
   (constructors, variant), `### Changed` (bounded constructors reject non-finite; non-finite `target`/`from`
   refused on any controller; `set_value`/simulation NaN canonicalization; tick endpoints exact); the two
   inverted tests named.

## Tests (RED first unless marked pin)
- `unbounded_without_ticker(d)`: `value() == 0.0`, `status() == Forward`, `velocity() == 0.0`, `is_animating()`
  false; `set_value(pixels)` fires no status change (still `Forward`).
- `with_bounds*(NaN, 1)`, `(0, NaN)`, `(-∞, ∞)`, `(-∞, 5)`, `(5, ∞)`, `(-∞, -∞)` → `InvalidBounds`;
  `(-1, 3)` still `Ok` with value −1 (pin); `builder.rs` bounds likewise.
- On unbounded: `forward()`, `forward_from(Some(0.5))`, `reverse()`, `fling(1.0)`, `fling(-1.0)`, `repeat(false)`,
  `repeat_with(None, Some(1.0), ..)` → `NonFiniteTarget`; afterwards value/status/direction (probe via `stop()`'s
  reported status on a fresh twin), `run_generation`, `is_animating` unchanged, a value-listener AND a
  status-listener counter both read 0, and a live `animate_to(200.0)` run's future is still pending.
- On unbounded: `animate_to(200.0)` runs (pin, the scroll oracle), `animate_with(sim)` runs (pin),
  `repeat_with(Some(0.0), Some(1.0), ..)` runs; `reset()` → value 0.0, status `Dismissed` (its documented
  contract "value = the beginning, status `Dismissed`" — on an unbounded controller the beginning is 0.0).
- On a BOUNDED controller: `animate_to(f32::NAN)`, `animate_to(f32::INFINITY)`, `forward_from(Some(NaN))`,
  `reverse_from(Some(-∞))` → `NonFiniteTarget` (declared change), nothing mutated.
- `set_value`: bounded NaN → lower (pin), bounded ∞ → upper (pin); unbounded NaN/∞ → unchanged; the warn fires
  once across three NaN calls (latch).
- `tick_time_based`: a run whose `t` is exactly 0 reads `start_value` exactly (pin, with `(-1, 3)` bounds and a
  curve that is not identity at the endpoints — the existing `_InterpolationSimulation` parity).
- Simulation: a scratch `Simulation` whose `x()` returns NaN once → value unchanged, one warn, run continues.
- `fling_with` refused for `InvalidSpring` leaves `direction` untouched (red today: `stop()` after a refused
  `fling(-1.0)` with an underdamped spring reports `Dismissed` where an untouched controller reports `Completed`).
- Inverted oracles: rewrite the two `…accepts_wide_open_ones` tests to "rejects wide-open; `unbounded_without_ticker`
  starts at 0" and say why in their docs.
- flui-widgets: the scroll suites (`tests/scroll.rs`, parity `scrollable_test.rs`, `refresh_indicator` tests) are
  the production oracle — run them; the `is_scrolling` ordering gets its own test: a `ScrollController::animate_to`
  with a NaN target leaves `is_scrolling()` false.

## Gates
`cargo nextest run -p flui-animation -p flui-widgets -p flui-material`, clippy/fmt/doc as usual, `cargo check -p flui --examples`,
`just ci` once. Review: rust-reviewer (5a+5b) + one outside lens (glm-5.3 — a new public error variant and
constructor family) + api-design-lead gate.

## v3 deltas (harsh-critic's recheck of v2 — ACCEPTABLE with 1–2 folded; all eight are part of the contract)
1. A simulation must not become an immortal run. `fling_with` refuses a non-finite `velocity` (NaN never
   compares, so today it builds a NaN spring whose `is_done` never fires); `drive_simulation` refuses a
   non-finite `sim.x(0.0)` — both before any mutation, `NonFiniteTarget`, controller-side warn. A MID-RUN
   non-finite sample (`tick_simulation`) ENDS the run at the last finite value: settled status by direction,
   `active_run` completed (the future resolves `Ok`), ticker stopped, latched warn — never "value unchanged,
   run continues" (that leaves `active_run` `Some`, `Vsync` ticking forever and a scrollable's `is_scrolling`
   stuck). Tests: `fling(f32::NAN)` refused; a sim whose `x` is NaN from t = 0 refused; a sim that turns NaN
   mid-run ends the run (status settled, future complete, `has_running()` false afterwards).
2. ONE rule for every value entry point (`target` of `animate_to*`/`animate_back*`, `from` of
   `forward_from`/`reverse_from`, `set_value`): NaN → refuse (`set_value`: canonicalize per Decision 3, it is
   infallible); ±∞ → CLAMP when the bound it points at is finite (today's working "go to the end" idiom on a
   bounded controller stays), REFUSE when that bound is infinite. So on a bounded controller `animate_to(∞)`
   and `forward_from(Some(∞))` clamp (pins), `animate_to(NaN)` is refused; on an unbounded one all three are
   refused.
3. `repeat_with`'s effective-range check is any-controller: `!(lo < hi) || !lo.is_finite() || !hi.is_finite()`
   → `InvalidBounds` (today `repeat_with(Some(NaN), ..)` on a bounded controller installs a NaN run). Test it.
4. Span overflow: refuse at run start when `(target - start)` is not finite (`set_value(-f32::MAX)` then
   `animate_to(f32::MAX)` gives `range = ∞`, and an `Interval`-style curve's interior `eased_t == 0.0` would
   still make NaN) — `NonFiniteTarget("… span overflows f32")`. Test it.
5. `last_reported_status` is initialized to the INITIAL status (not hard-coded `Dismissed`), or the first
   `finish` fires a spurious `Forward`. The "`set_value(pixels)` fires no status change" test uses a COUNTING
   status listener (status equality is vacuous under dedup).
6. Recorded choice: `Forward` at construction on an unbounded controller is `_internalSetValue` parity and is
   safe because `Vsync`/`tick_at` gate on `active_run`, not status. Costs to record in the mapping entry: a
   never-run unbounded controller reports `is_running()`; status at 0.0 is path-dependent (`reset()` →
   `Dismissed`, construction/`set_value(0.0)` → `Forward`) by the keep-direction rule; the first `stop()` on a
   fresh unbounded controller emits `Forward → Completed` — pin that event (every `jump_to` on a fresh
   `Scrollable` triggers it through the stop hook).
7. `set_value(non-finite)` on an unbounded controller is a FULL no-op (no `stop_running`, no notification) plus
   the latched warn — nothing changed, and a NaN from a bad computation must not kill a live fling.
8. Nits: `InvalidBounds`' rustdoc names the finite rule; `builder.rs`'s duplicate bounds check adopts it (RED
   test); the `NonFiniteTarget` warn is per call (a per-command refusal), the `set_value`/simulation warn is
   latched — say both.
