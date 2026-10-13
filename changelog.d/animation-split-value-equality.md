### Added

- **`Split::try_with_curves`** (`flui-animation`): the checked constructor returns
  `CurveError::NonFinite`/`OutOfRange` for an invalid split point instead of panicking.

### Fixed

- **Equal `Split` curves compare equal through `ArcCurve`** (`flui-animation`): a split of built-in
  halves now has value equality, so an implicit animation rebuilt each frame with an equal
  `Split` keeps its run instead of restarting it. A split with a user-defined half still compares
  by identity.
