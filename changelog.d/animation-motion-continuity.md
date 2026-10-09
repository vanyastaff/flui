### Added

- Scalar controller retargeting between curve and spring motion preserves the
  last published position and velocity and cancels the displaced run.
- `AnimatedValue` owns one Vsync registration for all components, exposes a
  surviving observer stream, and supports atomic target and motion replacement.
- Animation status subscriptions own removal authority through
  `StatusSubscription`; dropping a guard removes its callback without retaining
  the animation owner, while detaching leaves it registered until source closure.
- Opacity, padding and rotation support spring motion and retain their incoming
  velocities when retargeted through the render, layout and transform paths.

### Changed

- Replace manual `AnimatedValue::advance` and owner cloning with frame-driven
  ownership. Component vectors are fixed arrays; observer views remain cloneable.
- Custom `Animation<T>` implementations provide `subscribe_status`, returning
  source-bound removal authority; framework relays share delivery recovery.
- Scroll and page animation methods accept `ArcCurve`. Replacing programmatic
  scroll motion retains its published velocity, including when braking at the
  current position.

### Removed

- Manual animation status listener IDs and source-selected status removal.
  Retain the returned subscription to control its lifetime, or detach it to
  leave the callback registered until source closure.

### Fixed

- Return inert status subscriptions after controller or switch disposal and
  release state borrows before refusing exhausted controller identities.
- Finish interruptible spring motion continuously at its exact target, preserving
  velocity through the rest transition and removing dependence on the last frame.

- Keep controller sample time and pending playback changes intact when a curve
  panics, returns a non-finite position, or a simulation completion query fails.

- Report a curved controller's instantaneous velocity at its last sampled time, including direction and playback rate, instead of its average run velocity.
- Refuse stale derivatives after a curve or simulation reenters the controller; preserve the first failure through derivative-source retirement and keep representable scaled velocities finite.
