### Added

- **`Eq` on float-free curve and tween types** (`flui-animation`): `FlippedCurve`, `Tween`,
  `ConstantTween`, `ReverseTween`, `CurveTween` and `ChainedTween` derive `Eq` when their parameters do.

### Fixed

- **`flui-animation` prose docs**: every example in `README.md`, `docs/GUIDE.md` and
  `docs/PERFORMANCE.md` now compiles as a doctest against the current API (argument orders,
  `Option` parameters, error variants and `f64` types were stale), and `PERFORMANCE.md` lists
  distinguishes committed benchmark tables from historical scratch measurements.
