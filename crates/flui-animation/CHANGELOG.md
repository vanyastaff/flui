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

### Changed

- `AnimationController::is_animating()` is now ticker-based (Flutter parity:
  `AnimationController` overrides `isAnimating` with `ticker.isActive`).
  Previously a stopped controller positioned at an interior value reported
  itself as animating.
- `TweenSequence::transform` documents its clamping semantics (saturates
  overshoot, unlike plain `Tween` extrapolation).

### Fixed

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
