### Changed
- Gesture settings can select least squares, impulse or weighted recent-interval release velocity. Drag, multi-drag, tap-and-drag and scale capture the selection at contact admission; defaults remain least squares.
- Every selected estimator uses the same explicit sample-clock stop gate and memoized sample buffer.

### Removed
- The separate iOS, macOS and impulse tracker wrappers; use `VelocityTracker::with_estimator` and `VelocityEstimator` instead.

### Fixed
- Weighted release estimators exclude intervals before a stationary gap or outside the sample horizon, so resumed motion has the same estimate as an independent fresh history.
