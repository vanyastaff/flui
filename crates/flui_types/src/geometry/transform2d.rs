//! Generic 2D Transform - Type-safe transformations with Unit system
//!
//! This module provides `Transform2D<T>` - a generic 2D transformation type
//! that works with the Unit system (Pixels, DevicePixels, etc.).
//!
//! # Design
//!
//! - **Generic over Unit**: `Transform2D<Pixels>`, `Transform2D<DevicePixels>`
//! - **Affine Transformations**: Represents 2D affine transforms efficiently
//! - **Composition**: Combine transforms with matrix multiplication
//! - **Type Safety**: Prevents mixing coordinate systems at compile-time
//!
//! # Examples
//!
//! ```
//! use flui_types::geometry::{Transform2D, Pixels, px, Point};
//!
//! // Create a translation transform
//! let translate = Transform2D::<Pixels>::translation(px(100.0), px(50.0));
//!
//! // Apply to a point
//! let point = Point::new(px(10.0), px(20.0));
//! let transformed = translate.transform_point(point);
//! assert_eq!(transformed.x, px(110.0));
//! assert_eq!(transformed.y, px(70.0));
//!
//! // Compose transforms
//! let rotate = Transform2D::<Pixels>::rotation(std::f32::consts::PI / 2.0);
//! let combined = translate.then(&rotate);
//! ```
//!
//! # Affine Transform Representation
//!
//! Internally uses a 2x3 matrix representation:
//! ```text
//! [ m11  m12  m31 ]   [ sx   shy  tx ]
//! [ m21  m22  m32 ] = [ shx  sy   ty ]
//! [  0    0    1  ]   [  0    0    1 ]
//! ```
//!
//! Where:
//! - `sx`, `sy`: Scale factors
//! - `shx`, `shy`: Shear factors
//! - `tx`, `ty`: Translation offsets

use super::{traits::Unit, Offset, Pixels, Point, Rect};
use std::marker::PhantomData;

/// A 2D affine transformation matrix, generic over Unit type.
///
/// An affine 2D transformation represented as a 2x3 matrix.
/// This can represent translation, rotation, scale, shear, and combinations thereof.
///
/// # Type Safety
///
/// The generic parameter `T` ensures transforms can only be applied to
/// compatible coordinate systems:
/// ```compile_fail
/// # use flui_types::geometry::{Transform2D, Pixels, DevicePixels, Point, px, device_px};
/// let transform_pixels = Transform2D::<Pixels>::identity();
/// let point_device = Point::new(device_px(10), device_px(20));
/// // ERROR: Cannot apply Pixels transform to DevicePixels point
/// let result = transform_pixels.transform_point(point_device);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2D<T: Unit> {
    // 2x3 affine matrix stored as f32
    // [ m11  m12  m31 ]
    // [ m21  m22  m32 ]
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub m31: f32,
    pub m32: f32,
    _phantom: PhantomData<T>,
}

impl<T: Unit> Transform2D<T> {
    // ============================================================================
    // Constructors
    // ============================================================================

    /// Creates an identity transform (no transformation).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Transform2D, Pixels, Point, px};
    ///
    /// let identity = Transform2D::<Pixels>::identity();
    /// let point = Point::new(px(10.0), px(20.0));
    /// let result = identity.transform_point(point);
    /// assert_eq!(result, point);
    /// ```
    #[inline]
    pub fn identity() -> Self {
        Self {
            m11: 1.0,
            m12: 0.0,
            m21: 0.0,
            m22: 1.0,
            m31: 0.0,
            m32: 0.0,
            _phantom: PhantomData,
        }
    }

    /// Creates a translation transform.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Transform2D, Pixels, Point, px};
    ///
    /// let translate = Transform2D::translation(50.0, 100.0);
    /// let point = Point::new(px(10.0), px(20.0));
    /// let result = translate.transform_point(point);
    /// assert_eq!(result.x, px(60.0));
    /// assert_eq!(result.y, px(120.0));
    /// ```
    #[inline]
    pub fn translation(tx: f32, ty: f32) -> Self {
        Self {
            m11: 1.0,
            m12: 0.0,
            m21: 0.0,
            m22: 1.0,
            m31: tx,
            m32: ty,
            _phantom: PhantomData,
        }
    }

    /// Creates a uniform scale transform.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Transform2D, Pixels, Point, px};
    ///
    /// let scale = Transform2D::<Pixels>::scale(2.0);
    /// let point = Point::new(px(10.0), px(20.0));
    /// let result = scale.transform_point(point);
    /// assert_eq!(result.x, px(20.0));
    /// assert_eq!(result.y, px(40.0));
    /// ```
    #[inline]
    pub fn scale(factor: f32) -> Self {
        Self {
            m11: factor,
            m12: 0.0,
            m21: 0.0,
            m22: factor,
            m31: 0.0,
            m32: 0.0,
            _phantom: PhantomData,
        }
    }

    /// Creates a non-uniform scale transform.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Transform2D, Pixels, Point, px};
    ///
    /// let scale = Transform2D::<Pixels>::scale_xy(2.0, 3.0);
    /// let point = Point::new(px(10.0), px(20.0));
    /// let result = scale.transform_point(point);
    /// assert_eq!(result.x, px(20.0));
    /// assert_eq!(result.y, px(60.0));
    /// ```
    #[inline]
    pub fn scale_xy(sx: f32, sy: f32) -> Self {
        Self {
            m11: sx,
            m12: 0.0,
            m21: 0.0,
            m22: sy,
            m31: 0.0,
            m32: 0.0,
            _phantom: PhantomData,
        }
    }

    /// Creates a rotation transform (counter-clockwise, in radians).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Transform2D, Pixels, Point, px};
    /// use std::f32::consts::PI;
    ///
    /// let rotate_90 = Transform2D::<Pixels>::rotation(PI / 2.0);
    /// let point = Point::new(px(1.0), px(0.0));
    /// let result = rotate_90.transform_point(point);
    /// // After 90° rotation, (1, 0) becomes approximately (0, 1)
    /// assert!((result.x.get()).abs() < 1e-6);
    /// assert!((result.y.get() - 1.0).abs() < 1e-6);
    /// ```
    #[inline]
    pub fn rotation(angle: f32) -> Self {
        let cos = angle.cos();
        let sin = angle.sin();
        Self {
            m11: cos,
            m12: -sin,
            m21: sin,
            m22: cos,
            m31: 0.0,
            m32: 0.0,
            _phantom: PhantomData,
        }
    }

    // ============================================================================
    // Query Operations
    // ============================================================================

    /// Checks if this is the identity transform.
    #[inline]
    pub fn is_identity(&self) -> bool {
        (self.m11 - 1.0).abs() < 1e-6
            && self.m12.abs() < 1e-6
            && self.m21.abs() < 1e-6
            && (self.m22 - 1.0).abs() < 1e-6
            && self.m31.abs() < 1e-6
            && self.m32.abs() < 1e-6
    }

    /// Checks if this transform preserves axis alignment (only translation and scale).
    #[inline]
    pub fn is_axis_aligned(&self) -> bool {
        self.m12.abs() < 1e-6 && self.m21.abs() < 1e-6
    }
}

// ============================================================================
// Specialized implementation for Pixels
// ============================================================================

impl Transform2D<Pixels> {
    /// Applies this transform to a point.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Transform2D, Pixels, Point, px};
    ///
    /// let translate = Transform2D::translation(10.0, 20.0);
    /// let point = Point::new(px(5.0), px(15.0));
    /// let result = translate.transform_point(point);
    /// assert_eq!(result.x, px(15.0));
    /// assert_eq!(result.y, px(35.0));
    /// ```
    #[inline]
    pub fn transform_point(&self, point: Point<Pixels>) -> Point<Pixels> {
        use super::px;
        Point {
            x: px(point.x.get() * self.m11 + point.y.get() * self.m21 + self.m31),
            y: px(point.x.get() * self.m12 + point.y.get() * self.m22 + self.m32),
        }
    }

    /// Applies this transform to an offset (ignores translation).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Transform2D, Pixels, Offset, px};
    ///
    /// let scale = Transform2D::<Pixels>::scale(2.0);
    /// let offset = Offset::new(px(10.0), px(20.0));
    /// let result = scale.transform_offset(offset);
    /// assert_eq!(result.dx, px(20.0));
    /// assert_eq!(result.dy, px(40.0));
    /// ```
    #[inline]
    pub fn transform_offset(&self, offset: Offset<Pixels>) -> Offset<Pixels> {
        use super::px;
        Offset {
            dx: px(offset.dx.get() * self.m11 + offset.dy.get() * self.m21),
            dy: px(offset.dx.get() * self.m12 + offset.dy.get() * self.m22),
        }
    }

    /// Applies this transform to a rectangle.
    ///
    /// Returns the axis-aligned bounding box of the transformed rectangle.
    #[inline]
    pub fn transform_rect(&self, rect: Rect<Pixels>) -> Rect<Pixels> {
        let min = self.transform_point(rect.min);
        let max = self.transform_point(rect.max);
        Rect::from_points(min, max)
    }

    /// Composes this transform with another (this transform is applied first).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Transform2D, Pixels, Point, px};
    ///
    /// let translate = Transform2D::translation(10.0, 0.0);
    /// let scale = Transform2D::<Pixels>::scale(2.0);
    /// let combined = translate.then(&scale);
    ///
    /// // First translate, then scale
    /// let point = Point::new(px(5.0), px(0.0));
    /// let result = combined.transform_point(point);
    /// assert_eq!(result.x, px(30.0));  // (5 + 10) * 2 = 30
    /// ```
    #[inline]
    pub fn then(&self, other: &Transform2D<Pixels>) -> Transform2D<Pixels> {
        Transform2D {
            m11: self.m11 * other.m11 + self.m12 * other.m21,
            m12: self.m11 * other.m12 + self.m12 * other.m22,
            m21: self.m21 * other.m11 + self.m22 * other.m21,
            m22: self.m21 * other.m12 + self.m22 * other.m22,
            m31: self.m31 * other.m11 + self.m32 * other.m21 + other.m31,
            m32: self.m31 * other.m12 + self.m32 * other.m22 + other.m32,
            _phantom: PhantomData,
        }
    }

    /// Returns the inverse transform, if it exists.
    ///
    /// Returns `None` if the transform is not invertible (determinant is zero).
    #[inline]
    pub fn inverse(&self) -> Option<Transform2D<Pixels>> {
        let det = self.m11 * self.m22 - self.m12 * self.m21;
        if det.abs() < 1e-10 {
            return None;
        }

        let inv_det = 1.0 / det;
        Some(Transform2D {
            m11: self.m22 * inv_det,
            m12: -self.m12 * inv_det,
            m21: -self.m21 * inv_det,
            m22: self.m11 * inv_det,
            m31: (self.m21 * self.m32 - self.m22 * self.m31) * inv_det,
            m32: (self.m12 * self.m31 - self.m11 * self.m32) * inv_det,
            _phantom: PhantomData,
        })
    }
}

impl<T: Unit> Default for Transform2D<T> {
    fn default() -> Self {
        Self::identity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{px, Pixels};

    #[test]
    fn test_identity() {
        let id = Transform2D::<Pixels>::identity();
        let p = Point::new(px(10.0), px(20.0));
        assert_eq!(id.transform_point(p), p);
        assert!(id.is_identity());
    }

    #[test]
    fn test_translation() {
        let t = Transform2D::translation(5.0, 10.0);
        let p = Point::new(px(1.0), px(2.0));
        let result = t.transform_point(p);
        assert_eq!(result.x, px(6.0));
        assert_eq!(result.y, px(12.0));
    }

    #[test]
    fn test_scale() {
        let s = Transform2D::<Pixels>::scale(2.0);
        let p = Point::new(px(10.0), px(20.0));
        let result = s.transform_point(p);
        assert_eq!(result.x, px(20.0));
        assert_eq!(result.y, px(40.0));
    }

    #[test]
    fn test_composition() {
        let t = Transform2D::translation(10.0, 0.0);
        let s = Transform2D::<Pixels>::scale(2.0);
        let combined = t.then(&s);

        let p = Point::new(px(5.0), px(0.0));
        let result = combined.transform_point(p);
        assert_eq!(result.x, px(30.0)); // (5 + 10) * 2
    }

    #[test]
    fn test_inverse() {
        let t = Transform2D::translation(10.0, 20.0);
        let inv = t.inverse().unwrap();

        let p = Point::new(px(15.0), px(25.0));
        let transformed = t.transform_point(p);
        let back = inv.transform_point(transformed);

        assert!((back.x.get() - p.x.get()).abs() < 1e-5);
        assert!((back.y.get() - p.y.get()).abs() < 1e-5);
    }
}
