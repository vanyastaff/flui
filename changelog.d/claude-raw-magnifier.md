### Added

- `RawMagnifier`, a loupe of a fixed size that shows what is painted behind it enlarged, clipped to a `MagnifierShape` (circle, rounded rectangle, capsule), with a `MagnifierDecoration` border and shadows and a `MagnifierScale` that refuses a scale that is not finite and above zero.
- `ImageFilter::try_translated` and `ImageTransform::try_translated`, the same filter seen from a space whose origin is moved.

### Fixed

- A backdrop filter's `ImageFilter::Transform` now applies in the painting node's local space: before, a node away from its layer's origin magnified about the wrong point.
