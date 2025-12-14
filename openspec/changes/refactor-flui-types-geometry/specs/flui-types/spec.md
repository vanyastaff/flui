# Spec Delta: flui-types Geometry

## ADDED Requirements

### Requirement: Vec2 Type

The system SHALL provide a `Vec2<T, U>` type representing a 2D mathematical vector with direction and magnitude.

The `Vec2` type SHALL be generic over numeric type `T` (defaulting to `f64`) and unit type `U` (defaulting to `UnknownUnit`).

The `Vec2` type SHALL implement the following vector operations:
- `hypot()` - magnitude (length) of the vector
- `hypot2()` - squared magnitude (avoids sqrt)
- `normalize()` - unit vector in same direction
- `dot(other)` - dot product with another vector
- `cross(other)` - cross product (2D returns scalar)
- `angle()` - angle from positive x-axis in radians
- `from_angle(radians)` - create unit vector from angle
- `rotate(radians)` - rotate vector by angle
- `lerp(other, t)` - linear interpolation

The `Vec2` type SHALL implement standard operators:
- `Add<Vec2>` returning `Vec2`
- `Sub<Vec2>` returning `Vec2`
- `Mul<T>` (scalar) returning `Vec2`
- `Div<T>` (scalar) returning `Vec2`
- `Neg` returning `Vec2`

#### Scenario: Vector magnitude calculation
- **GIVEN** a vector `v = Vec2::new(3.0, 4.0)`
- **WHEN** `v.hypot()` is called
- **THEN** the result SHALL be `5.0`

#### Scenario: Vector normalization
- **GIVEN** a vector `v = Vec2::new(3.0, 4.0)`
- **WHEN** `v.normalize()` is called
- **THEN** the result SHALL be `Vec2::new(0.6, 0.8)`
- **AND** the normalized vector's magnitude SHALL be `1.0`

#### Scenario: Vector dot product
- **GIVEN** vectors `a = Vec2::new(1.0, 0.0)` and `b = Vec2::new(0.0, 1.0)`
- **WHEN** `a.dot(b)` is called
- **THEN** the result SHALL be `0.0` (perpendicular vectors)

#### Scenario: Vector addition
- **GIVEN** vectors `a = Vec2::new(1.0, 2.0)` and `b = Vec2::new(3.0, 4.0)`
- **WHEN** `a + b` is evaluated
- **THEN** the result SHALL be `Vec2::new(4.0, 6.0)`

### Requirement: Coordinate Space Unit Types

The system SHALL provide unit marker types for compile-time coordinate space safety:
- `UnknownUnit` - default unit when no specific space is needed
- `ScreenSpace` - screen pixel coordinates
- `WorldSpace` - world/scene coordinates
- `LocalSpace` - widget-local coordinates

All unit types SHALL be zero-sized types (ZST) with no runtime overhead.

Geometry types with different unit types SHALL NOT be directly combinable without explicit `cast_unit()` conversion.

#### Scenario: Prevent mixing coordinate spaces
- **GIVEN** `screen_point: Point<f64, ScreenSpace>` and `world_point: Point<f64, WorldSpace>`
- **WHEN** attempting to compute `screen_point + world_point`
- **THEN** compilation SHALL fail with a type error

#### Scenario: Explicit unit conversion
- **GIVEN** `screen_point: Point<f64, ScreenSpace>`
- **WHEN** `screen_point.cast_unit::<WorldSpace>()` is called
- **THEN** a `Point<f64, WorldSpace>` SHALL be returned with same coordinates

### Requirement: mint Interoperability

The system SHALL provide optional `mint` crate integration via feature flag.

When the `mint` feature is enabled, the following conversions SHALL be implemented:
- `From<mint::Point2<T>>` for `Point<T>`
- `From<Point<T>>` for `mint::Point2<T>`
- `From<mint::Vector2<T>>` for `Vec2<T>`
- `From<Vec2<T>>` for `mint::Vector2<T>`

All mint conversions SHALL be zero-cost (no runtime overhead).

#### Scenario: Convert from mint Point2
- **GIVEN** `mint_point = mint::Point2 { x: 10.0, y: 20.0 }`
- **WHEN** `Point::from(mint_point)` is called
- **THEN** a `Point` with `x = 10.0, y = 20.0` SHALL be returned

#### Scenario: Round-trip conversion preserves values
- **GIVEN** `point = Point::new(3.14, 2.71)`
- **WHEN** converted to mint and back: `Point::from(mint::Point2::from(point))`
- **THEN** the result SHALL equal the original point

### Requirement: glam Interoperability

The system SHALL provide optional `glam` crate integration via feature flag.

When the `glam` feature is enabled, the following conversions SHALL be implemented:
- `From<glam::Vec2>` for `Vec2<f32>`
- `From<glam::DVec2>` for `Vec2<f64>`
- `From<Vec2<f32>>` for `glam::Vec2`
- `From<Vec2<f64>>` for `glam::DVec2`

#### Scenario: Convert from glam Vec2
- **GIVEN** `glam_vec = glam::Vec2::new(1.0, 2.0)`
- **WHEN** `Vec2::<f32>::from(glam_vec)` is called
- **THEN** a `Vec2<f32>` with `x = 1.0, y = 2.0` SHALL be returned

### Requirement: f64 Default Precision with Generic API

All geometry types SHALL use `f64` as the default numeric type for improved precision in calculations.

All geometry types SHALL be fully generic, allowing users to specify precision via type parameter:
- `Point<T = f64, U = UnknownUnit>` - 2D position
- `Vec2<T = f64, U = UnknownUnit>` - 2D vector
- `Size<T = f64, U = UnknownUnit>` - 2D dimensions
- `Rect<T = f64, U = UnknownUnit>` - axis-aligned rectangle
- `RRect<T = f64, U = UnknownUnit>` - rounded rectangle

Users SHALL specify f32 precision using standard Rust generic syntax:
- `Point::<f32>::new(1.0, 2.0)` - turbofish syntax
- `let p: Point<f32> = Point::new(1.0, 2.0)` - type annotation

The system SHALL NOT provide type aliases like `Point32` — use Rust generics idiomatically.

#### Scenario: Default type is f64
- **GIVEN** `let point = Point::new(1.0, 2.0);`
- **WHEN** the type is inferred
- **THEN** it SHALL be `Point<f64, UnknownUnit>`

#### Scenario: Explicit f32 via turbofish
- **GIVEN** `let point = Point::<f32>::new(1.0, 2.0);`
- **WHEN** the type is inspected
- **THEN** it SHALL be `Point<f32, UnknownUnit>`

#### Scenario: Explicit f32 via type annotation
- **GIVEN** `let point: Point<f32> = Point::new(1.0, 2.0);`
- **WHEN** the type is inspected
- **THEN** it SHALL be `Point<f32, UnknownUnit>`

## MODIFIED Requirements

### Requirement: Point Semantics

The `Point<T, U>` type SHALL represent an absolute position in 2D coordinate space.

The `Point` type SHALL NOT implement vector operations (normalize, dot, cross, length).

The `Point` type SHALL implement the following position operations:
- `distance(other)` - Euclidean distance to another point
- `distance_squared(other)` - squared distance (avoids sqrt)
- `midpoint(other)` - point halfway between two points
- `lerp(other, t)` - linear interpolation between points
- `min(other)` - component-wise minimum
- `max(other)` - component-wise maximum
- `clamp(min, max)` - clamp to rectangular region

The `Point` type SHALL implement the following operators with correct semantics:
- `Sub<Point>` returning `Vec2` (point - point = vector)
- `Add<Vec2>` returning `Point` (point + vector = point)
- `Sub<Vec2>` returning `Point` (point - vector = point)

The `Point` type SHALL NOT implement `Add<Point>` (mathematically undefined).

#### Scenario: Point subtraction yields vector
- **GIVEN** points `a = Point::new(10.0, 20.0)` and `b = Point::new(7.0, 16.0)`
- **WHEN** `a - b` is evaluated
- **THEN** the result SHALL be `Vec2::new(3.0, 4.0)`
- **AND** the result type SHALL be `Vec2`

#### Scenario: Point plus vector yields point
- **GIVEN** `point = Point::new(10.0, 20.0)` and `vec = Vec2::new(5.0, 5.0)`
- **WHEN** `point + vec` is evaluated
- **THEN** the result SHALL be `Point::new(15.0, 25.0)`
- **AND** the result type SHALL be `Point`

#### Scenario: Point addition is not allowed
- **GIVEN** points `a = Point::new(1.0, 2.0)` and `b = Point::new(3.0, 4.0)`
- **WHEN** attempting to compute `a + b`
- **THEN** compilation SHALL fail

### Requirement: Size with Unit Types

The `Size<T, U>` type SHALL be generic over numeric type `T` (defaulting to `f64`) and unit type `U` (defaulting to `UnknownUnit`).

The `Size` type SHALL provide methods:
- `cast<NewT>()` - convert to different numeric type
- `cast_unit<NewU>()` - convert to different unit type

#### Scenario: Size with unit type
- **GIVEN** `size: Size<f64, ScreenSpace> = Size::new(800.0, 600.0)`
- **WHEN** `size.cast_unit::<WorldSpace>()` is called
- **THEN** a `Size<f64, WorldSpace>` SHALL be returned

### Requirement: Rect with Unit Types

The `Rect<T, U>` type SHALL be generic over numeric type `T` (defaulting to `f64`) and unit type `U` (defaulting to `UnknownUnit`).

The `Rect` internal points (`min`, `max`) SHALL use the same `T` and `U` parameters.

The `Rect` type SHALL provide `inflate(delta)` method as the preferred name for expanding a rectangle (replacing `expand_by`).

#### Scenario: Rect with consistent unit types
- **GIVEN** `rect: Rect<f64, ScreenSpace>`
- **WHEN** `rect.min` is accessed
- **THEN** its type SHALL be `Point<f64, ScreenSpace>`

### Requirement: RRect with Unit Types

The `RRect<T, U>` type SHALL be generic over numeric type `T` (defaulting to `f64`) and unit type `U` (defaulting to `UnknownUnit`).

The `RRect` internal rect SHALL use the same `T` and `U` parameters.

#### Scenario: RRect with unit types
- **GIVEN** `rrect: RRect<f64, LocalSpace>`
- **WHEN** `rrect.rect` is accessed
- **THEN** its type SHALL be `Rect<f64, LocalSpace>`

## REMOVED Requirements

### Requirement: Point Vector Operations

**Reason:** Vector operations (normalize, dot, cross, length) are mathematically vector operations, not point operations. These have been moved to the new `Vec2` type.

**Migration:** Replace `point.normalize()` with `(point - Point::ZERO).normalize()` or use `Vec2` directly.

### Requirement: Point Addition Operator

**Reason:** Adding two points is mathematically undefined. The operation `Point + Point` has no geometric meaning.

**Migration:** If you need the midpoint, use `point1.midpoint(point2)`. If you need vector addition, convert to vectors first.
