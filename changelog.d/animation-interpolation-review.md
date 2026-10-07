### Fixed

- `Angle::nearest_equivalent` stays finite for opposite extreme angles instead of returning NaN.
- `Matrix4::lerp` no longer overflows between opposite extreme components, extrapolates a tiny
  rotation along its arc, and returns identical endpoints unchanged, so a collapsed rotated
  transform keeps its orientation while `AnimatedContainer` restarts for another property.
- A settled `AnimatedValue` reports its target exactly, so a transparent non-black colour keeps
  its components at rest.
