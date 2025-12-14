# Tasks: Refactor flui_types Geometry

## 1. Foundation - New Types and Infrastructure

- [ ] 1.1 Create `Vec2<T, U>` type in `geometry/vector.rs`
  - Fields: `x: T`, `y: T`, `_unit: PhantomData<U>`
  - Default: `T = f64`, `U = UnknownUnit`
  - Implement all vector operations: `hypot()`, `hypot2()`, `normalize()`, `dot()`, `cross()`
  - Implement operators: `Add`, `Sub`, `Mul<T>`, `Div<T>`, `Neg`
  - Implement `lerp()`, `angle()`, `from_angle()`, `rotate()`

- [ ] 1.2 Create unit type markers in `geometry/units.rs`
  - `UnknownUnit` (move from lib.rs)
  - `ScreenSpace` - screen pixel coordinates
  - `WorldSpace` - world/scene coordinates  
  - `LocalSpace` - widget-local coordinates
  - Add documentation explaining when to use each

- [ ] 1.3 Add `mint` feature flag to `Cargo.toml`
  - Add `mint = { version = "0.5", optional = true }` dependency
  - Create feature: `mint = ["dep:mint"]`

- [ ] 1.4 Add `glam` feature flag to `Cargo.toml`
  - Add `glam = { version = "0.30", optional = true }` dependency
  - Create feature: `glam = ["dep:glam"]`

## 2. Migrate Point to f64 Default

- [ ] 2.1 Update `Point<T, U>` in `geometry/point.rs`
  - Change default: `T = f64` (was f32)
  - Keep unit type parameter `U = UnknownUnit`
  - Update all tests to use f64

- [ ] 2.2 Remove vector operations from `Point`
  - Remove `normalize()` method
  - Remove `dot()` method
  - Remove `cross()` method
  - Remove `length()` method
  - Remove `length_squared()` method
  - Remove `atan2()` method
  - Remove `from_angle()` method
  - Keep: `distance()`, `distance_squared()`, `midpoint()`, `lerp()`

- [ ] 2.3 Fix `Point` operator semantics
  - Change `Sub<Point>` output to `Vec2` (Point - Point = Vector)
  - Add `Add<Vec2>` with output `Point` (Point + Vector = Point)
  - Add `Sub<Vec2>` with output `Point` (Point - Vector = Point)
  - Remove `Add<Point>` (mathematically incorrect)

## 3. Migrate Size to f64 Default with Units

- [ ] 3.1 Add unit type parameter to `Size` in `geometry/size.rs`
  - Change to `Size<T = f64, U = UnknownUnit>`
  - Add `_unit: PhantomData<U>` field
  - Update all constructors
  - Update all tests

- [ ] 3.2 Add `cast()` and `cast_unit()` methods
  - `cast<NewT>()` - convert numeric type
  - `cast_unit<NewU>()` - convert unit type

## 4. Migrate Rect to f64 Default with Units

- [ ] 4.1 Add generics to `Rect` in `geometry/rect.rs`
  - Change to `Rect<T = f64, U = UnknownUnit>`
  - Update `min`/`max` to use `Point<T, U>`
  - Update all methods to use generic `T`
  - Update all tests

- [ ] 4.2 Rename methods for kurbo compatibility
  - `expand_by()` → `inflate()` (add deprecation to old name)
  - Keep `inset()` (already kurbo-compatible)

- [ ] 4.3 Add `cast()` and `cast_unit()` methods

## 5. Migrate RRect to f64 Default with Units

- [ ] 5.1 Add generics to `RRect` in `geometry/rrect.rs`
  - Change to `RRect<T = f64, U = UnknownUnit>`
  - Update `rect` field to use `Rect<T, U>`
  - Update all methods
  - Update all tests

- [ ] 5.2 Add `cast()` and `cast_unit()` methods

## 6. Update Offset Type

- [ ] 6.1 Keep `Offset` as f32 for Flutter compatibility
  - Do NOT add unit types (keep simple for UI use)
  - Add conversion methods to/from `Vec2`
  - `to_vec2() -> Vec2<f64>`
  - `from_vec2(v: Vec2<f64>) -> Offset`

- [ ] 6.2 Align naming with kurbo
  - Rename `distance()` → `hypot()` (add deprecation to old name)
  - Rename `distance_squared()` → `hypot2()` (add deprecation to old name)

## 7. Implement mint Interoperability

- [ ] 7.1 Create `geometry/interop.rs` module

- [ ] 7.2 Implement mint conversions for `Point`
  - `From<mint::Point2<T>>` for `Point<T>`
  - `From<Point<T>>` for `mint::Point2<T>`

- [ ] 7.3 Implement mint conversions for `Vec2`
  - `From<mint::Vector2<T>>` for `Vec2<T>`
  - `From<Vec2<T>>` for `mint::Vector2<T>`

- [ ] 7.4 Implement glam conversions (when both features enabled)
  - `From<glam::Vec2>` for `Vec2<f32>`
  - `From<glam::DVec2>` for `Vec2<f64>`
  - `From<Vec2<f32>>` for `glam::Vec2`
  - `From<Vec2<f64>>` for `glam::DVec2`

## 8. Update Matrix4 and Transform

- [ ] 8.1 Migrate `Matrix4` to f64 default
  - Change default generic to f64
  - Update all tests

- [ ] 8.2 Migrate `Transform` to f64 default
  - Update to use f64 internally
  - Update all tests

## 9. Update Path and Painting Module

- [ ] 9.1 Update `Path` to use new geometry types
  - Update `PathCommand` to use `Point<f64>`
  - Update all path methods
  - Update tests

- [ ] 9.2 Improve Bezier algorithms
  - Replace fixed subdivision with adaptive algorithm
  - Use f64 for curve calculations
  - Add configurable tolerance parameter

## 10. Update Dependent Code

- [ ] 10.1 Update `geometry/mod.rs` exports
  - Export new types: `Vec2`
  - Export unit types: `ScreenSpace`, `WorldSpace`, `LocalSpace`
  - Update re-exports

- [ ] 10.2 Update `lib.rs` prelude
  - Add `Vec2` to prelude
  - Verify `Point`, `Rect`, `Size` use new defaults

- [ ] 10.3 Fix all compilation errors in flui_types
  - Run `cargo build -p flui_types`
  - Fix any type mismatches
  - Update internal usages

## 11. Documentation and Migration Guide

- [ ] 11.1 Update module-level documentation
  - Document new type hierarchy
  - Document unit type system
  - Document f64/f32 usage patterns

- [ ] 11.2 Create migration guide in `docs/migrations/`
  - `GEOMETRY_F64_MIGRATION.md`
  - Include search/replace patterns
  - Include code examples before/after

- [ ] 11.3 Add doc examples for new types
  - `Vec2` usage examples
  - Unit type examples
  - mint interop examples

## 12. Testing and Validation

- [ ] 12.1 Run all existing tests
  - `cargo test -p flui_types`
  - Fix any failures

- [ ] 12.2 Add new tests for `Vec2`
  - Vector operations tests
  - Operator tests
  - Conversion tests

- [ ] 12.3 Add tests for mint interop
  - Round-trip conversion tests
  - Zero-cost verification

- [ ] 12.4 Add tests for unit type safety
  - Compile-fail tests for mixing units
  - cast_unit() tests

- [ ] 12.5 Run full workspace build
  - `cargo build --workspace`
  - Fix any downstream breakages

## 13. Feature Flag Testing

- [ ] 13.1 Test with mint feature
  - `cargo test -p flui_types --features mint`

- [ ] 13.2 Test with glam feature
  - `cargo test -p flui_types --features glam`

- [ ] 13.3 Test with all features
  - `cargo test -p flui_types --all-features`

- [ ] 13.4 Test without optional features
  - `cargo test -p flui_types --no-default-features`
