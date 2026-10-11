### Changed
- `SpringSimulation::new` now takes an explicit `Tolerance` and returns `Result<_, SimulationError>`; migrate `try_new` calls to this single constructor.
- `AnimationError::Simulation` preserves the typed construction cause. `AnimationError` no longer implements `Eq`, since simulation errors can contain floating-point inputs.

### Removed
- `Simulation::tolerance` and the unused `Tolerance::time` field. Supply tolerance when constructing a simulation; custom `Simulation` implementations only provide sampling and completion methods.

### Fixed
- An overflowing fling refuses before replacing an active run, preserving its value, direction and pending completion future.
