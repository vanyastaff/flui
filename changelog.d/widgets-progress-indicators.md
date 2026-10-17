### Added

- **Progress indicators** (`flui-material`): `CircularProgressIndicator`, and the Material 3
  Expressive `LinearWavyProgressIndicator` and `CircularWavyProgressIndicator`, determinate or
  indeterminate. Determinate linear and circular indicators leave a 4 dp gap before the
  track, and a linear track ends in a stop dot.
- **Refresh indicator style** (`flui-widgets`): `RefreshIndicatorStyle` builds the indicator a
  `RefreshIndicator` shows from its `RefreshIndicatorPhase` (the pull's progress, or
  refreshing). `MaterialApp` installs the Material circular indicator. Without a style the
  `ActivityIndicator` spinner shows as before (ADR-0235).
- **`ActivityArc`** (`flui-widgets`): the indeterminate circular motion `ActivityIndicator`
  draws, for indicators that draw the arc their own way.

### Changed

- **Progress values** (`flui-material`): a non-finite `LinearProgressIndicator` value now keeps
  showing the last finite value instead of reading as 0, and a determinate indicator publishes
  a 0–100 range to assistive technology.
