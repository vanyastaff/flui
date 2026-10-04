### Fixed
- Point distances and line lengths convert coordinates before subtraction, keeping large finite `f32` endpoint distances representable in their returned `f64` type.
