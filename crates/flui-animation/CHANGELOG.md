# Changelog

All notable changes to `flui-animation` are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: per `docs/release.md` policy.

## [Unreleased]

### Added

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

### Breaking (issue #556 — `flui-scheduler`'s `UpdateScheduler` reshape)

- This crate's re-export of `flui_scheduler::Scheduler` is renamed
  `UpdateScheduler` (hard rename, no alias, matching the rename in
  `flui-scheduler` itself).
- `VsyncCallback` and `VsyncScheduler` are no longer re-exported here —
  `flui-scheduler` deleted that fixed-rate vsync simulator outright (zero
  production consumers; not to be confused with this crate's own,
  unrelated `Vsync` per-presentation tick registry, which is unaffected).

### Changed

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
  contract). The one production-visible consequence: the unbounded scroll
  controller's ballistic `animate_to` toward a SMALLER pixel value now
  reports `Forward`/`Completed` instead of `Reverse`/`Dismissed` (its only
  consumer treats `Completed`/`Dismissed` identically, so this changes
  nothing observable there).
- A run's end status is now its direction's settled status with no bound
  check, so `animate_to(lower_bound)` from mid-range ends `Completed`, not
  `Dismissed` (Flutter's `_tick` rule). `stop()`/`set_value` are unchanged:
  both still report the bound actually reached, falling back to direction
  only for a non-bound stop.
- A settle whose value does not actually move (e.g. `forward_from(Some(x))`
  landing on the value it already held) no longer fires a spurious value
  notification.
- `Vsync::has_running`/`tick_all` now skip a controller `dispose()`d mid-run
  instead of ticking it forever: `dispose()` still leaves `status` untouched
  (unchanged, and still Flutter parity), so the two consumers instead read a
  disposed flag folded into the walk's per-controller probe.

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
