# Changelog

All notable changes to `flui-animation` are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: per `docs/release.md` policy.

## [Unreleased]

### Added

- `AnimationController::unbounded`/`unbounded_without_ticker`/
  `unbounded_with_detached_ticker` (issue #1183) — unboundedness is now a
  constructor fact (fixed `(f32::NEG_INFINITY, f32::INFINITY)` bounds,
  initial `value = 0.0`, initial status `Forward`), not a bound value passed
  to `with_bounds`/`without_ticker_bounds`/`with_detached_ticker_bounds`,
  which now reject a wide-open (or half-open) pair — see `### Changed` below
  and `crates/flui-animation/docs/ARCHITECTURE.md`'s "Unbounded is a
  constructor fact" mapping entry.
- `AnimationError::NonFiniteTarget(String)` — new additive
  (`#[non_exhaustive]`) variant returned by every run-starting method when a
  `target`/`from`/`velocity`/simulation sample is non-finite, or when it
  targets a bound this controller does not have (unbounded).
- `smoothing` module — frame-rate-independent followers Flutter does not ship:
  `exp_decay` / `exp_decay_half_life` / `Smoothed` (Holmér exponential decay,
  half-life parameterization) and `SmoothDamp` (critically damped spring
  approximation with max-speed clamp and overshoot guard, Game Programming
  Gems 4 ch. 1.10).
- `OklabColorTween` — perceptually uniform color interpolation through Oklab
  (Björn Ottosson 2020). Flutter's `Color.lerp` averages gamma-encoded sRGB
  channels, so cross-hue transitions pass through dark gray midpoints.
  Backed by `Color::to_oklab` / `from_oklab` / `lerp_oklab` in `flui-types`.
- Curve catalog parity + extensions: the full Penner cubic set (`Ease`,
  `EaseIn/Out/InOut{Quad,Cubic,Quart,Quint}`, `FastLinearToSlowEaseIn`,
  `LinearToEaseOut`, `EaseInToLinear`, `SlowMiddle`), `ThreePointCubic` with
  the Material 3 `EaseInOutCubicEmphasized` and `FastEaseInToSlowEaseOut`
  constants, and the `Split` curve (track-finger-then-fling transitions).

### Breaking (issue #556: `flui-scheduler`'s `UpdateScheduler` reshape; issue #1183 — bounded constructors reject non-finite bounds)

- This crate's re-export of `flui_scheduler::Scheduler` is renamed
  `UpdateScheduler` (hard rename, no alias, matching the rename in
  `flui-scheduler` itself).
- `VsyncCallback` and `VsyncScheduler` are no longer re-exported here —
  `flui-scheduler` deleted that fixed-rate vsync simulator outright (zero
  production consumers; not to be confused with this crate's own,
  unrelated `Vsync` per-presentation tick registry, which is unaffected).
- `with_bounds`/`without_ticker_bounds`/`with_detached_ticker_bounds` (and
  `AnimationControllerBuilder::bounds`) now reject `InvalidBounds` for a
  `NaN` endpoint, an infinite endpoint, a half-open pair (one finite bound,
  one infinite), or two finite endpoints whose RANGE overflows `f32`
  (`(-f32::MAX, f32::MAX)`). Flutter's own constructor is far more
  permissive: it asserts only `upperBound >= lowerBound` (`debug`-only,
  compiled out in `release`), which accepts an infinite pair and an equal
  pair. Since the assert is gone in `release`, it silently accepts an
  inverted pair there too. `cargo public-api`/`cargo semver-checks` cannot
  see a signature-preserving `Ok` -> `Err` narrowing like this, so
  `### Breaking` is the only mechanized signal that a previously-accepted
  call now returns `Err`.

### Changed

- **Unbounded is a constructor fact; bound-targeting runs on it are
  refused; no path reads NaN** (issue #1183, flutter/flutter#76014). The
  bounded-constructor rejection itself is listed under `### Breaking`
  above; this entry covers the rest of the change.
- A wide-open `(NEG_INFINITY, INFINITY)` pair, which `scrollable.rs`/
  `scroll_controller.rs`/`refresh_indicator.rs` all used to spell as
  `without_ticker_bounds(1ms, NEG_INFINITY, INFINITY).expect(..)`, now
  migrates to `unbounded_without_ticker(1ms)` instead (see `### Added`).
- On an unbounded controller, `forward`/`forward_from`/`reverse`/
  `reverse_from`/`fling`/`fling_with`, and `repeat`/`repeat_with` whose
  effective range is still non-finite, now return `Err(NonFiniteTarget)`
  instead of installing a run toward an infinite value.
  `repeat_with(Some(finite), Some(finite), ..)` is unaffected.
- On ANY controller (bounded too), `animate_to`/`animate_back`(`_curved`)/
  `forward_from`/`reverse_from` now refuse a `NaN` `target`/`from` instead
  of letting it through `clamp` unchanged. A `+-inf` `target`/`from` still
  clamps to a finite bound exactly as before; it is refused only when that
  bound is itself infinite.
- `repeat_with`'s range-inversion check now also catches a `NaN` endpoint:
  `repeat_with(Some(f32::NAN), ..)` used to reach `f32::clamp`'s own
  `assert!(min <= max)` and PANIC, not merely install a broken run.
- `fling`/`fling_with` refuse a non-finite `velocity`, and no longer
  corrupt `direction` on a refused fling — a pre-existing ordering bug
  where `direction` was written before the `InvalidSpring` check.
- `animate_to`/`animate_back` refuse when `target - value` overflows
  `f32`.
- `set_value`'s existing `NaN` canonicalization on a BOUNDED controller is
  unchanged (pinned). On an UNBOUNDED one, a non-finite `set_value` input
  is now a full no-op — was: canonicalized against `-inf`/`+inf`, exactly
  the bug this issue reports.
- A running simulation's sample going non-finite MID-RUN now ends the run
  at its last finite value, on ANY controller. This is new behavior, not a
  preserved pin: previously the sample had no non-finite handling at all
  and reached `clamp` unchanged, which returns `NaN` as-is (a `NaN` bound
  panics `clamp`; a `NaN` self does not), silently poisoning `value`.
- `reset()` on an unbounded controller now lands on `0.0`
  (flutter#76014's requested defined beginning), not `-inf`.
- `tick_time_based` now reads `start_value`/`target_value` directly at the
  exact endpoints instead of computing `start + range * eased_t` there, so
  `range` being non-finite can no longer produce `inf * 0.0 = NaN`.
- `scroll_controller.rs`'s `service_pending_command` now raises
  `is_scrolling(true)` only after the run start returns `Ok` AND is still
  running, instead of unconditionally before the call, so a refused (or
  synchronously-settled) start no longer parks the scrollable in
  "scrolling" forever.
- Two tests inverted: `…_bounds_rejects_wide_open_ones` (both the
  `without_ticker` and `with_detached_ticker` variants) used to assert a
  wide-open pair was ACCEPTED at `value() == NEG_INFINITY`. They now
  assert it is REJECTED, and that the unbounded constructor starts at
  `0.0`.
- **Repeat sampling is a pure function of elapsed time** (#1078):
  `AnimationController::repeat`/`repeat_with`'s `value`/`status`/`direction` at any `tick_at` are
  now computed from the elapsed time since the run started, the range, period, `reverse`, and
  `count` alone, in integer nanoseconds — the frame partition no longer changes the answer
  (`tick_at(1.25)` now equals `tick_at(1.0); tick_at(1.25)`, for any number of cycles a long
  frame spans). Behavior changes: a repeat now starts from the CURRENT value clamped into
  `[min, max]`, not from `min` (Flutter parity — a `repeat()` issued fresh on every build
  progresses instead of snapping back); `period` is resolved ONCE at the call
  (`period.unwrap_or(duration)`), so a later `set_duration` no longer retimes an active repeat
  and both legs of a bounce always share one period; a finite repeat's exhaustion now lands on
  its last cycle's own endpoint with that leg's settled status, instead of Flutter's `% 1.0`
  wrap (an intentional divergence — see `crates/flui-animation/docs/ARCHITECTURE.md`'s "Repeat
  sampling" mapping entry); a zero effective period, or an explicit `count: Some(0)`, now settle
  synchronously at the call instead of ticking a run that could never advance; and a leftover
  `animate_to_curved` easing curve no longer leaks into a following repeat, which always
  interpolates linearly. `cargo public-api`/`cargo semver-checks` report no diff against the
  pre-change baseline — every change here is behavior-only, no signature moved.
- `forward()`/`reverse()`/`forward_from`/`reverse_from` and
  `animate_to`/`animate_back` with no explicit duration now scale the run's
  duration by the remaining fraction of the range (Flutter parity:
  `AnimationController._animateToInternal`). A mid-flight `reverse()` keeps
  the full-range velocity instead of stretching the leftover distance over
  the entire duration. Starting a run whose value already sits at its target
  settles immediately with the final status (`Completed`/`Dismissed`) instead
  of running a full-duration no-op that re-notified listeners every frame.
- `AnimationController::is_animating()` is now ticker-based (Flutter parity:
  `AnimationController` overrides `isAnimating` with `ticker.isActive`).
  Previously a stopped controller positioned at an interior value reported
  itself as animating.
- `TweenSequence::transform` documents its clamping semantics (saturates
  overshoot, unlike plain `Tween` extrapolation).
- `Vsync`'s registry is now a `BTreeMap` keyed by registration id, walked by
  a cursor bounded at the id count captured on entry, instead of a per-frame
  id snapshot resolved through a linear `Vec` scan: `tick_all` drops from
  O(N²) to O(N log N) per pump, and `register`/`unregister`/lookup of one
  controller drop from O(N) to O(log N) — the cost a consumer with a large
  resident registry (many implicitly-animated widgets sharing one `Vsync`)
  will actually notice. Measured before/after in
  [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md#vsync-registry-indexing-1060)
  (Refs #1060).
- Zero-duration runs now settle SYNCHRONOUSLY, before the call returns,
  instead of completing on the first driven frame (issue #1171; Flutter
  parity, `_animateToInternal`'s `simulationDuration == Duration.zero`
  branch): `forward()`/`reverse()`/`forward_from`/`reverse_from`/`animate_to`/
  `animate_back` (and their `_curved` variants) whose base duration, per-run
  override, or distance resolves to zero now snap the value, fire value and
  status listeners once inline, and return an already-complete `TickerFuture`
  — the displaced run's own `TickerFuture` still cancels, but only after the
  new status is observable, and `run_generation` is left untouched (no
  ticker was ever installed).
- `animate_to`/`animate_to_curved` now always run direction `Forward`, and
  `animate_back`/`animate_back_curved` always run `Reverse`, regardless of
  whether `target` is above or below the current value (previously derived
  from travel, an unrecorded divergence from Flutter's own documented
  contract). Two production-visible consequences: the unbounded scroll
  controller's ballistic `animate_to` toward a SMALLER pixel value now
  reports `Forward`/`Completed` instead of `Reverse`/`Dismissed` (its only
  consumer treats `Completed`/`Dismissed` identically, so this changes
  nothing observable there); and `flui-cupertino`'s `CupertinoButton` release
  fade, which also calls `animate_to_curved` (kept for oracle parity), now
  reports `Completed` at both the press-in and release ends. That second
  consumer is fixed in the same change: the release used to chain off a
  status listener watching `Completed`, which could no longer tell "the
  press landed" from "the release landed" and re-triggered itself; it now
  chains on the press fade's own `TickerFuture` instead (`Ok`-only,
  one-shot).
- A run's end status is now its direction's settled status with no bound
  check, so `animate_to(lower_bound)` from mid-range ends `Completed`, not
  `Dismissed` (Flutter's `_tick` rule). The same applies through
  `tick_simulation`: `animate_with(sim)` landing on a bounded controller's
  lower bound now also ends `Completed`, not `Dismissed` — a `fling`'s own
  end is unaffected, since `fling`/`fling_with` pick direction from the sign
  of `velocity`, never from where the simulation lands. `stop()`/`set_value`
  are unchanged: both still report the bound actually reached, falling back
  to direction only for a non-bound stop.
- A settle whose value does not actually move (e.g. `forward_from(Some(x))`
  landing on the value it already held) no longer fires a spurious value
  notification. The same entry-value rule now also applies to a REAL
  (non-settling) run that still applies `from`: it notifies iff `from`
  actually moved the value, narrower than Flutter's `forward`/`reverse`
  (whose `value=` setter always notifies).
- `Vsync::has_running`/`tick_all` now skip a controller `dispose()`d mid-run
  instead of ticking it forever: `dispose()` still leaves `status` untouched
  (unchanged, and still Flutter parity), so the two consumers instead read a
  disposed flag folded into the walk's per-controller probe.
- Fixed: a `Vsync`-driven controller's `set_value` mid-run no longer gets
  silently overwritten by the next `tick_all`. `Vsync`/`tick_at` used to
  treat `status().is_running()` as "a run is installed", but `set_value` at
  an interior value reports a directional running status (Flutter parity)
  even though it already stopped the run and cleared `active_run` — the
  walk kept ticking it from the stale, already-stopped run's own
  `start_value`/`target_value`. Both now read `active_run.is_some()`
  instead.

### Fixed

- `Vsync::tick_all` re-reads `muted` on every registration it visits instead
  of only once at entry: a listener that calls `set_muted(true)` mid-walk
  now stops the rest of that frame's controllers from ticking, instead of
  letting the in-flight walk finish against stale state (Refs #1060).
- `TweenAnimation` never subscribed to its parent, so listeners on any tween
  combinator silently never fired (`AnimatedBuilder`-class breakage).
- `ProxyAnimation` status listeners were orphaned on the old parent after
  `set_parent` and removal targeted the wrong parent; the proxy now owns its
  status registry behind a migrating forwarder and fires on swap only when
  the status actually changes.
- `AnimationSwitch` (train-hop) left stale listener ids after a switch and
  `dispose()` cleaned the wrong animation; listeners (value and status) are
  rebound on every hop, and public status listeners are switch-owned with
  stable ids.
- `CurvedAnimation` locks the active curve to the run-entry direction
  (Flutter `_curveDirection`), so `reverse()` mid-run no longer swaps curves
  underneath the value; the capture is gated on a live run so a stopped
  controller's interior `set_value` cannot pin the direction.
- NaN canonicalization: Rust's `clamp` propagates NaN through the cubic
  solver and the controller value; canonicalized at `Cubic` /
  `ThreePointCubic` / `Split::transform` and `AnimationController::set_value`
  (warn + lower bound).
- `FrictionSimulation::through` rejects the zero-travel degenerate case with
  the physical constraint instead of an opaque downstream drag panic.
- `ThreePointCubic::new` asserts the midpoint lies strictly inside the unit
  square (divisor safety); `const` constructions turn violations into
  compile errors.
