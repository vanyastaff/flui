### Added

- Scalar controller retargeting between curve and spring motion preserves the
  last published position and velocity and cancels the displaced run.
- `AnimatedValue` owns one Vsync registration for all components, exposes a
  surviving observer stream, and supports atomic target and motion replacement.
- Opacity, padding and rotation support spring motion and retain their incoming
  velocities when retargeted through the render, layout and transform paths.

### Changed

- Replace manual `AnimatedValue::advance` and owner cloning with frame-driven
  ownership. Component vectors are fixed arrays; observer views remain cloneable.

### Fixed

- Finish interruptible spring motion continuously at its exact target, preserving
  velocity through the rest transition and removing dependence on the last frame.

- Keep controller sample time and pending playback changes intact when a curve
  panics, returns a non-finite position, or a simulation completion query fails.

- Report a curved controller's instantaneous velocity at its last sampled time, including direction and playback rate, instead of its average run velocity.
- Refuse stale derivatives after a curve or simulation reenters the controller; preserve the first failure through derivative-source retirement and keep representable scaled velocities finite.
