### Added

- `AnimationController::finish()` completes the original finite run successfully at its declared endpoint, including paused motion and finite repeats. Infinite repeats and arbitrary custom simulations return `AnimationError::NoFiniteEndpoint` without changing the active run.
