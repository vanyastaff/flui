### Added

- `ScrollBehavior`, `ScrollConfiguration` and `DefaultScrollBehavior`: a scroll view asks the
  behavior of the nearest configuration for the physics and drag devices its `ScrollViewConfig`
  leaves open, with the `PlatformConvention` it is built under and its axis
  (`ScrollBehaviorDetails`).
- `ClampingScrollPhysics::try_new`, `BouncingScrollPhysics::try_new` and `ScrollPhysicsError`, with
  accessors for each coefficient.

### Changed

- A scroll view that names no physics follows its platform: `BouncingScrollPhysics` on macOS and
  iOS, `ClampingScrollPhysics` elsewhere.
- The standard physics fling on any nonzero release velocity: the drag gesture already zeroes a
  release below the platform's minimum fling velocity.

### Removed

- The public fields of `ClampingScrollPhysics` and `BouncingScrollPhysics`, and their
  `min_fling_velocity_px_per_sec` threshold.
