//! Circle type.
//!
//! A circle defined by center point and radius.
//!
//! # Type Safety
//!
//! `Circle<T>` is generic over unit type `T`, preventing accidental mixing
//! of coordinate systems:
//!
//! ```ignore
//! use flui_types::geometry::{Circle, Point, Pixels, px};
//!
//! let ui_circle = Circle::<Pixels>::new(
//!     Point::new(px(50.0), px(50.0)),
//!     px(25.0)
//! );
//!
//! // Convert to f32 for GPU
//! let gpu_circle: Circle<Pixels> = ui_circle.to_f32();
//! ```

use super::{px, Pixels};
use std::fmt;

use super::traits::{NumericUnit, Unit};
use super::{Offset, Point, Radians, Rect, Size, Vec2};

/// A circle defined by a center point and radius.
///
/// Generic over unit type `T` for type-safe coordinate system handling.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle<T: Unit> {
    /// Center point.
    pub center: Point<T>,
    /// Radius (must be non-negative).
    pub radius: T,
}

// ============================================================================
// Generic Constructors
// ============================================================================

impl<T: Unit> Circle<T> {
    /// Creates a new circle with the given center and radius.
    #[must_use]
    pub const fn new(center: Point<T>, radius: T) -> Self {
        Self { center, radius }
    }

    /// Returns a copy of this circle with a new center.
    #[must_use]
    pub const fn with_center(&self, center: Point<T>) -> Self {
        Self {
            center,
            radius: self.radius,
        }
    }

    /// Returns a copy of this circle with a new radius.
    #[must_use]
    pub const fn with_radius(&self, radius: T) -> Self {
        Self {
            center: self.center,
            radius,
        }
    }

    /// Maps the circle's unit type using the provided function.
    #[must_use]
    pub fn map<U: Unit>(&self, f: impl Fn(T) -> U + Copy) -> Circle<U>
    where
        T: fmt::Debug + Default + PartialEq,
    {
        Circle {
            center: self.center.map(f),
            radius: f(self.radius),
        }
    }
}

// ============================================================================
// f32-specific Constructors
// ============================================================================

impl Circle<Pixels> {
    /// Creates a circle with the given radius centered at the origin.
    #[must_use]
    pub const fn from_radius(radius: Pixels) -> Self {
        Self {
            center: Point::ORIGIN,
            radius,
        }
    }

    /// Creates a circle from explicit center coordinates and radius.
    #[must_use]
    pub const fn from_coords(cx: Pixels, cy: Pixels, radius: Pixels) -> Self {
        Self {
            center: Point::new(cx, cy),
            radius,
        }
    }

    /// Creates the largest circle that fits inside the given rectangle.
    #[must_use]
    pub fn inscribed_in_rect(rect: Rect<Pixels>) -> Self {
        Self {
            center: rect.center(),
            radius: Pixels(rect.width().min(rect.height()).0 / 2.0),
        }
    }

    /// Creates the smallest circle that contains the given rectangle.
    #[must_use]
    pub fn circumscribed_around_rect(rect: Rect<Pixels>) -> Self {
        let center = rect.center();
        let radius = Offset::from_points(center, rect.min).distance();
        Self { center, radius }
    }
}

// ============================================================================
// Accessors (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Circle<T>
where
    T: Into<f32> + From<f32>,
{
    /// Returns the diameter of the circle (2 × radius).
    #[must_use]
    pub fn diameter(&self) -> T {
        let r: f32 = self.radius.into();
        T::from(r * 2.0)
    }

    /// Returns the circumference of the circle (2πr).
    #[must_use]
    pub fn circumference(&self) -> f32 {
        std::f32::consts::TAU * self.radius.into()
    }

    /// Returns the area of the circle (πr²).
    #[must_use]
    pub fn area(&self) -> f32 {
        let r: f32 = self.radius.into();
        std::f32::consts::PI * r * r
    }

    /// Returns the smallest axis-aligned bounds that contains this circle.
    #[must_use]
    pub fn bounding_box(&self) -> super::Bounds<T>
    where
        T: std::ops::Add<T, Output = T>
            + std::ops::Sub<T, Output = T>
            + std::ops::Div<f32, Output = T>,
    {
        let diameter = self.diameter();
        super::Bounds::centered_at(self.center, Size::new(diameter, diameter))
    }
}

// ============================================================================
// Queries (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Circle<T>
where
    T: Into<f32> + From<f32> + PartialOrd,
{
    /// Returns `true` if the circle has zero radius.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.radius.into() == 0.0
    }

    /// Returns `true` if the circle is valid (non-negative radius, finite center).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let r: f32 = self.radius.into();
        r >= 0.0 && r.is_finite() && self.center.is_finite()
    }

    /// Returns `true` if the point is inside or on the circle's boundary.
    #[must_use]
    pub fn contains(&self, point: Point<T>) -> bool {
        let r: f32 = self.radius.into();
        self.center.distance_squared(point) <= r * r
    }

    /// Returns `true` if the point is strictly inside the circle (not on boundary).
    #[must_use]
    pub fn contains_strict(&self, point: Point<T>) -> bool {
        let r: f32 = self.radius.into();
        self.center.distance_squared(point) < r * r
    }

    /// Returns `true` if this circle completely contains the other circle.
    #[must_use]
    pub fn contains_circle(&self, other: &Circle<T>) -> bool {
        let dist = self.center.distance(other.center);
        let my_r: f32 = self.radius.into();
        let other_r: f32 = other.radius.into();
        dist + other_r <= my_r
    }

    /// Returns `true` if this circle overlaps with another circle.
    #[must_use]
    pub fn overlaps(&self, other: &Circle<T>) -> bool {
        let dist_sq = self.center.distance_squared(other.center);
        let my_r: f32 = self.radius.into();
        let other_r: f32 = other.radius.into();
        let radii_sum = my_r + other_r;
        dist_sq < radii_sum * radii_sum
    }

    /// Returns the signed distance from the point to the circle boundary.
    ///
    /// Negative values indicate the point is inside the circle.
    #[must_use]
    pub fn signed_distance(&self, point: Point<T>) -> f32 {
        self.center.distance(point) - self.radius.into()
    }

    /// Returns the absolute distance from the point to the circle boundary.
    #[must_use]
    pub fn distance_to_point(&self, point: Point<T>) -> f32 {
        self.signed_distance(point).abs()
    }

    /// Returns the nearest point on the circle boundary to the given point.
    #[must_use]
    pub fn nearest_point(&self, point: Point<T>) -> Point<T> {
        let center_f32 = self.center.to_f32();
        let point_f32 = point.to_f32();

        if point_f32 == center_f32 {
            // Any point on boundary is equally close
            let r: f32 = self.radius.into();
            return Point::new(T::from(center_f32.x.0 + r), T::from(center_f32.y.0));
        }

        let dir = (point_f32 - center_f32).normalize_or(Vec2::ZERO);
        let r: f32 = self.radius.into();
        let result = center_f32 + dir * r;
        Point::new(T::from(result.x.0), T::from(result.y.0))
    }

    /// Returns the point on the circle boundary at the given angle.
    ///
    /// Angle is measured from the positive X axis, counter-clockwise.
    #[must_use]
    pub fn point_at_angle(&self, angle: Radians) -> Point<T> {
        let center_f32 = self.center.to_f32();
        let r: f32 = self.radius.into();
        Point::new(
            T::from(center_f32.x.0 + r * angle.get().cos()),
            T::from(center_f32.y.0 + r * angle.get().sin()),
        )
    }

    /// Returns the angle from the circle center to the given point.
    ///
    /// Result is in the range [-π, π].
    #[must_use]
    pub fn angle_to(&self, point: Point<T>) -> Radians {
        let center_f32 = self.center.to_f32();
        let point_f32 = point.to_f32();
        Radians::new((point_f32.y.0 - center_f32.y.0).atan2(point_f32.x.0 - center_f32.x.0))
    }
}

// ============================================================================
// Transformations (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Circle<T>
where
    T: Into<f32> + From<f32>,
{
    /// Translates the circle by the given offset vector.
    #[must_use]
    pub fn translate(&self, offset: Vec2<T>) -> Self {
        Self {
            center: self.center + offset,
            radius: self.radius,
        }
    }

    /// Scales the circle's radius by the given factor.
    #[must_use]
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            center: self.center,
            radius: T::from(self.radius.into() * factor),
        }
    }

    /// Increases the circle's radius by the given amount.
    ///
    /// The result is clamped to ensure radius remains non-negative.
    #[must_use]
    pub fn inflate(&self, amount: T) -> Self {
        let new_radius = (self.radius.into() + amount.into()).max(0.0);
        Self {
            center: self.center,
            radius: T::from(new_radius),
        }
    }

    /// Decreases the circle's radius by the given amount.
    ///
    /// Equivalent to `inflate(-amount)`.
    #[must_use]
    pub fn deflate(&self, amount: T) -> Self
    where
        T: std::ops::Neg<Output = T>,
    {
        self.inflate(-amount)
    }

    /// Linearly interpolates between this circle and another.
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let r1: f32 = self.radius.into();
        let r2: f32 = other.radius.into();
        Self {
            center: self.center.lerp(other.center, t),
            radius: T::from(r1 + (r2 - r1) * t),
        }
    }
}

// ============================================================================
// Conversions
// ============================================================================

impl<T: NumericUnit> Circle<T>
where
    T: Into<f32>,
{
    /// Converts the circle to f32-based Pixels.
    #[must_use]
    pub fn to_f32(&self) -> Circle<Pixels> {
        Circle {
            center: self.center.to_f32(),
            radius: px(self.radius.into()),
        }
    }
}

impl<T: Unit> Circle<T>
where
    T: Into<f32>,
{
    /// Converts the circle to an array [center_x, center_y, radius].
    #[must_use]
    pub fn to_array(&self) -> [f32; 3] {
        [
            self.center.x.into(),
            self.center.y.into(),
            self.radius.into(),
        ]
    }
}

// ============================================================================
// Intersections (f32 only - complex math)
// ============================================================================

impl Circle<Pixels> {
    /// Computes the intersection points between this circle and a line.
    ///
    /// Returns `None` if they don't intersect, or `Some((p1, p2))` with the two intersection points.
    #[must_use]
    pub fn intersect_line(
        &self,
        line: &super::Line<Pixels>,
    ) -> Option<(Point<Pixels>, Point<Pixels>)> {
        let d = line.to_vec();
        let f = line.p0 - self.center;

        let a = d.dot(&d);
        let b = 2.0 * f.dot(&d);
        let c = f.dot(&f) - (self.radius * self.radius).0;

        let discriminant: f32 = b * b - 4.0 * a * c;

        if discriminant < 0.0 {
            return None;
        }

        let sqrt_disc = discriminant.sqrt();
        let t1 = (-b - sqrt_disc) / (2.0 * a);
        let t2 = (-b + sqrt_disc) / (2.0 * a);

        Some((line.eval(t1), line.eval(t2)))
    }
}

// ============================================================================
// Specialized implementations for Pixels
// ============================================================================

impl Circle<super::Pixels> {
    /// Scales the circle to scaled pixels by the given factor.
    #[must_use]
    pub fn scale_to_scaled(&self, factor: f32) -> Circle<super::ScaledPixels> {
        Circle {
            center: self.center.scale(factor),
            radius: super::ScaledPixels(self.radius.get() * factor),
        }
    }
}

// ============================================================================
// Display
// ============================================================================

impl<T> fmt::Display for Circle<T>
where
    T: Unit + fmt::Display + Clone + fmt::Debug + Default + PartialEq,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Circle({}, r={})", self.center, self.radius)
    }
}

// ============================================================================
// Default
// ============================================================================

impl<T: Unit> Default for Circle<T> {
    fn default() -> Self {
        Self {
            center: Point::new(T::zero(), T::zero()),
            radius: T::zero(),
        }
    }
}
