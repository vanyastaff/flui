# Data Model: flui-types Crate

**Date**: 2026-01-26
**Branch**: `001-flui-types`
**Related**: [spec.md](spec.md), [plan.md](plan.md), [research.md](research.md)

## Purpose

This document defines the complete data model for the flui-types crate, including all types, fields, methods, and their relationships. This is the authoritative reference for implementation.

---

## Type Hierarchy Overview

```
flui_types/
├── units/
│   ├── Unit (trait)
│   ├── Pixels
│   ├── DevicePixels
│   ├── Rems
│   └── ScaledPixels
├── geometry/
│   ├── Point<T: Unit>
│   ├── Size<T: Unit>
│   ├── Rect<T: Unit>
│   ├── Offset<T: Unit>
│   ├── EdgeInsets<T: Unit>
│   └── Corners<T>
└── styling/
    ├── Color
    └── HSL (internal)
```

---

## Unit Types Module (`units/`)

### Unit Trait

**Purpose**: Marker trait for all measurement unit types. Enables generic geometric primitives.

**Definition**:
```rust
pub trait Unit: Copy + Clone + PartialEq + Debug + Default {
    /// Zero value for this unit type
    const ZERO: Self;

    /// Convert to raw f32 value
    fn to_f32(self) -> f32;

    /// Create from raw f32 value
    fn from_f32(value: f32) -> Self;

    /// Approximate equality within epsilon tolerance
    fn approx_eq(self, other: Self) -> bool {
        (self.to_f32() - other.to_f32()).abs() < EPSILON
    }
}
```

**Constants**:
```rust
/// Epsilon tolerance for floating-point equality comparisons
pub const EPSILON: f32 = 1e-6;
```

**Constraints**:
- Must be `Copy` (cheap to pass by value)
- Must be `Clone` (required for some generic contexts)
- Must be `PartialEq` (equality testing)
- Must be `Debug` (debugging and error messages)
- Must be `Default` (zero-initialization)

---

### Pixels (Logical Pixels)

**Purpose**: Device-independent layout units. Used for all UI layout calculations.

**Definition**:
```rust
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Pixels(pub f32);

impl Unit for Pixels {
    const ZERO: Self = Pixels(0.0);

    fn to_f32(self) -> f32 { self.0 }
    fn from_f32(value: f32) -> Self { Pixels(value) }
}
```

**Methods**:
```rust
impl Pixels {
    /// Create new logical pixel value
    pub const fn new(value: f32) -> Self { Pixels(value) }

    /// Convert to device pixels for rendering
    ///
    /// # Arguments
    /// * `scale_factor` - Device pixel ratio (e.g., 2.0 for Retina)
    pub fn to_device_pixels(self, scale_factor: f32) -> DevicePixels {
        DevicePixels(self.0 * scale_factor)
    }

    /// Convert to rems (font-relative units)
    ///
    /// # Arguments
    /// * `base_font_size` - Base font size in pixels (typically 16.0)
    pub fn to_rems(self, base_font_size: f32) -> Rems {
        Rems(self.0 / base_font_size)
    }
}
```

**Operators**:
```rust
impl Add for Pixels { type Output = Self; }
impl Sub for Pixels { type Output = Self; }
impl Mul<f32> for Pixels { type Output = Self; }
impl Div<f32> for Pixels { type Output = Self; }
impl Neg for Pixels { type Output = Self; }
```

**Memory Layout**: 4 bytes (f32)

---

### DevicePixels (Screen Pixels)

**Purpose**: Physical screen pixels for GPU rendering. Maps 1:1 with framebuffer pixels.

**Definition**:
```rust
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct DevicePixels(pub f32);

impl Unit for DevicePixels {
    const ZERO: Self = DevicePixels(0.0);

    fn to_f32(self) -> f32 { self.0 }
    fn from_f32(value: f32) -> Self { DevicePixels(value) }
}
```

**Methods**:
```rust
impl DevicePixels {
    /// Create new device pixel value
    pub const fn new(value: f32) -> Self { DevicePixels(value) }

    /// Convert to logical pixels for layout
    ///
    /// # Arguments
    /// * `scale_factor` - Device pixel ratio (e.g., 2.0 for Retina)
    pub fn to_logical_pixels(self, scale_factor: f32) -> Pixels {
        Pixels(self.0 / scale_factor)
    }
}
```

**Operators**: Same as Pixels

**Memory Layout**: 4 bytes (f32)

---

### Rems (Font-Relative Units)

**Purpose**: Typography-based spacing for accessible layouts. Scales with user font preferences.

**Definition**:
```rust
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Rems(pub f32);

impl Unit for Rems {
    const ZERO: Self = Rems(0.0);

    fn to_f32(self) -> f32 { self.0 }
    fn from_f32(value: f32) -> Self { Rems(value) }
}
```

**Methods**:
```rust
impl Rems {
    /// Create new rem value
    pub const fn new(value: f32) -> Self { Rems(value) }

    /// Convert to logical pixels
    ///
    /// # Arguments
    /// * `base_font_size` - Base font size in pixels (typically 16.0)
    pub fn to_pixels(self, base_font_size: f32) -> Pixels {
        Pixels(self.0 * base_font_size)
    }
}
```

**Operators**: Same as Pixels

**Memory Layout**: 4 bytes (f32)

---

### ScaledPixels (Internal Framework Use)

**Purpose**: Pre-scaling calculations for internal framework use. Not typically used in application code.

**Definition**:
```rust
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct ScaledPixels(pub f32);

impl Unit for ScaledPixels {
    const ZERO: Self = ScaledPixels(0.0);

    fn to_f32(self) -> f32 { self.0 }
    fn from_f32(value: f32) -> Self { ScaledPixels(value) }
}
```

**Methods**:
```rust
impl ScaledPixels {
    /// Create new scaled pixel value
    pub const fn new(value: f32) -> Self { ScaledPixels(value) }

    /// Convert to logical pixels
    ///
    /// # Arguments
    /// * `scale` - Scaling factor
    pub fn to_pixels(self, scale: f32) -> Pixels {
        Pixels(self.0 * scale)
    }
}
```

**Operators**: Same as Pixels

**Memory Layout**: 4 bytes (f32)

---

## Geometric Primitives Module (`geometry/`)

### Point\<T: Unit\>

**Purpose**: 2D coordinate position. Generic over unit types for type-safe calculations.

**Definition**:
```rust
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Point<T: Unit> {
    pub x: T,
    pub y: T,
}
```

**Methods**:
```rust
impl<T: Unit> Point<T> {
    /// Create new point
    pub const fn new(x: T, y: T) -> Self {
        Point { x, y }
    }

    /// Zero point (origin)
    pub const fn zero() -> Self {
        Point { x: T::ZERO, y: T::ZERO }
    }

    /// Calculate Euclidean distance to another point
    ///
    /// Performance: <10 nanoseconds per spec requirement
    pub fn distance_to(self, other: Self) -> f32 {
        let dx = self.x.to_f32() - other.x.to_f32();
        let dy = self.y.to_f32() - other.y.to_f32();
        (dx * dx + dy * dy).sqrt()
    }

    /// Offset point by delta
    pub fn offset_by(self, offset: Offset<T>) -> Self {
        Point {
            x: T::from_f32(self.x.to_f32() + offset.dx.to_f32()),
            y: T::from_f32(self.y.to_f32() + offset.dy.to_f32()),
        }
    }

    /// Approximate equality within epsilon tolerance
    pub fn approx_eq(self, other: Self) -> bool {
        self.x.approx_eq(other.x) && self.y.approx_eq(other.y)
    }
}
```

**Operators**:
```rust
// Point + Offset = Point
impl<T: Unit> Add<Offset<T>> for Point<T> { type Output = Point<T>; }

// Point - Point = Offset
impl<T: Unit> Sub<Point<T>> for Point<T> { type Output = Offset<T>; }
```

**Unit Conversions**:
```rust
impl Point<Pixels> {
    pub fn to_device_pixels(self, scale: f32) -> Point<DevicePixels> {
        Point::new(
            self.x.to_device_pixels(scale),
            self.y.to_device_pixels(scale),
        )
    }
}

impl Point<DevicePixels> {
    pub fn to_logical_pixels(self, scale: f32) -> Point<Pixels> {
        Point::new(
            self.x.to_logical_pixels(scale),
            self.y.to_logical_pixels(scale),
        )
    }
}
```

**Memory Layout**: 8 bytes (2× f32)

---

### Size\<T: Unit\>

**Purpose**: 2D dimensions (width, height). Always non-negative.

**Definition**:
```rust
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Size<T: Unit> {
    pub width: T,
    pub height: T,
}
```

**Methods**:
```rust
impl<T: Unit> Size<T> {
    /// Create new size (dimensions clamped to non-negative)
    pub fn new(width: T, height: T) -> Self {
        Size {
            width: T::from_f32(width.to_f32().max(0.0)),
            height: T::from_f32(height.to_f32().max(0.0)),
        }
    }

    /// Zero size (empty)
    pub const fn zero() -> Self {
        Size { width: T::ZERO, height: T::ZERO }
    }

    /// Check if size is empty (width or height is zero)
    pub fn is_empty(self) -> bool {
        self.width.to_f32() == 0.0 || self.height.to_f32() == 0.0
    }

    /// Calculate area
    pub fn area(self) -> f32 {
        self.width.to_f32() * self.height.to_f32()
    }

    /// Scale size by factor
    pub fn scale(self, factor: f32) -> Self {
        Size {
            width: T::from_f32(self.width.to_f32() * factor),
            height: T::from_f32(self.height.to_f32() * factor),
        }
    }

    /// Approximate equality within epsilon tolerance
    pub fn approx_eq(self, other: Self) -> bool {
        self.width.approx_eq(other.width) && self.height.approx_eq(other.height)
    }
}
```

**Unit Conversions**: Same pattern as Point

**Memory Layout**: 8 bytes (2× f32)

---

### Rect\<T: Unit\>

**Purpose**: Axis-aligned rectangle. Defined by origin point and size.

**Definition**:
```rust
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Rect<T: Unit> {
    pub origin: Point<T>,
    pub size: Size<T>,
}
```

**Methods**:
```rust
impl<T: Unit> Rect<T> {
    /// Create from origin and size
    pub const fn new(origin: Point<T>, size: Size<T>) -> Self {
        Rect { origin, size }
    }

    /// Create from left, top, width, height
    /// Negative dimensions adjust origin to preserve visual bounds
    pub fn from_ltwh(left: T, top: T, width: T, height: T) -> Self {
        let w = width.to_f32();
        let h = height.to_f32();

        if w < 0.0 || h < 0.0 {
            // Normalize: adjust origin, clamp dimensions
            let actual_left = if w < 0.0 { left.to_f32() + w } else { left.to_f32() };
            let actual_top = if h < 0.0 { top.to_f32() + h } else { top.to_f32() };

            Rect {
                origin: Point::new(
                    T::from_f32(actual_left),
                    T::from_f32(actual_top),
                ),
                size: Size::new(
                    T::from_f32(w.abs()),
                    T::from_f32(h.abs()),
                ),
            }
        } else {
            Rect {
                origin: Point::new(left, top),
                size: Size::new(width, height),
            }
        }
    }

    /// Zero rectangle (empty at origin)
    pub const fn zero() -> Self {
        Rect {
            origin: Point::zero(),
            size: Size::zero(),
        }
    }

    // Edge accessors
    pub fn left(self) -> T { self.origin.x }
    pub fn top(self) -> T { self.origin.y }
    pub fn right(self) -> T {
        T::from_f32(self.origin.x.to_f32() + self.size.width.to_f32())
    }
    pub fn bottom(self) -> T {
        T::from_f32(self.origin.y.to_f32() + self.size.height.to_f32())
    }

    /// Center point of rectangle
    pub fn center(self) -> Point<T> {
        Point::new(
            T::from_f32(self.origin.x.to_f32() + self.size.width.to_f32() / 2.0),
            T::from_f32(self.origin.y.to_f32() + self.size.height.to_f32() / 2.0),
        )
    }

    /// Check if rectangle is empty
    pub fn is_empty(self) -> bool {
        self.size.is_empty()
    }

    /// Check if point is inside rectangle (inclusive of edges)
    pub fn contains(self, point: Point<T>) -> bool {
        let px = point.x.to_f32();
        let py = point.y.to_f32();

        px >= self.left().to_f32() &&
        px <= self.right().to_f32() &&
        py >= self.top().to_f32() &&
        py <= self.bottom().to_f32()
    }

    /// Check if this rectangle overlaps with another
    pub fn intersects(self, other: Self) -> bool {
        self.left().to_f32() < other.right().to_f32() &&
        self.right().to_f32() > other.left().to_f32() &&
        self.top().to_f32() < other.bottom().to_f32() &&
        self.bottom().to_f32() > other.top().to_f32()
    }

    /// Calculate intersection rectangle
    /// Returns empty rectangle if no overlap
    ///
    /// Performance: <20 nanoseconds per spec requirement
    pub fn intersect(self, other: Self) -> Self {
        if !self.intersects(other) {
            return Rect::zero();
        }

        let left = self.left().to_f32().max(other.left().to_f32());
        let top = self.top().to_f32().max(other.top().to_f32());
        let right = self.right().to_f32().min(other.right().to_f32());
        let bottom = self.bottom().to_f32().min(other.bottom().to_f32());

        Rect::from_ltwh(
            T::from_f32(left),
            T::from_f32(top),
            T::from_f32(right - left),
            T::from_f32(bottom - top),
        )
    }

    /// Calculate union rectangle (bounding box of both)
    pub fn union(self, other: Self) -> Self {
        let left = self.left().to_f32().min(other.left().to_f32());
        let top = self.top().to_f32().min(other.top().to_f32());
        let right = self.right().to_f32().max(other.right().to_f32());
        let bottom = self.bottom().to_f32().max(other.bottom().to_f32());

        Rect::from_ltwh(
            T::from_f32(left),
            T::from_f32(top),
            T::from_f32(right - left),
            T::from_f32(bottom - top),
        )
    }

    /// Expand rectangle by distance in all directions
    pub fn inflate(self, distance: T) -> Self {
        let d = distance.to_f32();
        Rect::from_ltwh(
            T::from_f32(self.left().to_f32() - d),
            T::from_f32(self.top().to_f32() - d),
            T::from_f32(self.size.width.to_f32() + 2.0 * d),
            T::from_f32(self.size.height.to_f32() + 2.0 * d),
        )
    }

    /// Shrink rectangle by distance in all directions
    pub fn deflate(self, distance: T) -> Self {
        self.inflate(T::from_f32(-distance.to_f32()))
    }

    /// Inset rectangle by edge insets
    pub fn inset_by(self, insets: EdgeInsets<T>) -> Self {
        Rect::from_ltwh(
            T::from_f32(self.left().to_f32() + insets.left.to_f32()),
            T::from_f32(self.top().to_f32() + insets.top.to_f32()),
            T::from_f32(self.size.width.to_f32() - insets.horizontal()),
            T::from_f32(self.size.height.to_f32() - insets.vertical()),
        )
    }

    /// Approximate equality within epsilon tolerance
    pub fn approx_eq(self, other: Self) -> bool {
        self.origin.approx_eq(other.origin) && self.size.approx_eq(other.size)
    }
}
```

**Unit Conversions**: Same pattern as Point

**Memory Layout**: 16 bytes (Point + Size = 8 + 8)

---

### Offset\<T: Unit\>

**Purpose**: Displacement vector or delta between two points.

**Definition**:
```rust
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Offset<T: Unit> {
    pub dx: T,
    pub dy: T,
}
```

**Methods**:
```rust
impl<T: Unit> Offset<T> {
    /// Create new offset
    pub const fn new(dx: T, dy: T) -> Self {
        Offset { dx, dy }
    }

    /// Zero offset (no displacement)
    pub const fn zero() -> Self {
        Offset { dx: T::ZERO, dy: T::ZERO }
    }

    /// Calculate magnitude (length) of offset vector
    pub fn magnitude(self) -> f32 {
        let dx = self.dx.to_f32();
        let dy = self.dy.to_f32();
        (dx * dx + dy * dy).sqrt()
    }

    /// Normalize to unit vector
    pub fn normalized(self) -> Self {
        let mag = self.magnitude();
        if mag == 0.0 {
            return Offset::zero();
        }

        Offset {
            dx: T::from_f32(self.dx.to_f32() / mag),
            dy: T::from_f32(self.dy.to_f32() / mag),
        }
    }
}
```

**Operators**:
```rust
impl<T: Unit> Add for Offset<T> { type Output = Self; }
impl<T: Unit> Sub for Offset<T> { type Output = Self; }
impl<T: Unit> Mul<f32> for Offset<T> { type Output = Self; }
impl<T: Unit> Div<f32> for Offset<T> { type Output = Self; }
impl<T: Unit> Neg for Offset<T> { type Output = Self; }
```

**Memory Layout**: 8 bytes (2× f32)

---

### EdgeInsets\<T: Unit\>

**Purpose**: Padding, margins, and safe areas. Represents insets from each edge.

**Definition**:
```rust
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct EdgeInsets<T: Unit> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}
```

**Methods**:
```rust
impl<T: Unit> EdgeInsets<T> {
    /// Create with specific values for each edge
    pub const fn new(top: T, right: T, bottom: T, left: T) -> Self {
        EdgeInsets { top, right, bottom, left }
    }

    /// Same inset on all edges
    pub const fn all(value: T) -> Self {
        EdgeInsets {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// Symmetric insets (vertical and horizontal)
    pub const fn symmetric(vertical: T, horizontal: T) -> Self {
        EdgeInsets {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Specify only certain edges (others default to zero)
    pub fn only(
        top: Option<T>,
        right: Option<T>,
        bottom: Option<T>,
        left: Option<T>,
    ) -> Self {
        EdgeInsets {
            top: top.unwrap_or(T::ZERO),
            right: right.unwrap_or(T::ZERO),
            bottom: bottom.unwrap_or(T::ZERO),
            left: left.unwrap_or(T::ZERO),
        }
    }

    /// Total horizontal inset (left + right)
    pub fn horizontal(self) -> f32 {
        self.left.to_f32() + self.right.to_f32()
    }

    /// Total vertical inset (top + bottom)
    pub fn vertical(self) -> f32 {
        self.top.to_f32() + self.bottom.to_f32()
    }

    /// Zero insets (no padding/margin)
    pub const fn zero() -> Self {
        EdgeInsets {
            top: T::ZERO,
            right: T::ZERO,
            bottom: T::ZERO,
            left: T::ZERO,
        }
    }
}
```

**Memory Layout**: 16 bytes (4× f32)

---

### Corners\<T\>

**Purpose**: Per-corner values (e.g., corner radii for rounded rectangles).

**Definition**:
```rust
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Corners<T> {
    pub top_left: T,
    pub top_right: T,
    pub bottom_right: T,
    pub bottom_left: T,
}
```

**Methods**:
```rust
impl<T: Copy> Corners<T> {
    /// Create with specific value for each corner
    pub const fn new(top_left: T, top_right: T, bottom_right: T, bottom_left: T) -> Self {
        Corners { top_left, top_right, bottom_right, bottom_left }
    }

    /// Same value for all corners
    pub const fn all(value: T) -> Self {
        Corners {
            top_left: value,
            top_right: value,
            bottom_right: value,
            bottom_left: value,
        }
    }

    /// Top corners only
    pub const fn top(value: T) -> Self
    where T: Default {
        Corners {
            top_left: value,
            top_right: value,
            bottom_right: T::default(),
            bottom_left: T::default(),
        }
    }

    /// Bottom corners only
    pub const fn bottom(value: T) -> Self
    where T: Default {
        Corners {
            top_left: T::default(),
            top_right: T::default(),
            bottom_right: value,
            bottom_left: value,
        }
    }

    /// Specify only certain corners (others default to zero)
    pub fn only(
        top_left: Option<T>,
        top_right: Option<T>,
        bottom_right: Option<T>,
        bottom_left: Option<T>,
    ) -> Self
    where T: Default {
        Corners {
            top_left: top_left.unwrap_or_default(),
            top_right: top_right.unwrap_or_default(),
            bottom_right: bottom_right.unwrap_or_default(),
            bottom_left: bottom_left.unwrap_or_default(),
        }
    }
}
```

**Memory Layout**: Depends on T (typically 16 bytes for 4× f32)

---

## Color System Module (`styling/`)

### Color

**Purpose**: RGBA color representation with multiple blending modes and HSL conversions.

**Definition**:
```rust
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Color {
    pub r: f32,  // Red: 0.0-1.0
    pub g: f32,  // Green: 0.0-1.0
    pub b: f32,  // Blue: 0.0-1.0
    pub a: f32,  // Alpha: 0.0-1.0
}
```

**Constructors**:
```rust
impl Color {
    /// Create from RGB values (0-255), full opacity
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Color {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    /// Create from RGBA values (0-255 for RGB, 0.0-1.0 for alpha)
    pub fn from_rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Color {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a.clamp(0.0, 1.0),
        }
    }

    /// Create from hex string: "#RRGGBB" or "#RRGGBBAA"
    ///
    /// # Panics
    /// Debug builds: Panics with clear message on invalid format
    /// Release builds: Returns transparent black with warning log
    pub fn from_hex(hex: &str) -> Self {
        // Algorithm outline:
        // 1. Strip leading '#' if present
        // 2. Validate length: 6 (RGB) or 8 (RGBA) hex chars
        // 3. Parse each 2-char chunk as u8 hex: RR, GG, BB, [AA]
        // 4. Convert to f32: r = RR as f32 / 255.0
        // 5. Default alpha = 1.0 if not provided
        // Error handling:
        //   Debug: panic!("Invalid hex color '{}': expected format #RRGGBB or #RRGGBBAA", hex)
        //   Release: tracing::warn!(...); return Color::TRANSPARENT
        unimplemented!("See algorithm outline above")
    }

    /// Create from HSL values
    ///
    /// # Arguments
    /// * `h` - Hue: 0.0-360.0 degrees
    /// * `s` - Saturation: 0.0-1.0
    /// * `l` - Lightness: 0.0-1.0
    pub fn from_hsl(h: f32, s: f32, l: f32) -> Self {
        HSL { h, s, l }.to_rgb()
    }
}
```

**Blending Operations**:
```rust
impl Color {
    /// Linear interpolation between two colors
    ///
    /// # Arguments
    /// * `other` - Target color
    /// * `ratio` - Interpolation factor (0.0 = self, 1.0 = other)
    ///
    /// Performance: <20 nanoseconds per spec requirement
    pub fn mix(&self, other: &Color, ratio: f32) -> Color {
        let t = ratio.clamp(0.0, 1.0);
        Color {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }

    /// Alpha compositing (Porter-Duff Source Over)
    /// Composite this color over background
    ///
    /// Performance: <20 nanoseconds per spec requirement
    pub fn blend_over(&self, background: &Color) -> Color {
        let src_a = self.a;
        let dst_a = background.a;
        let out_a = src_a + dst_a * (1.0 - src_a);

        if out_a == 0.0 {
            return Color::TRANSPARENT;
        }

        Color {
            r: (self.r * src_a + background.r * dst_a * (1.0 - src_a)) / out_a,
            g: (self.g * src_a + background.g * dst_a * (1.0 - src_a)) / out_a,
            b: (self.b * src_a + background.b * dst_a * (1.0 - src_a)) / out_a,
            a: out_a,
        }
    }

    /// Multiply RGB values by factor
    ///
    /// # Arguments
    /// * `factor` - Scaling factor (0.0-1.0 for darkening, >1.0 for brightening)
    pub fn scale(&self, factor: f32) -> Color {
        Color {
            r: (self.r * factor).clamp(0.0, 1.0),
            g: (self.g * factor).clamp(0.0, 1.0),
            b: (self.b * factor).clamp(0.0, 1.0),
            a: self.a,
        }
    }
}
```

**Color Adjustments**:
```rust
impl Color {
    /// Lighten color by amount via HSL lightness
    ///
    /// # Arguments
    /// * `amount` - Lightness increase (0.0-1.0)
    pub fn lighten(&self, amount: f32) -> Color {
        let hsl = self.to_hsl();
        let new_l = (hsl.l + amount).clamp(0.0, 1.0);
        HSL { h: hsl.h, s: hsl.s, l: new_l }.to_rgb()
    }

    /// Darken color by amount via HSL lightness
    ///
    /// # Arguments
    /// * `amount` - Lightness decrease (0.0-1.0)
    pub fn darken(&self, amount: f32) -> Color {
        self.lighten(-amount)
    }

    /// Create new color with different opacity
    pub fn with_opacity(&self, opacity: f32) -> Color {
        Color {
            r: self.r,
            g: self.g,
            b: self.b,
            a: opacity.clamp(0.0, 1.0),
        }
    }
}
```

**Conversions**:
```rust
impl Color {
    /// Convert to RGBA tuple (0-255 for RGB, 0.0-1.0 for alpha)
    pub fn to_rgba(&self) -> (u8, u8, u8, f32) {
        (
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8,
            self.a,
        )
    }

    /// Convert to RGB tuple (0-255)
    pub fn to_rgb(&self) -> (u8, u8, u8) {
        (
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8,
        )
    }

    /// Convert to HSL representation
    pub fn to_hsl(&self) -> HSL {
        // Algorithm outline (standard RGB→HSL conversion):
        // 1. Find max and min of (r, g, b)
        // 2. Calculate lightness: L = (max + min) / 2
        // 3. If max == min: H = 0, S = 0 (grayscale)
        // 4. Else calculate saturation: S = (max - min) / (1 - |2L - 1|)
        // 5. Calculate hue based on which channel is max:
        //    - If r is max: H = 60° × ((g - b) / (max - min) mod 6)
        //    - If g is max: H = 60° × ((b - r) / (max - min) + 2)
        //    - If b is max: H = 60° × ((r - g) / (max - min) + 4)
        // 6. Return HSL { h: H, s: S, l: L }
        unimplemented!("See algorithm outline above")
    }
}
```

**Named Color Constants**:
```rust
impl Color {
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const RED: Color = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const GREEN: Color = Color { r: 0.0, g: 1.0, b: 0.0, a: 1.0 };
    pub const BLUE: Color = Color { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };
    pub const YELLOW: Color = Color { r: 1.0, g: 1.0, b: 0.0, a: 1.0 };
    pub const CYAN: Color = Color { r: 0.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const MAGENTA: Color = Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 };
}
```

**Memory Layout**: 16 bytes (4× f32)

---

### HSL (Internal Utility)

**Purpose**: Intermediate representation for HSL-based color adjustments. Not exposed in public API.

**Definition**:
```rust
#[derive(Copy, Clone, Debug)]
pub(crate) struct HSL {
    pub h: f32,  // Hue: 0.0-360.0
    pub s: f32,  // Saturation: 0.0-1.0
    pub l: f32,  // Lightness: 0.0-1.0
}

impl HSL {
    /// Convert to RGB color
    pub fn to_rgb(self) -> Color {
        // Algorithm outline (standard HSL→RGB conversion):
        // 1. If s == 0: grayscale, return Color { r: l, g: l, b: l, a: 1.0 }
        // 2. Calculate temporary values:
        //    - If l < 0.5: temp2 = l × (1 + s)
        //    - If l ≥ 0.5: temp2 = l + s - (l × s)
        //    - temp1 = 2 × l - temp2
        // 3. Normalize hue: h_norm = h / 360.0 (convert to 0-1 range)
        // 4. For each channel, calculate temp color value:
        //    - Red:   temp_r = h_norm + 1/3
        //    - Green: temp_g = h_norm
        //    - Blue:  temp_b = h_norm - 1/3
        //    - Wrap each to [0, 1]: if < 0 add 1, if > 1 subtract 1
        // 5. Apply piecewise conversion for each temp value:
        //    - If 6 × temp < 1: color = temp1 + (temp2 - temp1) × 6 × temp
        //    - Else if 2 × temp < 1: color = temp2
        //    - Else if 3 × temp < 2: color = temp1 + (temp2 - temp1) × (2/3 - temp) × 6
        //    - Else: color = temp1
        // 6. Return Color { r, g, b, a: 1.0 }
        unimplemented!("See algorithm outline above")
    }
}
```

**Memory Layout**: 12 bytes (3× f32)

---

## Type Relationships Summary

### Compile-Time Type Safety

**Valid Operations** (same unit type):
```rust
Point<Pixels> + Offset<Pixels> → Point<Pixels>  ✅
Rect<Pixels>.intersect(Rect<Pixels>) → Rect<Pixels>  ✅
```

**Invalid Operations** (mixed unit types):
```rust
Point<Pixels> + Offset<DevicePixels>  ❌ Compile error
Rect<Pixels>.intersect(Rect<DevicePixels>)  ❌ Compile error
```

### Unit Conversion Flow

```
Pixels ←→ DevicePixels (via scale_factor)
   ↕
 Rems (via base_font_size)
   ↕
ScaledPixels (via scale)
```

### Memory Efficiency

| Type | Size | Alignment | Copy-able |
|------|------|-----------|-----------|
| Pixels, DevicePixels, Rems, ScaledPixels | 4 bytes | 4 | ✅ |
| Point\<T\>, Size\<T\>, Offset\<T\> | 8 bytes | 4 | ✅ |
| Rect\<T\>, EdgeInsets\<T\>, Color | 16 bytes | 4 | ✅ |
| Corners\<T\> | 4×sizeof(T) | align(T) | ✅ (if T: Copy) |

---

## Performance Characteristics

| Operation | Target | Verification |
|-----------|--------|--------------|
| Point distance | <10ns | criterion benchmark |
| Rect intersection | <20ns | criterion benchmark |
| Color blend_over | <20ns | criterion benchmark |
| Color mix | <20ns | criterion benchmark |
| Unit conversion (const scale) | 0ns | Optimized away by compiler |
| Rect contains point | <5ns | Inline comparison |

---

## Next Steps

1. ✅ Data model documented
2. → Create API contracts (contracts/README.md)
3. → Create quickstart guide (quickstart.md)
4. → Run agent context update script
5. → Ready for `/speckit.tasks` command
