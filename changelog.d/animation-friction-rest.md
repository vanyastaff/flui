### Fixed

- Friction simulations with weak drag and very small positive tolerances retain a finite time to rest instead of running indefinitely after threshold underflow.

### Removed

- Remove the unused `FrictionSimulation::final_x()` getter. Query the resting position with `Simulation::x(f64::INFINITY)`; position-to-time queries remain available through `time_at_x()`.
