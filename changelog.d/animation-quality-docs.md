### Added

- **`Eq` on float-free curve and tween types** (`flui-animation`): `SawTooth`, `FlippedCurve`,
  `ReverseCurve`, `Tween`, `ConstantTween`, `ReverseTween`, `CurveTween`, `ChainedTween` and
  `OklabColorTween` derive `Eq` when their parameters do.

### Fixed

- **`flui-animation` prose docs**: every example in `README.md`, `docs/GUIDE.md` and
  `docs/PERFORMANCE.md` now compiles as a doctest against the current API (argument orders,
  `Option` parameters, error variants and `f64` types were stale), and `PERFORMANCE.md` lists
  only benchmarked figures.
