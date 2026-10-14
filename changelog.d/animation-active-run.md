### Changed

- `AnimationController` releases a timed run's curve when the run completes, not at the next command or controller drop, so a curve's `Drop` runs as the run finishes. A trivial curved run (zero distance or duration) admitted while the controller delivers now keeps its curve until that run settles.
