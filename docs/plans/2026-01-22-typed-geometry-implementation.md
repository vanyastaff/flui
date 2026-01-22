# Type-Safe Geometry Phase 1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement unit-parametrized geometry types (Point<T>, Vec2<T>, Size<T>, Offset<T>) with GPUI-style type safety and numeric validation.

**Architecture:** Convert existing geometry primitives to generic types over Unit trait, add unit newtypes (Pixels, DevicePixels, etc.), implement three-level safety model (fast/safe/validated), provide GPU conversion paths.

**Tech Stack:** Rust 2021, traits (Unit, NumericUnit), newtypes, thiserror for errors, std::ops for operators

---

## Task 1: Core Traits Foundation

**Files:**
- Modify: `crates/flui_types/src/geometry/traits.rs:1-100`
- Test: `crates/flui_types/src/geometry/traits.rs:200+` (inline tests)

**Step 1: Add Unit trait**

Add to `traits.rs` after existing `Axis` impl:

```rust
// ============================================================================
// UNIT - Marker trait for unit types
// ============================================================================

/// Marker trait for all unit types (Pixels, DevicePixels, etc.).
///
/// This trait enables generic geometry types to work with different
/// coordinate systems in a type-safe manner.
pub trait Unit: Copy + Clone + Debug {
    /// The underlying scalar type (f32, i32, etc.)
    type Scalar: Copy;
    
    /// Returns the zero value for this unit
    fn zero() -> Self;
}
```

**Step 2: Add NumericUnit trait**

Add after `Unit` trait:

```rust
/// Units that support arithmetic operations.
///
/// This trait enables math operations on unit types while maintaining
/// type safety. All operations preserve the unit type.
pub trait NumericUnit: Unit {
    /// Add two values of the same unit
    fn add(self, other: Self) -> Self;
    
    /// Subtract two values of the same unit
    fn sub(self, other: Self) -> Self;
    
    /// Multiply by a scalar (dimensionless)
    fn mul(self, scalar: f32) -> Self;
    
    /// Divide by a scalar (dimensionless)
    fn div(self, scalar: f32) -> Self;
}
```

**Step 3: Implement Unit for f32**

Add impl for raw f32 support:

```rust
impl Unit for f32 {
    type Scalar = f32;
    
    #[inline]
    fn zero() -> Self {
        0.0
    }
}

impl NumericUnit for f32 {
    #[inline]
    fn add(self, other: Self) -> Self {
        self + other
    }
    
    #[inline]
    fn sub(self, other: Self) -> Self {
        self - other
    }
    
    #[inline]
    fn mul(self, scalar: f32) -> Self {
        self * scalar
    }
    
    #[inline]
    fn div(self, scalar: f32) -> Self {
        self / scalar
    }
}
```

**Step 4: Add inline tests**

Add at end of file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_f32_unit() {
        assert_eq!(f32::zero(), 0.0);
        assert_eq!(NumericUnit::add(1.0, 2.0), 3.0);
        assert_eq!(NumericUnit::mul(2.0, 3.0), 6.0);
    }
}
```

**Step 5: Run tests**

```bash
cargo test -p flui_types --lib geometry::traits
```

Expected: All tests PASS

**Step 6: Commit**

```bash
git add crates/flui_types/src/geometry/traits.rs
git commit -m "feat(geometry): add Unit and NumericUnit traits

- Unit trait for type-safe coordinate systems
- NumericUnit trait for arithmetic operations
- Implement for f32 (raw numeric support)
- Add unit tests"
```

---

## Task 2: Unit Types - Pixels

**Files:**
- Modify: `crates/flui_types/src/geometry/units.rs:1-200`
- Test: `crates/flui_types/src/geometry/units.rs:800+`

**Step 1: Implement Unit trait for Pixels**

Add after existing `Pixels` impl block:

```rust
use super::traits::{Unit, NumericUnit, Half, Negate, IsZero};

impl Unit for Pixels {
    type Scalar = f32;
    
    #[inline]
    fn zero() -> Self {
        Self::ZERO
    }
}

impl NumericUnit for Pixels {
    #[inline]
    fn add(self, other: Self) -> Self {
        Pixels(self.0 + other.0)
    }
    
    #[inline]
    fn sub(self, other: Self) -> Self {
        Pixels(self.0 - other.0)
    }
    
    #[inline]
    fn mul(self, scalar: f32) -> Self {
        Pixels(self.0 * scalar)
    }
    
    #[inline]
    fn div(self, scalar: f32) -> Self {
        Pixels(self.0 / scalar)
    }
}
```

**Step 2: Implement conversion traits**

Add conversions for GPU integration:

```rust
impl From<Pixels> for f32 {
    #[inline]
    fn from(p: Pixels) -> f32 {
        p.0
    }
}

impl From<f32> for Pixels {
    #[inline]
    fn from(v: f32) -> Pixels {
        Pixels(v)
    }
}
```

**Step 3: Add scale method**

Add to existing `Pixels` impl:

```rust
impl Pixels {
    /// Scale pixels to device pixels with scale factor
    pub fn scale(self, factor: f32) -> ScaledPixels {
        ScaledPixels(self.0 * factor)
    }
}
```

**Step 4: Write tests**

Add tests at end of units.rs:

```rust
#[cfg(test)]
mod unit_trait_tests {
    use super::*;
    
    #[test]
    fn test_pixels_unit_trait() {
        let zero = Pixels::zero();
        assert_eq!(zero.0, 0.0);
        
        let a = px(10.0);
        let b = px(20.0);
        assert_eq!(a.add(b).0, 30.0);
        assert_eq!(a.mul(2.0).0, 20.0);
    }
    
    #[test]
    fn test_pixels_conversions() {
        let p = px(100.0);
        let f: f32 = p.into();
        assert_eq!(f, 100.0);
        
        let p2: Pixels = 50.0.into();
        assert_eq!(p2.0, 50.0);
    }
    
    #[test]
    fn test_pixels_scale() {
        let p = px(100.0);
        let scaled = p.scale(2.0);
        assert_eq!(scaled.0, 200.0);
    }
}
```

**Step 5: Run tests**

```bash
cargo test -p flui_types --lib geometry::units::unit_trait_tests
```

Expected: All tests PASS

**Step 6: Commit**

```bash
git add crates/flui_types/src/geometry/units.rs
git commit -m "feat(geometry): implement Unit traits for Pixels

- Unit and NumericUnit trait implementations
- From<f32> and Into<f32> conversions
- scale() method for GPU conversion
- Unit tests for traits and conversions"
```

---

## Task 3: Unit Types - DevicePixels, ScaledPixels, Rems

**Files:**
- Modify: `crates/flui_types/src/geometry/units.rs:500-800`

**Step 1: Implement Unit for DevicePixels**

Add after DevicePixels impl:

```rust
impl Unit for DevicePixels {
    type Scalar = i32;
    
    #[inline]
    fn zero() -> Self {
        DevicePixels(0)
    }
}

impl NumericUnit for DevicePixels {
    #[inline]
    fn add(self, other: Self) -> Self {
        DevicePixels(self.0.saturating_add(other.0))
    }
    
    #[inline]
    fn sub(self, other: Self) -> Self {
        DevicePixels(self.0.saturating_sub(other.0))
    }
    
    #[inline]
    fn mul(self, scalar: f32) -> Self {
        DevicePixels((self.0 as f32 * scalar).round() as i32)
    }
    
    #[inline]
    fn div(self, scalar: f32) -> Self {
        DevicePixels((self.0 as f32 / scalar).round() as i32)
    }
}
```

**Step 2: Implement Unit for ScaledPixels**

```rust
impl Unit for ScaledPixels {
    type Scalar = f32;
    
    #[inline]
    fn zero() -> Self {
        ScaledPixels(0.0)
    }
}

impl NumericUnit for ScaledPixels {
    #[inline]
    fn add(self, other: Self) -> Self {
        ScaledPixels(self.0 + other.0)
    }
    
    #[inline]
    fn sub(self, other: Self) -> Self {
        ScaledPixels(self.0 - other.0)
    }
    
    #[inline]
    fn mul(self, scalar: f32) -> Self {
        ScaledPixels(self.0 * scalar)
    }
    
    #[inline]
    fn div(self, scalar: f32) -> Self {
        ScaledPixels(self.0 / scalar)
    }
}

impl ScaledPixels {
    /// Convert to device pixels by rounding
    pub fn to_device_pixels(self) -> DevicePixels {
        DevicePixels(self.0.round() as i32)
    }
}
```

**Step 3: Implement Unit for Rems**

```rust
impl Unit for Rems {
    type Scalar = f32;
    
    #[inline]
    fn zero() -> Self {
        Rems(0.0)
    }
}

impl NumericUnit for Rems {
    #[inline]
    fn add(self, other: Self) -> Self {
        Rems(self.0 + other.0)
    }
    
    #[inline]
    fn sub(self, other: Self) -> Self {
        Rems(self.0 - other.0)
    }
    
    #[inline]
    fn mul(self, scalar: f32) -> Self {
        Rems(self.0 * scalar)
    }
    
    #[inline]
    fn div(self, scalar: f32) -> Self {
        Rems(self.0 / scalar)
    }
}
```

**Step 4: Add conversions**

```rust
impl From<ScaledPixels> for f32 {
    fn from(sp: ScaledPixels) -> f32 { sp.0 }
}

impl From<f32> for ScaledPixels {
    fn from(v: f32) -> ScaledPixels { ScaledPixels(v) }
}

impl From<Rems> for f32 {
    fn from(r: Rems) -> f32 { r.0 }
}

impl From<f32> for Rems {
    fn from(v: f32) -> Rems { Rems(v) }
}
```

**Step 5: Add tests**

```rust
#[test]
fn test_device_pixels_unit() {
    let a = device_px(10);
    let b = device_px(20);
    assert_eq!(a.add(b).0, 30);
    assert_eq!(a.mul(2.0).0, 20);
}

#[test]
fn test_scaled_pixels_unit() {
    let sp = scaled_px(200.0);
    let dp = sp.to_device_pixels();
    assert_eq!(dp.0, 200);
}

#[test]
fn test_rems_unit() {
    let a = rems(1.0);
    let b = rems(0.5);
    assert_eq!(a.add(b).0, 1.5);
}
```

**Step 6: Run tests**

```bash
cargo test -p flui_types --lib geometry::units
```

Expected: All tests PASS

**Step 7: Commit**

```bash
git add crates/flui_types/src/geometry/units.rs
git commit -m "feat(geometry): implement Unit traits for all unit types

- DevicePixels: Unit + NumericUnit with saturating ops
- ScaledPixels: Unit + NumericUnit with to_device_pixels()
- Rems: Unit + NumericUnit
- f32 conversions for all types
- Comprehensive unit tests"
```

---

## Task 4: Geometry Error Type

**Files:**
- Create: `crates/flui_types/src/geometry/error.rs`
- Modify: `crates/flui_types/src/geometry/mod.rs:1-10`
- Modify: `crates/flui_types/Cargo.toml:15`

**Step 1: Add thiserror dependency**

In `Cargo.toml`, add to `[dependencies]`:

```toml
thiserror = "2.0"
```

**Step 2: Create error.rs**

```rust
//! Error types for geometry operations.

use std::fmt;

/// Errors that can occur during geometry operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum GeometryError {
    /// Coordinates are not finite (NaN or infinity)
    #[error("Invalid coordinates: ({x}, {y}) - must be finite")]
    InvalidCoordinates { x: f32, y: f32 },
    
    /// Division by zero attempted
    #[error("Division by zero")]
    DivisionByZero,
    
    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}
```

**Step 3: Export from mod.rs**

Add to geometry/mod.rs after existing pub mod declarations:

```rust
pub mod error;
pub use error::GeometryError;
```

**Step 4: Write tests**

Add to error.rs:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_display() {
        let err = GeometryError::InvalidCoordinates { x: f32::NAN, y: 100.0 };
        let msg = format!("{}", err);
        assert!(msg.contains("Invalid coordinates"));
        assert!(msg.contains("must be finite"));
    }
    
    #[test]
    fn test_error_debug() {
        let err = GeometryError::DivisionByZero;
        let msg = format!("{:?}", err);
        assert!(msg.contains("DivisionByZero"));
    }
}
```

**Step 5: Run tests**

```bash
cargo test -p flui_types --lib geometry::error
```

Expected: All tests PASS

**Step 6: Commit**

```bash
git add crates/flui_types/Cargo.toml crates/flui_types/src/geometry/error.rs crates/flui_types/src/geometry/mod.rs
git commit -m "feat(geometry): add GeometryError type

- Add thiserror dependency
- Define InvalidCoordinates, DivisionByZero, InvalidOperation
- Export from geometry module
- Add error display tests"
```

---

## Task 5: Generic Point<T> - Structure & Constructors

**Files:**
- Modify: `crates/flui_types/src/geometry/point.rs:1-100`
- Test: Create new test module at end

**Step 1: Update Point struct**

Replace existing Point definition:

```rust
use super::traits::{Unit, NumericUnit, Along, Half, Negate, IsZero, Axis};
use super::error::GeometryError;
use super::Vec2;
use std::fmt::{self, Debug, Display};
use std::ops::*;

/// Absolute position in 2D space.
///
/// Generic over unit type `T`. Common usage:
/// - `Point<Pixels>` - UI coordinates
/// - `Point<DevicePixels>` - Screen pixels
/// - `Point<f32>` - Normalized/dimensionless coordinates
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{Point, px, Pixels};
///
/// let ui_pos = Point::<Pixels>::new(px(100.0), px(200.0));
/// let normalized = Point::<f32>::new(0.5, 0.75);
/// ```
#[derive(Clone, Copy, PartialEq)]
pub struct Point<T: Unit> {
    pub x: T,
    pub y: T,
}
```

**Step 2: Add basic constructors**

```rust
impl<T: Unit> Point<T> {
    /// Creates a new point (fast, no validation).
    #[inline]
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
    
    /// Creates a point with both coordinates set to the same value.
    #[inline]
    pub fn splat(value: T) -> Self {
        Self { x: value, y: value }
    }
}
```

**Step 3: Add safe constructors**

```rust
impl<T: NumericUnit> Point<T> 
where
    T: Into<f32> + From<f32>
{
    /// Creates a point with validation (returns Result).
    pub fn try_new(x: T, y: T) -> Result<Self, GeometryError> {
        let point = Self { x, y };
        if !point.is_valid() {
            return Err(GeometryError::InvalidCoordinates {
                x: x.into(),
                y: y.into(),
            });
        }
        Ok(point)
    }
    
    /// Creates a point, clamping invalid values to valid range.
    pub fn new_clamped(x: T, y: T) -> Self {
        let clamp_f32 = |v: f32| {
            if v.is_nan() {
                0.0
            } else if v.is_infinite() {
                if v > 0.0 { f32::MAX } else { f32::MIN }
            } else {
                v
            }
        };
        
        Self {
            x: T::from(clamp_f32(x.into())),
            y: T::from(clamp_f32(y.into())),
        }
    }
}
```

**Step 4: Add validation methods**

```rust
impl<T: NumericUnit> Point<T> 
where
    T: Into<f32>
{
    /// Checks if coordinates are valid (finite, not NaN).
    pub fn is_valid(&self) -> bool {
        let x_f32: f32 = self.x.into();
        let y_f32: f32 = self.y.into();
        x_f32.is_finite() && y_f32.is_finite()
    }
    
    /// Returns true if both coordinates are finite.
    pub fn is_finite(&self) -> bool {
        self.is_valid()
    }
    
    /// Returns true if any coordinate is NaN.
    pub fn is_nan(&self) -> bool {
        let x_f32: f32 = self.x.into();
        let y_f32: f32 = self.y.into();
        x_f32.is_nan() || y_f32.is_nan()
    }
}
```

**Step 5: Write tests**

Add at end of point.rs:

```rust
#[cfg(test)]
mod typed_tests {
    use super::*;
    use crate::geometry::{Pixels, px};
    
    #[test]
    fn test_point_new() {
        let p = Point::<Pixels>::new(px(10.0), px(20.0));
        assert_eq!(p.x.0, 10.0);
        assert_eq!(p.y.0, 20.0);
    }
    
    #[test]
    fn test_point_f32() {
        let p = Point::<f32>::new(0.5, 0.75);
        assert_eq!(p.x, 0.5);
        assert_eq!(p.y, 0.75);
    }
    
    #[test]
    fn test_point_validation() {
        let valid = Point::<f32>::new(1.0, 2.0);
        assert!(valid.is_valid());
        assert!(!valid.is_nan());
        
        let invalid = Point::<f32>::new(f32::NAN, 2.0);
        assert!(!invalid.is_valid());
        assert!(invalid.is_nan());
    }
    
    #[test]
    fn test_point_try_new() {
        let result = Point::<f32>::try_new(1.0, 2.0);
        assert!(result.is_ok());
        
        let result = Point::<f32>::try_new(f32::NAN, 2.0);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_point_clamped() {
        let p = Point::<f32>::new_clamped(f32::NAN, 2.0);
        assert_eq!(p.x, 0.0);
        assert_eq!(p.y, 2.0);
        
        let p = Point::<f32>::new_clamped(f32::INFINITY, -f32::INFINITY);
        assert_eq!(p.x, f32::MAX);
        assert_eq!(p.y, f32::MIN);
    }
}
```

**Step 6: Run tests**

```bash
cargo test -p flui_types --lib geometry::point::typed_tests
```

Expected: All tests PASS

**Step 7: Commit**

```bash
git add crates/flui_types/src/geometry/point.rs
git commit -m "feat(geometry): make Point<T> generic over Unit trait

- Generic Point<T: Unit> structure
- Fast constructors: new(), splat()
- Safe constructors: try_new(), new_clamped()
- Validation: is_valid(), is_finite(), is_nan()
- Support Point<Pixels>, Point<f32>, etc.
- Comprehensive tests"
```

---

## Task 6: Point<T> - Conversions & GPU Integration

**Files:**
- Modify: `crates/flui_types/src/geometry/point.rs:100-200`

**Step 1: Add cast method**

```rust
impl<T: Unit> Point<T> {
    /// Converts point to different unit type.
    pub fn cast<U: Unit>(self) -> Point<U>
    where
        T: Into<U>
    {
        Point {
            x: self.x.into(),
            y: self.y.into(),
        }
    }
}
```

**Step 2: Add GPU conversion methods**

```rust
impl<T: NumericUnit> Point<T> 
where
    T: Into<f32>
{
    /// Converts to Point<f32> (shorthand for GPU usage).
    pub fn to_f32(self) -> Point<f32> {
        Point {
            x: self.x.into(),
            y: self.y.into(),
        }
    }
    
    /// Converts to raw array [x, y] for GPU buffers.
    pub fn to_array(self) -> [f32; 2] {
        [self.x.into(), self.y.into()]
    }
    
    /// Converts to tuple (x, y).
    pub fn to_tuple(self) -> (f32, f32) {
        (self.x.into(), self.y.into())
    }
}
```

**Step 3: Add From<Point<T>> for Point<f32>**

```rust
impl<T: Unit> From<Point<T>> for Point<f32>
where
    T: Into<f32>
{
    fn from(point: Point<T>) -> Point<f32> {
        Point {
            x: point.x.into(),
            y: point.y.into(),
        }
    }
}
```

**Step 4: Add From conversions for tuples/arrays**

```rust
impl From<(f32, f32)> for Point<f32> {
    fn from((x, y): (f32, f32)) -> Self {
        Point { x, y }
    }
}

impl From<[f32; 2]> for Point<f32> {
    fn from([x, y]: [f32; 2]) -> Self {
        Point { x, y }
    }
}

impl<T: Unit> From<Point<T>> for (f32, f32)
where
    T: Into<f32>
{
    fn from(p: Point<T>) -> (f32, f32) {
        (p.x.into(), p.y.into())
    }
}

impl<T: Unit> From<Point<T>> for [f32; 2]
where
    T: Into<f32>
{
    fn from(p: Point<T>) -> [f32; 2] {
        [p.x.into(), p.y.into()]
    }
}
```

**Step 5: Add tests**

```rust
#[test]
fn test_point_cast() {
    let p = Point::<Pixels>::new(px(100.0), px(200.0));
    let p_f32: Point<f32> = p.cast();
    assert_eq!(p_f32.x, 100.0);
    assert_eq!(p_f32.y, 200.0);
}

#[test]
fn test_point_to_f32() {
    let p = Point::<Pixels>::new(px(100.0), px(200.0));
    let p_f32 = p.to_f32();
    assert_eq!(p_f32.x, 100.0);
}

#[test]
fn test_point_to_array() {
    let p = Point::<Pixels>::new(px(100.0), px(200.0));
    let arr = p.to_array();
    assert_eq!(arr, [100.0, 200.0]);
}

#[test]
fn test_point_from_into() {
    let p = Point::<Pixels>::new(px(100.0), px(200.0));
    let p_f32: Point<f32> = p.into();
    assert_eq!(p_f32.x, 100.0);
    
    let tuple: (f32, f32) = p.into();
    assert_eq!(tuple, (100.0, 200.0));
    
    let arr: [f32; 2] = p.into();
    assert_eq!(arr, [100.0, 200.0]);
}
```

**Step 6: Run tests**

```bash
cargo test -p flui_types --lib geometry::point
```

Expected: All tests PASS

**Step 7: Commit**

```bash
git add crates/flui_types/src/geometry/point.rs
git commit -m "feat(geometry): add Point<T> conversions for GPU

- cast<U>() for explicit unit conversion
- to_f32(), to_array(), to_tuple() for GPU
- From<Point<T>> for Point<f32>
- From/Into for tuples and arrays
- Tests for all conversion paths"
```

---

## Task 7: Point<T> - Arithmetic Operations

**Files:**
- Modify: `crates/flui_types/src/geometry/point.rs:200-400`

**Step 1: Implement Add<Vec2<T>>**

```rust
impl<T: NumericUnit> Add<Vec2<T>> for Point<T> {
    type Output = Self;
    
    #[inline]
    fn add(self, rhs: Vec2<T>) -> Self::Output {
        Self {
            x: self.x.add(rhs.x),
            y: self.y.add(rhs.y),
        }
    }
}

impl<T: NumericUnit> AddAssign<Vec2<T>> for Point<T> {
    #[inline]
    fn add_assign(&mut self, rhs: Vec2<T>) {
        self.x = self.x.add(rhs.x);
        self.y = self.y.add(rhs.y);
    }
}
```

**Step 2: Implement Sub<Point<T>> -> Vec2<T>**

```rust
impl<T: NumericUnit> Sub for Point<T> {
    type Output = Vec2<T>;
    
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Vec2 {
            x: self.x.sub(rhs.x),
            y: self.y.sub(rhs.y),
        }
    }
}
```

**Step 3: Implement Sub<Vec2<T>>**

```rust
impl<T: NumericUnit> Sub<Vec2<T>> for Point<T> {
    type Output = Self;
    
    #[inline]
    fn sub(self, rhs: Vec2<T>) -> Self::Output {
        Self {
            x: self.x.sub(rhs.x),
            y: self.y.sub(rhs.y),
        }
    }
}

impl<T: NumericUnit> SubAssign<Vec2<T>> for Point<T> {
    #[inline]
    fn sub_assign(&mut self, rhs: Vec2<T>) {
        self.x = self.x.sub(rhs.x);
        self.y = self.y.sub(rhs.y);
    }
}
```

**Step 4: Implement scalar multiplication/division**

```rust
impl<T: NumericUnit> Mul<f32> for Point<T> {
    type Output = Self;
    
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x.mul(rhs),
            y: self.y.mul(rhs),
        }
    }
}

impl<T: NumericUnit> Mul<Point<T>> for f32 {
    type Output = Point<T>;
    
    #[inline]
    fn mul(self, rhs: Point<T>) -> Self::Output {
        rhs * self
    }
}

impl<T: NumericUnit> Div<f32> for Point<T> {
    type Output = Self;
    
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x.div(rhs),
            y: self.y.div(rhs),
        }
    }
}
```

**Step 5: Implement Neg**

```rust
impl<T: NumericUnit + Negate> Neg for Point<T> {
    type Output = Self;
    
    #[inline]
    fn neg(self) -> Self::Output {
        Self {
            x: self.x.negate(),
            y: self.y.negate(),
        }
    }
}
```

**Step 6: Add checked arithmetic**

```rust
impl<T: NumericUnit> Point<T> 
where
    T: Into<f32> + From<f32>
{
    /// Checked addition (returns None on invalid result).
    pub fn checked_add(self, rhs: Vec2<T>) -> Option<Self> {
        let result = Self {
            x: self.x.add(rhs.x),
            y: self.y.add(rhs.y),
        };
        
        if result.is_valid() {
            Some(result)
        } else {
            None
        }
    }
    
    /// Saturating addition (clamps to valid range).
    pub fn saturating_add(self, rhs: Vec2<T>) -> Self {
        Self::new_clamped(
            self.x.add(rhs.x),
            self.y.add(rhs.y),
        )
    }
}
```

**Step 7: Add tests**

```rust
#[test]
fn test_point_arithmetic() {
    use crate::geometry::vec2;
    
    let p = Point::<Pixels>::new(px(10.0), px(20.0));
    let v = vec2(px(5.0), px(10.0));
    
    let p2 = p + v;
    assert_eq!(p2.x.0, 15.0);
    assert_eq!(p2.y.0, 30.0);
    
    let p3 = p - v;
    assert_eq!(p3.x.0, 5.0);
    assert_eq!(p3.y.0, 10.0);
}

#[test]
fn test_point_sub_point() {
    let p1 = Point::<Pixels>::new(px(20.0), px(30.0));
    let p2 = Point::<Pixels>::new(px(10.0), px(15.0));
    
    let v = p1 - p2;
    assert_eq!(v.x.0, 10.0);
    assert_eq!(v.y.0, 15.0);
}

#[test]
fn test_point_scalar_ops() {
    let p = Point::<Pixels>::new(px(10.0), px(20.0));
    
    let p2 = p * 2.0;
    assert_eq!(p2.x.0, 20.0);
    assert_eq!(p2.y.0, 40.0);
    
    let p3 = 2.0 * p;
    assert_eq!(p3.x.0, 20.0);
    
    let p4 = p / 2.0;
    assert_eq!(p4.x.0, 5.0);
}

#[test]
fn test_point_checked_add() {
    let p = Point::<f32>::new(1.0, 2.0);
    let v = Vec2::<f32>::new(3.0, 4.0);
    
    let result = p.checked_add(v);
    assert!(result.is_some());
    assert_eq!(result.unwrap().x, 4.0);
    
    let p_invalid = Point::<f32>::new(f32::MAX, 2.0);
    let result = p_invalid.checked_add(v);
    // May be None if overflow creates invalid result
}
```

**Step 8: Run tests**

```bash
cargo test -p flui_types --lib geometry::point
```

Expected: All tests PASS

**Step 9: Commit**

```bash
git add crates/flui_types/src/geometry/point.rs
git commit -m "feat(geometry): add Point<T> arithmetic operations

- Add: Point + Vec2 = Point
- Sub: Point - Point = Vec2 (displacement)
- Sub: Point - Vec2 = Point
- Scalar ops: Point * f32, f32 * Point, Point / f32
- Negation: -Point
- Checked ops: checked_add(), saturating_add()
- Comprehensive arithmetic tests"
```

---

Due to length constraints, I'll continue with the remaining tasks (Vec2<T>, Size<T>, Offset<T>, traits implementations, and final integration) in a summary format:

## Remaining Tasks Summary

### Task 8: Generic Vec2<T> Implementation
- Make Vec2<T: Unit> generic
- Add vector operations: length(), normalize(), dot(), cross(), angle(), rotate()
- Implement arithmetic operators
- Add conversions and tests

### Task 9: Generic Size<T> Implementation
- Make Size<T: Unit> generic
- Add size-specific methods: square(), is_empty(), area(), aspect_ratio(), center()
- Implement operators
- Add conversions and tests

### Task 10: Generic Offset<T> Implementation
- Make Offset<T: Unit> generic (Flutter compatibility)
- Mirror Vec2 operations with dx/dy naming
- Implement conversions Vec2<->Offset
- Add tests

### Task 11: GPUI Utility Traits
- Implement Along trait for all geometry types
- Implement Half, Negate, IsZero for all types
- Add comprehensive trait tests

### Task 12: Constructor Functions
- Update point(), vec2(), size() to work with generics
- Add type inference tests
- Update module exports

### Task 13: Documentation & Examples
- Update module-level docs
- Add usage examples to each type
- Create examples/typed_geometry_demo.rs
- Update CHANGELOG.md

### Task 14: Integration Testing
- Test type safety (compile-fail tests)
- Test GPU conversion pipeline
- Test cross-unit conversions
- Performance benchmarks

---

**End of Plan**

Total estimated time: 4-6 hours of focused implementation.
