### Fixed

- Report a curved controller's instantaneous velocity at its last sampled time, including direction and playback rate, instead of its average run velocity.
- Refuse stale derivatives after a curve or simulation reenters the controller; preserve the first failure through derivative-source retirement and keep representable scaled velocities finite.
