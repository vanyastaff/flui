# Type-Safe Unit-Parametrized Geometry System

**Date:** 2026-01-22  
**Status:** Design Approved  
**Author:** Claude + User  

## Overview

Redesign FLUI's geometry types (`Point`, `Vec2`, `Size`, etc.) to use unit-parametrized generics for type safety, following GPUI patterns. This prevents unit mixing bugs, adds numeric safety, and maintains zero-cost abstractions for GPU rendering.

## Goals

1. **Type Safety** - Prevent mixing incompatible units (e.g., `Pixels` + `DevicePixels`)
2. **Numeric Safety** - Guard against NaN/infinity propagation through validation
3. **GPU Integration** - Clean conversion to f32 for wgpu buffers
4. **Ergonomics** - Maintain ease of use with type inference and multiple conversion paths
5. **Zero-Cost** - No runtime overhead for type safety

## Architecture

### Unit-Parametrized Types

Core geometric types become generic over unit type `T`:

```rust
pub struct Point<T: Unit> { pub x: T, pub y: T }
pub struct Vec2<T: Unit> { pub x: T, pub y: T }
pub struct Size<T: Unit> { pub width: T, pub height: T }
pub struct Offset<T: Unit> { pub dx: T, pub dy: T }
```

### Unit Types

Following GPUI patterns, unit types are newtypes over numeric primitives:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixels(pub f32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DevicePixels(pub i32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaledPixels(pub f32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rems(pub f32);

// Constructor functions
pub const fn px(value: f32) -> Pixels { Pixels(value) }
pub const fn device_px(value: i32) -> DevicePixels { DevicePixels(value) }
pub const fn scaled_px(value: f32) -> ScaledPixels { ScaledPixels(value) }
pub const fn rems(value: f32) -> Rems { Rems(value) }
```

### Core Traits

```rust
/// Marker trait for all unit types
pub trait Unit: Copy + Clone + Debug {
    type Scalar: Copy;
    fn zero() -> Self;
}

/// Units that support arithmetic operations
pub trait NumericUnit: Unit {
    fn add(self, other: Self) -> Self;
    fn sub(self, other: Self) -> Self;
    fn mul(self, scalar: f32) -> Self;
    fn div(self, scalar: f32) -> Self;
}

/// GPUI-style utility traits
pub trait Half { fn half(self) -> Self; }
pub trait Negate { fn negate(self) -> Self; }
pub trait IsZero { fn is_zero(&self) -> bool; }

/// Axis-based operations (GPUI pattern)
pub trait Along {
    type Unit;
    fn along(&self, axis: Axis) -> Self::Unit;
    fn apply_along(&mut self, axis: Axis, f: impl FnOnce(Self::Unit) -> Self::Unit);
}
```

## Type-Safe Conversions

### GPU Integration (B + C Approach)

Three methods for converting to GPU-friendly f32:

```rust
impl<T: Unit> Point<T> {
    // Method B: Into/From traits
    // let gpu_point: Point<f32> = ui_point.into();
    
    // Method C: Explicit cast
    pub fn cast<U: Unit>(self) -> Point<U> 
    where T: Into<U>
    {
        Point { x: self.x.into(), y: self.y.into() }
    }
    
    // Shorthand for GPU
    pub fn to_f32(self) -> Point<f32> 
    where T: Into<f32>
    {
        self.cast()
    }
    
    // Direct array conversion
    pub fn to_array(self) -> [f32; 2]
    where T: Into<f32>
    {
        [self.x.into(), self.y.into()]
    }
}

// Into/From implementations
impl<T: Unit> From<Point<T>> for Point<f32> 
where T: Into<f32>
{
    fn from(point: Point<T>) -> Point<f32> {
        Point { x: point.x.into(), y: point.y.into() }
    }
}
```

### Unit-Specific Conversions

```rust
impl Point<Pixels> {
    /// Scale to device pixels with scale factor
    pub fn scale(&self, scale_factor: f32) -> Point<DevicePixels> {
        Point {
            x: DevicePixels((self.x.0 * scale_factor).round() as i32),
            y: DevicePixels((self.y.0 * scale_factor).round() as i32),
        }
    }
    
    /// Convert to scaled pixels
    pub fn to_scaled(&self, factor: f32) -> Point<ScaledPixels> {
        Point {
            x: ScaledPixels(self.x.0 * factor),
            y: ScaledPixels(self.y.0 * factor),
        }
    }
}
```

## Hybrid Safety Model

Three levels of safety for different use cases:

### Level 1: Fast Path (No Validation)

```rust
impl<T: NumericUnit> Point<T> {
    /// Creates a new point without validation (fast)
    #[inline]
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}
```

**Use case:** Hot loops, production code where inputs are trusted.

### Level 2: Safe Path (Validated)

```rust
impl<T: NumericUnit> Point<T> {
    /// Creates a point with validation (returns Result)
    pub fn try_new(x: T, y: T) -> Result<Self, GeometryError> 
    where T: Into<f32>
    {
        let point = Self { x, y };
        if !point.is_valid() {
            return Err(GeometryError::InvalidCoordinates {
                x: x.into(),
                y: y.into(),
            });
        }
        Ok(point)
    }
    
    /// Creates a point, clamping invalid values to valid range
    pub fn new_clamped(x: T, y: T) -> Self 
    where T: Into<f32> + From<f32>
    {
        let clamp_f32 = |v: f32| {
            if v.is_nan() { 0.0 }
            else if v.is_infinite() { 
                if v > 0.0 { f32::MAX } else { f32::MIN }
            }
            else { v }
        };
        
        Self {
            x: T::from(clamp_f32(x.into())),
            y: T::from(clamp_f32(y.into())),
        }
    }
}
```

**Use case:** Parsing user input, external data, untrusted sources.

### Level 3: Validation Helpers

```rust
impl<T: NumericUnit> Point<T> {
    /// Checks if coordinates are valid (finite, not NaN)
    pub fn is_valid(&self) -> bool 
    where T: Into<f32>
    {
        let x_f32: f32 = self.x.into();
        let y_f32: f32 = self.y.into();
        x_f32.is_finite() && y_f32.is_finite()
    }
    
    pub fn is_finite(&self) -> bool where T: Into<f32> {
        self.is_valid()
    }
    
    pub fn is_nan(&self) -> bool where T: Into<f32> {
        let x_f32: f32 = self.x.into();
        let y_f32: f32 = self.y.into();
        x_f32.is_nan() || y_f32.is_nan()
    }
}
```

### Checked Arithmetic

```rust
impl<T: NumericUnit> Point<T> {
    /// Checked addition (returns None on overflow/invalid result)
    pub fn checked_add(self, rhs: Vec2<T>) -> Option<Self> 
    where T: Into<f32> + From<f32>
    {
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
    
    /// Saturating addition (clamps to valid range)
    pub fn saturating_add(self, rhs: Vec2<T>) -> Self 
    where T: Into<f32> + From<f32>
    {
        Self::new_clamped(
            self.x.add(rhs.x),
            self.y.add(rhs.y),
        )
    }
}
```

### Debug Assertions

```rust
impl<T: NumericUnit> Point<T> {
    #[inline]
    fn debug_assert_valid(&self) 
    where T: Into<f32>
    {
        debug_assert!(
            self.is_valid(),
            "Point coordinates must be finite: {:?}",
            self
        );
    }
}
```

## Error Handling

```rust
#[derive(Debug, Clone, thiserror::Error)]
pub enum GeometryError {
    #[error("Invalid coordinates: ({x}, {y}) - must be finite")]
    InvalidCoordinates { x: f32, y: f32 },
    
    #[error("Division by zero")]
    DivisionByZero,
    
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}
```

## Arithmetic Operations

Following semantic correctness:

```rust
// Point + Vec2 = Point (translate point)
impl<T: NumericUnit> Add<Vec2<T>> for Point<T> {
    type Output = Self;
    fn add(self, rhs: Vec2<T>) -> Self::Output { ... }
}

// Point - Point = Vec2 (displacement between points)
impl<T: NumericUnit> Sub for Point<T> {
    type Output = Vec2<T>;
    fn sub(self, rhs: Self) -> Self::Output { ... }
}

// Point - Vec2 = Point (translate backwards)
impl<T: NumericUnit> Sub<Vec2<T>> for Point<T> {
    type Output = Self;
    fn sub(self, rhs: Vec2<T>) -> Self::Output { ... }
}

// Point * f32 (scalar multiplication)
impl<T: NumericUnit> Mul<f32> for Point<T> {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self::Output { ... }
}

// f32 * Point (reverse)
impl<T: NumericUnit> Mul<Point<T>> for f32 {
    type Output = Point<T>;
    fn mul(self, rhs: Point<T>) -> Self::Output { ... }
}

// Point / f32 (scalar division)
impl<T: NumericUnit> Div<f32> for Point<T> {
    type Output = Self;
    fn div(self, rhs: f32) -> Self::Output { ... }
}

// -Point (negation)
impl<T: NumericUnit + Negate> Neg for Point<T> {
    type Output = Self;
    fn neg(self) -> Self::Output { ... }
}
```

## Vector-Specific Operations

```rust
impl<T: NumericUnit> Vec2<T> 
where T: Into<f32> + From<f32>
{
    pub fn length(&self) -> f32 { ... }
    pub fn length_squared(&self) -> f32 { ... }
    pub fn normalize(&self) -> Vec2<f32> { ... }
    pub fn dot(&self, other: &Self) -> f32 { ... }
    pub fn cross(&self, other: &Self) -> f32 { ... }
    pub fn angle(&self) -> f32 { ... }
    pub fn rotate(&self, angle: f32) -> Self { ... }
}
```

## Size-Specific Operations

```rust
impl<T: NumericUnit> Size<T> 
where T: Into<f32> + From<f32>
{
    pub fn square(side: T) -> Self { ... }
    pub fn is_empty(&self) -> bool where T: IsZero { ... }
    pub fn area(&self) -> f32 { ... }
    pub fn aspect_ratio(&self) -> f32 { ... }
    pub fn center(&self) -> Point<T> where T: Half { ... }
    pub fn contains(&self, point: Point<T>) -> bool { ... }
}
```

## Migration Strategy

### No Legacy Support - Clean Break

This is a breaking change requiring major version bump. No deprecated types, no legacy aliases.

### Phase 1: Core Types (Current Phase)
- ✅ `Point<T>`, `Vec2<T>`, `Size<T>`, `Offset<T>`
- ✅ Unit types: `Pixels`, `DevicePixels`, `ScaledPixels`, `Rems`
- ✅ Core traits: `Unit`, `NumericUnit`, `Along`, `Half`, `Negate`, `IsZero`
- ✅ Conversions & safety model
- ✅ Complete test suite

### Phase 2: Containers
- `Rect<T>`, `Bounds<T>`, `Line<T>`, `Circle<T>`
- These types use `Point<T>`, `Size<T>` internally

### Phase 3: Layout & Helpers
- `Edges<T>`, `Corners<T>`, `Corner` enum
- Layout-specific operations

### Phase 4: Advanced (Optional)
- `Matrix4<T>`, `Transform<T>`
- Bezier curves with units
- `RRect<T>`, `RSuperellipse<T>`

### Code Migration Examples

#### Before (old API):
```rust
let point = Point::new(100.0, 200.0);
let size = Size::new(400.0, 300.0);
```

#### After (new API):
```rust
// Explicit units (type-safe)
let point = Point::<Pixels>::new(px(100.0), px(200.0));
let size = Size::<Pixels>::new(px(400.0), px(300.0));

// Or with type inference
let point: Point<Pixels> = Point::new(px(100.0), px(200.0));

// Raw f32 when units don't matter
let normalized = Point::<f32>::new(0.5, 0.5);
```

#### GPU Conversion:
```rust
let ui_point = Point::<Pixels>::new(px(100.0), px(200.0));

// Method 1: Into
let gpu_point: Point<f32> = ui_point.into();

// Method 2: Explicit cast
let gpu_point = ui_point.cast::<f32>();

// Method 3: Direct to array
let vertex_pos = ui_point.to_array();  // [f32; 2]
```

## Usage Examples

### Example 1: UI Layout
```rust
use flui_types::geometry::*;

fn layout_button(position: Point<Pixels>, size: Size<Pixels>) -> Bounds<Pixels> {
    Bounds { origin: position, size }
}

let button_pos = Point::new(px(10.0), px(20.0));
let button_size = Size::new(px(100.0), px(40.0));
let button_bounds = layout_button(button_pos, button_size);
```

### Example 2: GPU Rendering
```rust
fn render_quad(bounds: Bounds<Pixels>, scale_factor: f32) {
    // Convert UI coordinates to device pixels
    let device_bounds = bounds.scale(scale_factor);
    
    // Convert to GPU coordinates (f32)
    let gpu_origin: Point<f32> = device_bounds.origin.into();
    let gpu_size: Size<f32> = device_bounds.size.into();
    
    // Build vertex buffer
    let vertices = [
        gpu_origin.to_array(),
        (gpu_origin + vec2(gpu_size.width, 0.0)).to_array(),
        (gpu_origin + vec2(gpu_size.width, gpu_size.height)).to_array(),
        (gpu_origin + vec2(0.0, gpu_size.height)).to_array(),
    ];
    
    // Send to wgpu...
}
```

### Example 3: Type-Safe Hit Testing
```rust
fn handle_mouse_event(
    mouse_pos: Point<Pixels>, 
    widget_bounds: Bounds<Pixels>
) -> bool {
    widget_bounds.contains(mouse_pos)
}

// Type safety prevents:
// let device_mouse = Point::<DevicePixels>::new(...);
// handle_mouse_event(device_mouse, widget_bounds); // ❌ Won't compile!

// Must explicitly convert:
let pixel_mouse = device_mouse.cast::<Pixels>();
handle_mouse_event(pixel_mouse, widget_bounds); // ✅
```

### Example 4: Animation with Safety
```rust
struct Animation {
    position: Point<Pixels>,
    velocity: Vec2<Pixels>,
}

impl Animation {
    fn update(&mut self, delta_time: f32) {
        // Checked arithmetic for safety
        if let Some(new_pos) = self.position.checked_add(self.velocity * delta_time) {
            self.position = new_pos;
        } else {
            // Handle overflow/invalid result
            tracing::warn!("Animation position overflow");
        }
    }
}
```

### Example 5: Safe Parsing
```rust
fn parse_position(x_str: &str, y_str: &str) -> Result<Point<Pixels>, GeometryError> {
    let x = x_str.parse::<f32>()
        .map_err(|_| GeometryError::InvalidOperation("Invalid x".into()))?;
    let y = y_str.parse::<f32>()
        .map_err(|_| GeometryError::InvalidOperation("Invalid y".into()))?;
    
    // Use safe constructor
    Point::try_new(px(x), px(y))
}
```

### Example 6: Coordinate Space Conversions
```rust
fn world_to_screen(
    world_pos: Point<f32>,
    camera_offset: Vec2<Pixels>,
    zoom: f32,
) -> Point<Pixels> {
    // Convert world coordinates to pixels
    let screen_x = px(world_pos.x * zoom);
    let screen_y = px(world_pos.y * zoom);
    
    Point::new(screen_x, screen_y) + camera_offset
}
```

## Benefits

### Type Safety
- ✅ Cannot accidentally mix `Pixels` and `DevicePixels`
- ✅ Compile-time enforcement of unit conversions
- ✅ Clear intent in function signatures
- ✅ Self-documenting code

### Numeric Safety
- ✅ Validation methods prevent NaN/infinity bugs
- ✅ Checked arithmetic for critical paths
- ✅ Debug assertions catch issues early
- ✅ Multiple safety levels for different use cases

### GPU Integration
- ✅ Clean conversion to f32 for wgpu
- ✅ Zero-cost abstractions (newtype pattern)
- ✅ `#[repr(transparent)]` for unit types
- ✅ Multiple conversion paths (Into, cast, to_array)

### Ergonomics
- ✅ Type inference works well
- ✅ GPUI-compatible patterns (proven in production)
- ✅ Familiar API for geometry operations
- ✅ Constructor functions (px, device_px, etc.)

### Maintainability
- ✅ Clear semantic meaning of coordinates
- ✅ Easy to refactor
- ✅ Prevents entire classes of bugs
- ✅ Better IDE support and error messages

## Testing Strategy

### Unit Tests
```rust
#[test]
fn test_type_safety() {
    let p1 = Point::<Pixels>::new(px(100.0), px(200.0));
    let p2 = Point::<DevicePixels>::new(device_px(800), device_px(600));
    
    // This should NOT compile:
    // let bad = p1 + Vec2::from(p2);
    
    // Explicit conversion required:
    let p2_as_pixels: Point<Pixels> = p2.cast();
    let good = p1 + Vec2::from(p2_as_pixels);
}

#[test]
fn test_numeric_safety() {
    // NaN handling
    let invalid = Point::<f32>::new(f32::NAN, 100.0);
    assert!(!invalid.is_valid());
    
    // Safe constructor
    assert!(Point::<f32>::try_new(f32::NAN, 100.0).is_err());
    
    // Clamped constructor
    let clamped = Point::<f32>::new_clamped(f32::NAN, 100.0);
    assert_eq!(clamped.x, 0.0);
    assert_eq!(clamped.y, 100.0);
}

#[test]
fn test_conversions() {
    let ui = Point::<Pixels>::new(px(100.0), px(200.0));
    
    // To f32
    let raw: Point<f32> = ui.into();
    assert_eq!(raw.x, 100.0);
    
    // To array
    let arr = ui.to_array();
    assert_eq!(arr, [100.0, 200.0]);
    
    // Scaling
    let device = ui.scale(2.0);
    assert_eq!(device.x.0, 200);
}
```

### Integration Tests
- Test with actual rendering pipeline
- Test coordinate conversions throughout stack
- Test performance (ensure zero-cost)
- Test error propagation

## Implementation Checklist

### Phase 1 Tasks

- [ ] **Traits** (`flui_types/src/geometry/traits.rs`)
  - [ ] `Unit`, `NumericUnit` traits
  - [ ] `Half`, `Negate`, `IsZero` traits
  - [ ] `Along` trait with `Axis` enum

- [ ] **Unit Types** (`flui_types/src/geometry/units.rs`)
  - [ ] `Pixels`, `DevicePixels`, `ScaledPixels`, `Rems`
  - [ ] Implement all traits for each unit
  - [ ] Arithmetic operators
  - [ ] Conversions (Into<f32>, From<f32>)

- [ ] **Core Types**
  - [ ] `Point<T>` with all methods
  - [ ] `Vec2<T>` with vector operations
  - [ ] `Size<T>` with size operations
  - [ ] `Offset<T>` (Flutter compat)

- [ ] **Error Handling**
  - [ ] `GeometryError` enum
  - [ ] Error messages and Display impl

- [ ] **Tests**
  - [ ] Unit tests for all types
  - [ ] Type safety tests (compile-fail tests)
  - [ ] Numeric safety tests
  - [ ] Conversion tests
  - [ ] Performance benchmarks

- [ ] **Documentation**
  - [ ] Update module-level docs
  - [ ] Add examples to each type
  - [ ] Migration guide
  - [ ] CHANGELOG entry

## Performance Considerations

- **Zero-cost abstractions**: Unit types are `#[repr(transparent)]` newtypes
- **Inlining**: All hot-path methods marked `#[inline]`
- **No allocations**: All operations on stack
- **SIMD-ready**: Compatible with future SIMD optimizations
- **Debug assertions only**: Validation only in debug builds when needed

## Future Enhancements

### Phase 2+
- Generic bounds and rects
- Layout types with units
- Transform matrices with units

### Potential Additions
- SIMD acceleration using platform intrinsics
- More unit types (ViewportPixels, TexturePixels)
- Coordinate space markers (ScreenSpace, WorldSpace) - euclid-style
- Integration with physics engines

## Conclusion

This design provides a solid foundation for type-safe geometry in FLUI while maintaining performance and ergonomics. The GPUI-inspired approach has been proven in production, and the hybrid safety model gives flexibility for different use cases.

Next step: Begin Phase 1 implementation.
