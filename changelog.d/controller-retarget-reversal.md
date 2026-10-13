### Fixed

- **Controller retargets reverse faster** (`flui-animation`): `AnimationController::retarget` with
  `MotionSpec::Curve` now shortens a segment that reverses toward the previous segment's start by
  the eased fraction already travelled, as `MotionSpec::Curve` documents and `AnimatedValue`
  already did. A reversed `ScrollController::animate_to` no longer runs its full duration back.
