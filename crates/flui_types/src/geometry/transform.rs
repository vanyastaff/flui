//! 2D Transform API - High-level, type-safe transformations
//!
//! Provides an ergonomic API for working with 2D transformations that compiles
//! down to efficient Matrix4 operations.
//!
//! # Philosophy
//!
//! This API follows Flutter's transform philosophy:
//! - **Declarative**: Express what you want, not how to build the matrix
//! - **Composable**: Chain transforms with `.then()` for readability
//! - **Type-Safe**: Compile-time guarantees about transform correctness
//! - **Zero-Cost**: Compiles to direct Matrix4 operations
//!
//! # Examples
//!
//! ## Basic Transforms
//!
//! ```rust,ignore
//! use flui_types::geometry::{Transform, Matrix4, Offset};
//! use std::f32::consts::PI;
//!
//! // Translation - move by offset
//! let t = Transform::translate(50.0, 100.0);
//! let t = Transform::translate_offset(Offset::new(50.0, 100.0));
//!
//! // Rotation - spin around origin
//! let t = Transform::rotate(PI / 4.0);          // Radians
//! let t = Transform::rotate_degrees(45.0);      // Degrees
//!
//! // Scale - resize uniformly or independently
//! let t = Transform::scale(2.0);                // Uniform 2x
//! let t = Transform::scale_xy(2.0, 3.0);        // 2x width, 3x height
//! ```
//!
//! ## Skew Transforms (Shear)
//!
//! ```rust,ignore
//! // Italic text effect - horizontal shear
//! let italic = Transform::skew(0.2, 0.0);
//! // Result: text leans right at ~11.3° angle
//!
//! // Perspective effect - both axes
//! let perspective = Transform::skew(0.3, 0.3);
//!
//! // Trapezoid shape - for title bars, buttons
//! let trapezoid = Transform::skew(0.15, 0.0);
//! ```
//!
//! ## Pivot Point Transforms
//!
//! ```rust,ignore
//! // Rotate button around its center (not origin)
//! let button_center = (100.0, 50.0);
//! let t = Transform::rotate_around(
//!     PI / 4.0,              // 45° rotation
//!     button_center.0,       // pivot X
//!     button_center.1,       // pivot Y
//! );
//!
//! // Scale icon around its center
//! let icon_center = (64.0, 64.0);
//! let t = Transform::scale_around(
//!     2.0, 2.0,              // 2x scale
//!     icon_center.0,         // pivot X
//!     icon_center.1,         // pivot Y
//! );
//! ```
//!
//! ## Composition (Fluent API)
//!
//! ```rust,ignore
//! // Transforms applied left-to-right: translate → rotate → scale
//! let transform = Transform::translate(50.0, 50.0)
//!     .then(Transform::rotate(PI / 4.0))
//!     .then(Transform::scale(2.0));
//!
//! // Automatic optimization - Identity is eliminated
//! let t = Transform::translate(10.0, 10.0)
//!     .then(Transform::Identity);  // No-op, removed
//!
//! // Nested compositions are flattened
//! let t1 = Transform::translate(10.0, 10.0)
//!     .then(Transform::rotate(PI / 4.0));
//! let t2 = Transform::scale(2.0)
//!     .then(Transform::skew(0.1, 0.0));
//! let combined = t1.then(t2);  // Single Compose with 4 transforms
//! ```
//!
//! ## Conversion to Matrix4 (Idiomatic Rust)
//!
//! ```rust,ignore
//! let transform = Transform::rotate(PI / 4.0);
//!
//! // Owned conversion (consumes transform)
//! let matrix: Matrix4 = transform.into();
//!
//! // Reference conversion (borrows, transform still usable)
//! let transform = Transform::rotate(PI / 4.0);
//! let matrix: Matrix4 = (&transform).into();
//! let matrix2: Matrix4 = transform.into();  // Can still use it
//!
//! // Backward compatible method
//! let transform = Transform::scale(2.0);
//! let matrix = transform.to_matrix();
//! ```
//!
//! ## Query Transform Properties
//!
//! ```rust,ignore
//! let transform = Transform::translate(10.0, 20.0)
//!     .then(Transform::rotate(PI / 4.0))
//!     .then(Transform::scale(2.0))
//!     .then(Transform::skew(0.1, 0.0));
//!
//! assert!(transform.has_translation());
//! assert!(transform.has_rotation());
//! assert!(transform.has_scale());
//! assert!(transform.has_skew());
//! assert!(!transform.is_identity());
//! ```
//!
//! ## Inverse Transforms
//!
//! ```rust,ignore
//! // For hit testing, reverse animations
//! let forward = Transform::translate(100.0, 50.0)
//!     .then(Transform::rotate(PI / 4.0));
//!
//! let backward = forward.inverse().unwrap();
//!
//! // Non-invertible transforms return None
//! let t = Transform::scale(0.0);  // Scale by zero
//! assert!(t.inverse().is_none());
//! ```
//!
//! ## Real-World Use Cases
//!
//! ```rust,ignore
//! // UI Container with zoom and pan
//! let container_transform = Transform::translate(pan_x, pan_y)
//!     .then(Transform::scale(zoom_level));
//!
//! // Animated rotating loader
//! let angle = time * 2.0 * PI;  // Full rotation per second
//! let loader = Transform::rotate_around(
//!     angle,
//!     center_x, center_y,
//! );
//!
//! // Card flip animation (with perspective)
//! let flip_angle = lerp(0.0, PI, progress);
//! let card = Transform::rotate_around(flip_angle, card_center_x, card_center_y)
//!     .then(Transform::skew(0.2 * progress, 0.0));  // Add perspective
//!
//! // Text on curved path (with rotation and offset)
//! for (i, ch) in text.chars().enumerate() {
//!     let angle = i as f32 * 0.1;
//!     let t = Transform::rotate(angle)
//!         .then(Transform::translate(i as f32 * 10.0, curve_height));
//!     // Draw character with transform
//! }
//! ```

use super::{Matrix4, Offset, Pixels};
use std::f32::consts::PI;

/// High-level 2D transformation API
///
/// Represents common 2D transformations in a type-safe, composable way.
/// Each variant compiles down to efficient Matrix4 operations.
///
/// # Transform Order
///
/// When composing transforms with `.then()`, they are applied in the order specified:
///
/// ```rust,ignore
/// // Translate THEN rotate THEN scale
/// Transform::translate(10.0, 10.0)
///     .then(Transform::rotate(PI / 4.0))
///     .then(Transform::scale(2.0, 2.0))
/// ```
///
/// This matches Flutter's transform semantics and Canvas2D API.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Transform {
    /// Identity transform (no transformation)
    #[default]
    Identity,

    /// Translation by (x, y) offset
    Translate {
        /// X-axis translation offset
        x: f32,
        /// Y-axis translation offset
        y: f32,
    },

    /// Rotation by angle in radians (counter-clockwise)
    Rotate {
        /// Rotation angle in radians (counter-clockwise)
        angle: f32,
    },

    /// Uniform scale (same factor for X and Y)
    Scale {
        /// Scale factor (applies to both X and Y axes)
        factor: f32,
    },

    /// Non-uniform scale (different factors for X and Y)
    ScaleXY {
        /// X-axis scale factor
        x: f32,
        /// Y-axis scale factor
        y: f32,
    },

    /// Skew transform (shear along X and Y axes)
    ///
    /// Common use cases:
    /// - Italic text: `Skew { x: 0.2, y: 0.0 }`
    /// - Perspective: `Skew { x: 0.3, y: 0.3 }`
    Skew {
        /// Skew angle along X-axis in radians (horizontal shear)
        x: f32,
        /// Skew angle along Y-axis in radians (vertical shear)
        y: f32,
    },

    /// Rotation around a specific pivot point
    ///
    /// Equivalent to: translate(-pivot) → rotate(angle) → translate(pivot)
    RotateAround {
        /// Rotation angle in radians (counter-clockwise)
        angle: f32,
        /// Pivot point X coordinate
        pivot_x: f32,
        /// Pivot point Y coordinate
        pivot_y: f32,
    },

    /// Scale around a specific pivot point
    ///
    /// Equivalent to: translate(-pivot) → scale(x, y) → translate(pivot)
    ScaleAround {
        /// X-axis scale factor
        x: f32,
        /// Y-axis scale factor
        y: f32,
        /// Pivot point X coordinate
        pivot_x: f32,
        /// Pivot point Y coordinate
        pivot_y: f32,
    },

    /// Composition of multiple transforms (applied in order)
    ///
    /// Transforms are applied left-to-right (first to last).
    Compose(Vec<Transform>),

    /// Raw matrix transform (escape hatch for complex cases)
    ///
    /// Use this when you need full control or have a precomputed matrix.
    /// For most cases, prefer the typed variants above.
    Matrix(Matrix4),
}

impl Transform {
    // ===== Convenience Constructors =====

    /// Create a translation transform
    #[inline]
    pub fn translate(x: f32, y: f32) -> Self {
        Self::Translate { x, y }
    }

    /// Create a translation transform from an Offset
    #[inline]
    pub fn translate_offset(offset: Offset<Pixels>) -> Self {
        Self::Translate {
            x: offset.dx.0,
            y: offset.dy.0,
        }
    }

    /// Create a rotation transform (angle in radians, counter-clockwise)
    #[inline]
    pub fn rotate(angle: f32) -> Self {
        Self::Rotate { angle }
    }

    /// Create a rotation transform (type-safe version).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Transform, Radians};
    ///
    /// let t = Transform::rotate_radians(Radians::from_degrees(45.0));
    /// ```
    #[inline]
    pub fn rotate_radians(angle: crate::geometry::Radians) -> Self {
        Self::Rotate { angle: angle.0 }
    }

    /// Create a rotation transform (angle in degrees, counter-clockwise)
    #[inline]
    pub fn rotate_degrees(degrees: f32) -> Self {
        Self::Rotate {
            angle: degrees * PI / 180.0,
        }
    }

    /// Create a uniform scale transform (same factor for X and Y)
    #[inline]
    pub fn scale(factor: f32) -> Self {
        Self::Scale { factor }
    }

    /// Create a non-uniform scale transform (different factors for X and Y)
    #[inline]
    pub fn scale_xy(x: f32, y: f32) -> Self {
        Self::ScaleXY { x, y }
    }

    /// Create a skew transform (shear along X and Y axes)
    ///
    /// Angles in radians. For italic text, use `skew(0.2, 0.0)`.
    #[inline]
    pub fn skew(x: f32, y: f32) -> Self {
        Self::Skew { x, y }
    }

    /// Create a rotation around a pivot point
    #[inline]
    pub fn rotate_around(angle: f32, pivot_x: f32, pivot_y: f32) -> Self {
        Self::RotateAround {
            angle,
            pivot_x,
            pivot_y,
        }
    }

    /// Create a rotation around a pivot point (type-safe version).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Transform, Radians};
    ///
    /// let t = Transform::rotate_around_radians(
    ///     Radians::from_degrees(45.0),
    ///     100.0,
    ///     100.0
    /// );
    /// ```
    #[inline]
    pub fn rotate_around_radians(
        angle: crate::geometry::Radians,
        pivot_x: f32,
        pivot_y: f32,
    ) -> Self {
        Self::RotateAround {
            angle: angle.0,
            pivot_x,
            pivot_y,
        }
    }

    /// Create a scale around a pivot point
    #[inline]
    pub fn scale_around(x: f32, y: f32, pivot_x: f32, pivot_y: f32) -> Self {
        Self::ScaleAround {
            x,
            y,
            pivot_x,
            pivot_y,
        }
    }

    /// Create identity transform
    #[inline]
    pub fn identity() -> Self {
        Self::Identity
    }

    /// Create from a raw Matrix4
    #[inline]
    pub fn from_matrix(matrix: Matrix4) -> Self {
        Self::Matrix(matrix)
    }

    // ===== Composition API =====

    /// Chain this transform with another (applies this transform THEN other)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let transform = Transform::translate(10.0, 10.0)
    ///     .then(Transform::rotate(PI / 4.0))
    ///     .then(Transform::scale(2.0));
    /// ```
    #[inline]
    pub fn then(self, other: Transform) -> Self {
        match (self, other) {
            // Identity optimizations
            (Transform::Identity, other) => other,
            (this, Transform::Identity) => this,

            // Flatten nested compositions
            (Transform::Compose(mut transforms), Transform::Compose(mut other_transforms)) => {
                transforms.append(&mut other_transforms);
                Transform::Compose(transforms)
            }
            (Transform::Compose(mut transforms), other) => {
                transforms.push(other);
                Transform::Compose(transforms)
            }
            (this, Transform::Compose(other_transforms)) => {
                let mut transforms = Vec::with_capacity(1 + other_transforms.len());
                transforms.push(this);
                transforms.extend(other_transforms);
                Transform::Compose(transforms)
            }

            // Build new composition
            (this, other) => Transform::Compose(vec![this, other]),
        }
    }

    /// Convenience alias for `then()` for chaining
    #[inline]
    pub fn and_then(self, other: Transform) -> Self {
        self.then(other)
    }

    // ===== Internal Matrix Conversion =====

    /// Internal method for matrix conversion
    ///
    /// Use `Into<Matrix4>` trait instead: `let matrix: Matrix4 = transform.into();`
    fn to_matrix_internal(&self) -> Matrix4 {
        match self {
            Transform::Identity => Matrix4::identity(),

            Transform::Translate { x, y } => Matrix4::translation(*x, *y, 0.0),

            Transform::Rotate { angle } => Matrix4::rotation_z(*angle),

            Transform::Scale { factor } => Matrix4::scaling(*factor, *factor, 1.0),

            Transform::ScaleXY { x, y } => Matrix4::scaling(*x, *y, 1.0),

            Transform::Skew { x, y } => {
                // Skew matrix:
                // [ 1      tan(y)  0  0 ]
                // [ tan(x) 1       0  0 ]
                // [ 0      0       1  0 ]
                // [ 0      0       0  1 ]
                let mut matrix = Matrix4::identity();
                matrix.m[4] = y.tan(); // m[1][0] = tan(y)
                matrix.m[1] = x.tan(); // m[0][1] = tan(x)
                matrix
            }

            Transform::RotateAround {
                angle,
                pivot_x,
                pivot_y,
            } => {
                // translate(-pivot) * rotate(angle) * translate(pivot)
                Matrix4::translation(*pivot_x, *pivot_y, 0.0)
                    * Matrix4::rotation_z(*angle)
                    * Matrix4::translation(-pivot_x, -pivot_y, 0.0)
            }

            Transform::ScaleAround {
                x,
                y,
                pivot_x,
                pivot_y,
            } => {
                // translate(-pivot) * scale(x, y) * translate(pivot)
                Matrix4::translation(*pivot_x, *pivot_y, 0.0)
                    * Matrix4::scaling(*x, *y, 1.0)
                    * Matrix4::translation(-pivot_x, -pivot_y, 0.0)
            }

            Transform::Compose(transforms) => {
                // Apply transforms left-to-right (multiply matrices right-to-left)
                transforms
                    .iter()
                    .map(|t| t.to_matrix_internal())
                    .fold(Matrix4::identity(), |acc, matrix| acc * matrix)
            }

            Transform::Matrix(matrix) => *matrix,
        }
    }

    /// Convenience method for backward compatibility
    ///
    /// Prefer using `.into()` trait: `let matrix: Matrix4 = transform.into();`
    #[inline]
    pub fn to_matrix(&self) -> Matrix4 {
        self.to_matrix_internal()
    }

    // ===== Query Methods =====

    /// Check if this is an identity transform (no transformation)
    #[inline]
    pub fn is_identity(&self) -> bool {
        match self {
            Transform::Identity => true,
            Transform::Matrix(m) => m.is_identity(),
            Transform::Compose(transforms) => transforms.iter().all(|t| t.is_identity()),
            _ => false,
        }
    }

    /// Check if this transform includes translation
    #[inline]
    pub fn has_translation(&self) -> bool {
        match self {
            Transform::Translate { .. } => true,
            Transform::RotateAround { .. } | Transform::ScaleAround { .. } => true,
            Transform::Compose(transforms) => transforms.iter().any(|t| t.has_translation()),
            Transform::Matrix(m) => m.m[12] != 0.0 || m.m[13] != 0.0,
            _ => false,
        }
    }

    /// Check if this transform includes rotation
    #[inline]
    pub fn has_rotation(&self) -> bool {
        match self {
            Transform::Rotate { .. } | Transform::RotateAround { .. } => true,
            Transform::Compose(transforms) => transforms.iter().any(|t| t.has_rotation()),
            _ => false,
        }
    }

    /// Check if this transform includes scaling
    #[inline]
    pub fn has_scale(&self) -> bool {
        match self {
            Transform::Scale { .. } | Transform::ScaleXY { .. } | Transform::ScaleAround { .. } => {
                true
            }
            Transform::Compose(transforms) => transforms.iter().any(|t| t.has_scale()),
            _ => false,
        }
    }

    /// Check if this transform includes skew
    #[inline]
    pub fn has_skew(&self) -> bool {
        match self {
            Transform::Skew { .. } => true,
            Transform::Compose(transforms) => transforms.iter().any(|t| t.has_skew()),
            _ => false,
        }
    }

    // ===== Inversion =====

    /// Compute the inverse transform (if possible)
    ///
    /// Returns None if the transform is not invertible (e.g., scale by 0).
    #[inline]
    pub fn inverse(&self) -> Option<Transform> {
        // For simple transforms, we can compute analytical inverses
        // For complex cases, fall back to matrix inversion
        match self {
            Transform::Identity => Some(Transform::Identity),

            Transform::Translate { x, y } => Some(Transform::Translate { x: -x, y: -y }),

            Transform::Rotate { angle } => Some(Transform::Rotate { angle: -angle }),

            Transform::Scale { factor } => {
                if factor.abs() < f32::EPSILON {
                    None
                } else {
                    Some(Transform::Scale {
                        factor: 1.0 / factor,
                    })
                }
            }

            Transform::ScaleXY { x, y } => {
                if x.abs() < f32::EPSILON || y.abs() < f32::EPSILON {
                    None
                } else {
                    Some(Transform::ScaleXY {
                        x: 1.0 / x,
                        y: 1.0 / y,
                    })
                }
            }

            Transform::Skew { x, y } => Some(Transform::Skew { x: -x, y: -y }),

            // For complex transforms, use matrix inversion
            _ => {
                let matrix: Matrix4 = self.clone().into();
                matrix.try_inverse().map(Transform::Matrix)
            }
        }
    }

    /// Decompose a 2D affine transform into translate, rotate, and scale components
    ///
    /// This is useful for applying transforms via painter APIs that only support
    /// primitive operations (translate, rotate, scale) rather than arbitrary matrices.
    ///
    /// # Returns
    ///
    /// A tuple of (translation, rotation_radians, scale_x, scale_y)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let transform = Transform::translate(50.0, 100.0)
    ///     .then(Transform::rotate(PI / 4.0))
    ///     .then(Transform::scale_xy(2.0, 3.0));
    ///
    /// let (tx, ty, rotation, sx, sy) = transform.decompose();
    /// // Apply via painter: translate(tx, ty) → rotate(rotation) → scale(sx, sy)
    /// ```
    #[inline]
    pub fn decompose(&self) -> (f32, f32, f32, f32, f32) {
        let matrix: Matrix4 = self.clone().into();

        // Extract translation from matrix (m[12], m[13] are 2D translation)
        let tx = matrix.m[12];
        let ty = matrix.m[13];

        // For 2D affine transforms, the matrix looks like:
        // [ m[0]  m[4]  0   m[12] ]   [ a  c  0  tx ]
        // [ m[1]  m[5]  0   m[13] ] = [ b  d  0  ty ]
        // [ m[2]  m[6]  1   m[14] ]   [ 0  0  1  0  ]
        // [ m[3]  m[7]  0   m[15] ]   [ 0  0  0  1  ]

        let a = matrix.m[0];
        let b = matrix.m[1];
        let c = matrix.m[4];
        let d = matrix.m[5];

        // Extract scale from column vectors
        let sx = (a * a + b * b).sqrt();
        let det = a * d - b * c;
        let sy = if sx > f32::EPSILON {
            det / sx
        } else {
            (c * c + d * d).sqrt()
        };

        // Extract rotation from normalized column vector
        let rotation = if sx > f32::EPSILON {
            b.atan2(a) // Rotation angle in radians
        } else {
            0.0
        };

        (tx, ty, rotation, sx, sy)
    }
}

// ===== Idiomatic Rust Trait Implementations =====

/// Convert Matrix4 to Transform (wraps in Matrix variant)
impl From<Matrix4> for Transform {
    fn from(matrix: Matrix4) -> Self {
        if matrix.is_identity() {
            Transform::Identity
        } else {
            Transform::Matrix(matrix)
        }
    }
}

/// Convert Offset to Transform (creates Translation)
impl From<Offset<Pixels>> for Transform {
    fn from(offset: Offset<Pixels>) -> Self {
        Transform::translate(offset.dx.0, offset.dy.0)
    }
}

/// Convert Transform to Matrix4 (the main conversion)
///
/// This is the idiomatic Rust way to convert Transform to Matrix4:
///
/// ```rust,ignore
/// let transform = Transform::rotate(PI / 4.0);
/// let matrix: Matrix4 = transform.into();
/// ```
impl From<Transform> for Matrix4 {
    fn from(transform: Transform) -> Self {
        transform.to_matrix_internal()
    }
}

/// Convert &Transform to Matrix4 (reference conversion)
///
/// Allows converting from a reference without moving:
///
/// ```rust,ignore
/// let transform = Transform::rotate(PI / 4.0);
/// let matrix: Matrix4 = (&transform).into();
/// // transform is still usable here
/// ```
impl From<&Transform> for Matrix4 {
    fn from(transform: &Transform) -> Self {
        transform.to_matrix_internal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::units::px;

    #[test]
    fn test_identity() {
        let transform = Transform::identity();
        assert!(transform.is_identity());
        assert_eq!(transform.to_matrix(), Matrix4::identity());
    }

    #[test]
    fn test_translate() {
        let transform = Transform::translate(10.0, 20.0);
        let matrix = transform.to_matrix();
        assert_eq!(matrix.m[12], 10.0);
        assert_eq!(matrix.m[13], 20.0);
    }

    #[test]
    fn test_rotate() {
        let transform = Transform::rotate(PI / 2.0);
        let matrix = transform.to_matrix();

        // At 90 degrees, cos(90°) ≈ 0, sin(90°) ≈ 1
        assert!((matrix.m[0] - 0.0).abs() < 0.001); // cos(90°)
        assert!((matrix.m[1] - 1.0).abs() < 0.001); // sin(90°)
    }

    #[test]
    fn test_scale_uniform() {
        let transform = Transform::scale(2.0);
        let matrix = transform.to_matrix();
        assert_eq!(matrix.m[0], 2.0);
        assert_eq!(matrix.m[5], 2.0);
    }

    #[test]
    fn test_scale_xy() {
        let transform = Transform::scale_xy(2.0, 3.0);
        let matrix = transform.to_matrix();
        assert_eq!(matrix.m[0], 2.0);
        assert_eq!(matrix.m[5], 3.0);
    }

    #[test]
    fn test_skew() {
        let transform = Transform::skew(0.2, 0.3);
        let matrix = transform.to_matrix();

        // Check skew matrix structure
        assert_eq!(matrix.m[0], 1.0); // No scaling on X
        assert_eq!(matrix.m[5], 1.0); // No scaling on Y
        assert!((matrix.m[1] - 0.2f32.tan()).abs() < 0.001); // tan(x) in m[0][1]
        assert!((matrix.m[4] - 0.3f32.tan()).abs() < 0.001); // tan(y) in m[1][0]
    }

    #[test]
    fn test_compose() {
        let transform = Transform::translate(10.0, 20.0)
            .then(Transform::rotate(PI / 4.0))
            .then(Transform::scale(2.0));

        assert!(transform.has_translation());
        assert!(transform.has_rotation());
        assert!(transform.has_scale());
    }

    #[test]
    fn test_compose_flattening() {
        let t1 = Transform::translate(10.0, 10.0).then(Transform::rotate(PI / 4.0));
        let t2 = Transform::scale(2.0).then(Transform::skew(0.1, 0.0));
        let composed = t1.then(t2);

        if let Transform::Compose(transforms) = composed {
            assert_eq!(transforms.len(), 4); // Flattened
        } else {
            panic!("Expected Compose variant");
        }
    }

    #[test]
    fn test_identity_optimization() {
        let transform = Transform::identity().then(Transform::translate(10.0, 20.0));

        // Should optimize to just Translate
        assert!(matches!(transform, Transform::Translate { .. }));
    }

    #[test]
    fn test_inverse_translate() {
        let transform = Transform::translate(10.0, 20.0);
        let inverse = transform.inverse().unwrap();

        if let Transform::Translate { x, y } = inverse {
            assert_eq!(x, -10.0);
            assert_eq!(y, -20.0);
        } else {
            panic!("Expected Translate inverse");
        }
    }

    #[test]
    fn test_inverse_rotate() {
        let transform = Transform::rotate(PI / 4.0);
        let inverse = transform.inverse().unwrap();

        if let Transform::Rotate { angle } = inverse {
            assert!((angle + PI / 4.0).abs() < 0.001);
        } else {
            panic!("Expected Rotate inverse");
        }
    }

    #[test]
    fn test_inverse_scale() {
        let transform = Transform::scale(2.0);
        let inverse = transform.inverse().unwrap();

        if let Transform::Scale { factor } = inverse {
            assert!((factor - 0.5).abs() < 0.001);
        } else {
            panic!("Expected Scale inverse");
        }
    }

    #[test]
    fn test_rotate_around() {
        let transform = Transform::rotate_around(PI / 2.0, 50.0, 50.0);
        let matrix = transform.to_matrix();

        // Should rotate around (50, 50) instead of origin
        // Verify that (50, 50) stays at (50, 50) after transform
        let x = 50.0;
        let y = 50.0;
        let transformed_x = matrix.m[0] * x + matrix.m[4] * y + matrix.m[12];
        let transformed_y = matrix.m[1] * x + matrix.m[5] * y + matrix.m[13];

        assert!((transformed_x - 50.0).abs() < 0.001);
        assert!((transformed_y - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_from_offset() {
        let offset = Offset::new(px(10.0), px(20.0));
        let transform = Transform::from(offset);

        if let Transform::Translate { x, y } = transform {
            assert_eq!(x, 10.0);
            assert_eq!(y, 20.0);
        } else {
            panic!("Expected Translate from Offset");
        }
    }

    #[test]
    fn test_query_methods() {
        let transform = Transform::translate(10.0, 20.0)
            .then(Transform::rotate(PI / 4.0))
            .then(Transform::scale(2.0))
            .then(Transform::skew(0.1, 0.0));

        assert!(transform.has_translation());
        assert!(transform.has_rotation());
        assert!(transform.has_scale());
        assert!(transform.has_skew());
        assert!(!transform.is_identity());
    }
}
