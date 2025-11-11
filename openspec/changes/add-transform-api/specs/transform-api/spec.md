# Spec Delta: Transform API

**Change ID:** `add-transform-api`
**Capability:** `transform-api`
**Status:** Implemented (Phase 1)

## ADDED Requirements

### Requirement: High-Level Transform Enum

The geometry module SHALL provide a Transform enum that represents common 2D transformations in a type-safe manner.

**Rationale**: Enable developers to express transformations declaratively without manipulating low-level Matrix4 objects. Improves code readability, reduces errors, and provides semantic meaning.

#### Scenario: Identity Transform

**Given** a developer needs a no-op transformation
**When** Transform::identity() is created
**Then** it SHALL represent no transformation
**And** is_identity() SHALL return true
**And** conversion to Matrix4 SHALL yield identity matrix

```rust
let transform = Transform::identity();
assert!(transform.is_identity());

let matrix: Matrix4 = transform.into();
assert_eq!(matrix, Matrix4::identity());
```

#### Scenario: Translation Transform

**Given** a developer needs to move an object by (x, y) offset
**When** Transform::translate(x, y) is created
**Then** it SHALL represent a 2D translation
**And** conversion to Matrix4 SHALL have x in m[12] and y in m[13]

```rust
let transform = Transform::translate(50.0, 100.0);
let matrix: Matrix4 = transform.into();

assert_eq!(matrix.m[12], 50.0);  // Translation X
assert_eq!(matrix.m[13], 100.0); // Translation Y
```

#### Scenario: Rotation Transform

**Given** a developer needs to rotate an object by angle in radians
**When** Transform::rotate(angle) is created
**Then** it SHALL represent a 2D rotation around Z-axis
**And** conversion SHALL produce correct rotation matrix

```rust
use std::f32::consts::PI;

let transform = Transform::rotate(PI / 2.0);  // 90 degrees
let matrix: Matrix4 = transform.into();

// At 90°: cos(90°) ≈ 0, sin(90°) ≈ 1
assert!((matrix.m[0] - 0.0).abs() < 0.001);
assert!((matrix.m[1] - 1.0).abs() < 0.001);
```

#### Scenario: Uniform Scale Transform

**Given** a developer needs to scale uniformly
**When** Transform::scale(factor) is created
**Then** it SHALL scale equally in X and Y directions
**And** conversion SHALL have factor in m[0] and m[5]

```rust
let transform = Transform::scale(2.0);
let matrix: Matrix4 = transform.into();

assert_eq!(matrix.m[0], 2.0);  // Scale X
assert_eq!(matrix.m[5], 2.0);  // Scale Y
```

#### Scenario: Non-Uniform Scale Transform

**Given** a developer needs different scale factors for X and Y
**When** Transform::scale_xy(x, y) is created
**Then** it SHALL scale independently in X and Y directions

```rust
let transform = Transform::scale_xy(2.0, 3.0);
let matrix: Matrix4 = transform.into();

assert_eq!(matrix.m[0], 2.0);  // Scale X
assert_eq!(matrix.m[5], 3.0);  // Scale Y
```

### Requirement: Skew (Shear) Transform Support

The Transform enum SHALL provide first-class support for skew/shear transformations.

**Rationale**: Enable italic text, perspective effects, and trapezoid layouts without manual matrix construction. Essential for visual effects and text styling.

#### Scenario: Horizontal Skew (Italic Text)

**Given** a developer needs italic text effect
**When** Transform::skew(0.2, 0.0) is created
**Then** it SHALL produce horizontal shear
**And** matrix SHALL have tan(0.2) in m[1]

```rust
let transform = Transform::skew(0.2, 0.0);  // Italic text
let matrix: Matrix4 = transform.into();

// Skew matrix structure:
// [1      0       0  0]
// [tan(x) 1       0  0]
// [0      0       1  0]
// [0      0       0  1]
assert_eq!(matrix.m[0], 1.0);
assert!((matrix.m[1] - 0.2f32.tan()).abs() < 0.001);
assert_eq!(matrix.m[5], 1.0);
```

#### Scenario: Perspective Skew

**Given** a developer needs perspective effect
**When** Transform::skew(0.3, 0.3) is created
**Then** it SHALL produce both horizontal and vertical shear
**And** matrix SHALL have tan values in both m[1] and m[4]

```rust
let transform = Transform::skew(0.3, 0.3);
let matrix: Matrix4 = transform.into();

assert!((matrix.m[1] - 0.3f32.tan()).abs() < 0.001);  // X shear
assert!((matrix.m[4] - 0.3f32.tan()).abs() < 0.001);  // Y shear
```

### Requirement: Pivot Point Transforms

The Transform enum SHALL support rotation and scaling around specific pivot points.

**Rationale**: Common UI pattern where objects rotate/scale around their center or arbitrary point, not origin. Essential for button animations, icon transformations.

#### Scenario: Rotate Around Pivot

**Given** a developer needs to rotate around point (px, py)
**When** Transform::rotate_around(angle, px, py) is created
**Then** it SHALL be equivalent to translate(-px, -py) → rotate(angle) → translate(px, py)
**And** pivot point SHALL remain stationary after transformation

```rust
let transform = Transform::rotate_around(PI / 2.0, 50.0, 50.0);
let matrix: Matrix4 = transform.into();

// Point (50, 50) should stay at (50, 50)
let x = 50.0;
let y = 50.0;
let tx = matrix.m[0] * x + matrix.m[4] * y + matrix.m[12];
let ty = matrix.m[1] * x + matrix.m[5] * y + matrix.m[13];

assert!((tx - 50.0).abs() < 0.001);
assert!((ty - 50.0).abs() < 0.001);
```

#### Scenario: Scale Around Pivot

**Given** a developer needs to scale around point (px, py)
**When** Transform::scale_around(sx, sy, px, py) is created
**Then** it SHALL scale while keeping pivot point fixed

```rust
let transform = Transform::scale_around(2.0, 2.0, 100.0, 100.0);
let matrix: Matrix4 = transform.into();

// Pivot (100, 100) should remain at (100, 100)
// Other points should scale relative to pivot
```

### Requirement: Fluent Composition API

The Transform enum SHALL provide a fluent builder API for composing multiple transformations.

**Rationale**: Enable readable, self-documenting transform chains. Matches Flutter's API design and improves developer experience.

#### Scenario: Chain Transforms with .then()

**Given** a developer needs multiple transformations
**When** transforms are chained with .then()
**Then** they SHALL be applied in left-to-right order
**And** composition SHALL be automatic

```rust
let transform = Transform::translate(50.0, 50.0)
    .then(Transform::rotate(PI / 4.0))
    .then(Transform::scale(2.0));

// Applied as: translate → rotate → scale
assert!(transform.has_translation());
assert!(transform.has_rotation());
assert!(transform.has_scale());
```

#### Scenario: Identity Optimization

**Given** a transform chain includes Identity
**When** .then(Transform::Identity) is called
**Then** Identity SHALL be eliminated
**And** result SHALL be equivalent to original transform

```rust
let t1 = Transform::translate(10.0, 20.0);
let t2 = t1.then(Transform::Identity);

// Should optimize to just Translate
assert!(matches!(t2, Transform::Translate { .. }));
```

#### Scenario: Composition Flattening

**Given** nested Compose transforms
**When** they are chained
**Then** compositions SHALL be flattened into single Compose variant
**And** nested vectors SHALL be merged

```rust
let t1 = Transform::translate(10.0, 10.0)
    .then(Transform::rotate(PI / 4.0));
let t2 = Transform::scale(2.0)
    .then(Transform::skew(0.1, 0.0));
let composed = t1.then(t2);

// Should flatten to single Compose with 4 transforms
if let Transform::Compose(transforms) = composed {
    assert_eq!(transforms.len(), 4);
} else {
    panic!("Expected flattened Compose");
}
```

### Requirement: Idiomatic Rust Conversions

The Transform enum SHALL implement From/Into traits for Matrix4 conversion.

**Rationale**: Follow Rust idioms for type conversions. Enable type inference and composability with other Into<Matrix4> implementations.

#### Scenario: From Transform to Matrix4 (Owned)

**Given** a Transform instance
**When** converted to Matrix4 with .into()
**Then** it SHALL consume the Transform
**And** produce equivalent Matrix4

```rust
let transform = Transform::rotate(PI / 4.0);
let matrix: Matrix4 = transform.into();  // Consumes transform

// matrix now has rotation applied
```

#### Scenario: From &Transform to Matrix4 (Reference)

**Given** a Transform reference
**When** converted to Matrix4 with .into()
**Then** it SHALL NOT consume the Transform
**And** Transform SHALL remain usable

```rust
let transform = Transform::rotate(PI / 4.0);
let matrix: Matrix4 = (&transform).into();  // Borrows

// transform is still usable here
let matrix2: Matrix4 = transform.into();
```

#### Scenario: From Matrix4 to Transform

**Given** a Matrix4 instance
**When** converted to Transform
**Then** it SHALL wrap in Transform::Matrix variant
**And** identity matrices SHALL convert to Transform::Identity

```rust
let matrix = Matrix4::identity();
let transform: Transform = matrix.into();

assert!(matches!(transform, Transform::Identity));
```

#### Scenario: From Offset to Transform

**Given** an Offset instance
**When** converted to Transform
**Then** it SHALL create Transform::Translate

```rust
let offset = Offset::new(10.0, 20.0);
let transform: Transform = offset.into();

if let Transform::Translate { x, y } = transform {
    assert_eq!(x, 10.0);
    assert_eq!(y, 20.0);
} else {
    panic!("Expected Translate");
}
```

### Requirement: Transform Query Methods

The Transform enum SHALL provide methods to query transform properties.

**Rationale**: Enable conditional logic based on transform type without destructuring. Useful for optimization and debug logging.

#### Scenario: Identity Query

**Given** a Transform
**When** is_identity() is called
**Then** it SHALL return true only for Identity or identity Matrix

```rust
assert!(Transform::Identity.is_identity());
assert!(Transform::from(Matrix4::identity()).is_identity());
assert!(!Transform::translate(10.0, 10.0).is_identity());
```

#### Scenario: Has Translation Query

**Given** a Transform
**When** has_translation() is called
**Then** it SHALL return true if transform includes translation component

```rust
assert!(Transform::translate(10.0, 10.0).has_translation());
assert!(Transform::rotate_around(PI, 50.0, 50.0).has_translation());  // Pivot uses translation
assert!(!Transform::rotate(PI / 4.0).has_translation());
```

#### Scenario: Has Rotation Query

**Given** a Transform
**When** has_rotation() is called
**Then** it SHALL return true if transform includes rotation

```rust
assert!(Transform::rotate(PI / 4.0).has_rotation());
assert!(Transform::rotate_around(PI, 50.0, 50.0).has_rotation());
assert!(!Transform::translate(10.0, 10.0).has_rotation());
```

#### Scenario: Has Scale Query

**Given** a Transform
**When** has_scale() is called
**Then** it SHALL return true if transform includes scaling

```rust
assert!(Transform::scale(2.0).has_scale());
assert!(Transform::scale_xy(2.0, 3.0).has_scale());
assert!(Transform::scale_around(2.0, 2.0, 50.0, 50.0).has_scale());
assert!(!Transform::translate(10.0, 10.0).has_scale());
```

#### Scenario: Has Skew Query

**Given** a Transform
**When** has_skew() is called
**Then** it SHALL return true if transform includes skew/shear

```rust
assert!(Transform::skew(0.2, 0.0).has_skew());
assert!(!Transform::rotate(PI / 4.0).has_skew());
```

### Requirement: Transform Inversion

The Transform enum SHALL support computing inverse transforms.

**Rationale**: Essential for hit testing, reverse animations, and coordinate space conversions.

#### Scenario: Invert Translation

**Given** a translation Transform
**When** inverse() is called
**Then** it SHALL return translation with negated offsets

```rust
let transform = Transform::translate(10.0, 20.0);
let inverse = transform.inverse().unwrap();

if let Transform::Translate { x, y } = inverse {
    assert_eq!(x, -10.0);
    assert_eq!(y, -20.0);
}
```

#### Scenario: Invert Rotation

**Given** a rotation Transform
**When** inverse() is called
**Then** it SHALL return rotation with negated angle

```rust
let transform = Transform::rotate(PI / 4.0);
let inverse = transform.inverse().unwrap();

if let Transform::Rotate { angle } = inverse {
    assert!((angle + PI / 4.0).abs() < 0.001);
}
```

#### Scenario: Invert Scale

**Given** a scale Transform
**When** inverse() is called
**Then** it SHALL return scale with reciprocal factors

```rust
let transform = Transform::scale(2.0);
let inverse = transform.inverse().unwrap();

if let Transform::Scale { factor } = inverse {
    assert!((factor - 0.5).abs() < 0.001);
}
```

#### Scenario: Non-Invertible Scale

**Given** a scale Transform with zero factor
**When** inverse() is called
**Then** it SHALL return None

```rust
let transform = Transform::scale(0.0);
assert!(transform.inverse().is_none());
```

## MODIFIED Requirements

_No existing requirements modified - this is a pure addition_

## REMOVED Requirements

_No requirements removed_

## Dependencies

- **flui_types::geometry::Matrix4** - Target conversion type
- **flui_types::geometry::Offset** - Source conversion type
- **std::f32::consts::PI** - Rotation angle constants

## Implementation Notes

### Matrix4 Conversion Algorithm

Transform to Matrix4 conversion uses pattern matching with inline functions:

```rust
fn to_matrix_internal(&self) -> Matrix4 {
    match self {
        Transform::Identity => Matrix4::identity(),
        Transform::Translate { x, y } => Matrix4::translation(*x, *y, 0.0),
        Transform::Rotate { angle } => Matrix4::rotation_z(*angle),
        Transform::Scale { factor } => Matrix4::scaling(*factor, *factor, 1.0),
        Transform::ScaleXY { x, y } => Matrix4::scaling(*x, *y, 1.0),
        Transform::Skew { x, y } => {
            let mut matrix = Matrix4::identity();
            matrix.m[4] = y.tan();  // m[1][0]
            matrix.m[1] = x.tan();  // m[0][1]
            matrix
        },
        Transform::RotateAround { angle, pivot_x, pivot_y } => {
            Matrix4::translation(*pivot_x, *pivot_y, 0.0)
                * Matrix4::rotation_z(*angle)
                * Matrix4::translation(-*pivot_x, -*pivot_y, 0.0)
        },
        Transform::ScaleAround { x, y, pivot_x, pivot_y } => {
            Matrix4::translation(*pivot_x, *pivot_y, 0.0)
                * Matrix4::scaling(*x, *y, 1.0)
                * Matrix4::translation(-*pivot_x, -*pivot_y, 0.0)
        },
        Transform::Compose(transforms) => {
            transforms.iter()
                .map(|t| t.to_matrix_internal())
                .fold(Matrix4::identity(), |acc, m| acc * m)
        },
        Transform::Matrix(matrix) => *matrix,
    }
}
```

### Performance Optimizations

1. **Identity Elimination**: `.then(Identity)` is no-op
2. **Composition Flattening**: Nested Compose variants are merged
3. **Inline Everything**: All conversions marked `#[inline]`
4. **Stack Allocation**: Enum fits in 24 bytes (vs 64 for Matrix4)

### Testing Requirements

Each transform variant MUST have:
- Unit test for creation
- Unit test for Matrix4 conversion
- Unit test for composition
- Unit test for inversion (if applicable)

Composition MUST have:
- Test for flattening behavior
- Test for identity optimization
- Test for transform order preservation

## Validation

### Compilation

```bash
cargo check -p flui_types
cargo clippy -p flui_types -- -D warnings
```

### Tests

```bash
cargo test -p flui_types transform
# Expected: 18/18 tests pass
```

### Documentation

```bash
cargo doc -p flui_types --open
# Verify Transform docs render correctly
```

## Migration Impact

**User Code:** No changes required - Transform is additive

**Existing Matrix4 Code:** Continues to work unchanged

**New Code:** Can optionally use Transform for better DX

**Deprecations:** None - both Transform and Matrix4 are valid

## Use Cases

### Use Case 1: Italic Text

```rust
let italic = Transform::skew(0.2, 0.0);
canvas.save();
canvas.transform(italic);
canvas.draw_text("Italic Text", position, style);
canvas.restore();
```

### Use Case 2: Button Rotation Animation

```rust
let rotation = Transform::rotate_around(
    lerp(0.0, PI * 2.0, t),  // Animate 0° to 360°
    button_center_x,
    button_center_y,
);
canvas.transform(rotation);
canvas.draw_button(button);
```

### Use Case 3: Composed UI Transform

```rust
let transform = Transform::translate(container_x, container_y)
    .then(Transform::scale(zoom_factor))
    .then(Transform::rotate(tilt_angle));

canvas.transform(transform);
// Draw entire UI with combined transform
```

## Breaking Changes

**None** - Transform is a new API with no changes to existing code.

## Notes

- Transform is designed for 2D graphics; use Matrix4 directly for 3D
- Skew matrix structure follows standard graphics conventions
- Pivot transforms use translate-rotate/scale-translate pattern
- Composition applies transforms left-to-right (matches Flutter)
- Epsilon comparisons used for floating-point identity checks
