### Removed

- Remove the unused standalone `ChainedTween` wrapper from `flui-animation`.
  Compose a curve and value mapping through `Curve::transform` followed by
  `Animatable::transform`, or use `CurvedAnimation` with `TweenAnimation` when
  observing a live animation. This intentionally changes the pre-1.0 public API.

### Changed

- `DrivenController::is_bound` reads its live registry seat instead of only
  inspecting stored ownership. It is no longer usable in a `const` context.
