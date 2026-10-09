### Added

- Scalar controller retargeting between curve and spring motion preserves the
  last published position and velocity and cancels the displaced run.

### Fixed

- Keep controller sample time and pending playback changes intact when a curve
  panics, returns a non-finite position, or a simulation completion query fails.

- Report a curved controller's instantaneous velocity at its last sampled time, including direction and playback rate, instead of its average run velocity.
- Refuse stale derivatives after a curve or simulation reenters the controller; preserve the first failure through derivative-source retirement and keep representable scaled velocities finite.
