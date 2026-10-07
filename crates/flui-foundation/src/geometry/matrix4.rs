//! 4x4 transformation matrix for 2D and 3D transformations.
//!
//! Matrix4 represents a 4x4 matrix stored in column-major order (like
//! OpenGL/egui). Used for affine transformations: translation, rotation,
//! scaling, skewing, perspective.
//!
//! # Design Philosophy
//!
//! This implementation prioritizes:
//! - **Memory Safety**: No unsafe code, bounds-checked access
//! - **Type Safety**: Strong typing with `#[must_use]` annotations
//! - **Zero Allocations**: All operations use stack-allocated arrays
//! - **Idiomatic Rust**: Implements standard traits (`From`, `Into`, `Index`,
//!   etc.)
//! - **Performance**: Inline functions, const methods, zero-copy conversions
//! - **Mathematical Correctness**: Extensively tested with edge cases
//!
//! # Examples
//!
//! ## Basic Transformations
//!
//! ```
//! use flui_foundation::geometry::Matrix4;
//!
//! // Identity matrix (const-evaluable)
//! const IDENTITY: Matrix4 = Matrix4::identity();
//!
//! // Translation
//! let translate = Matrix4::translation(10.0, 20.0, 0.0);
//!
//! // Scaling
//! let scale = Matrix4::scaling(2.0, 2.0, 1.0);
//!
//! // Rotation (around Z axis for 2D)
//! let rotate = Matrix4::rotation_z(std::f64::consts::PI / 4.0); // 45 degrees
//!
//! // Combine transformations (right-to-left application)
//! let combined = translate * rotate * scale;
//!
//! // Transform a point
//! let (x, y) = combined.transform_point(1.0, 0.0);
//! ```
//!
//! ## Advanced Operations
//!
//! ```
//! use flui_foundation::geometry::Matrix4;
//!
//! let m = Matrix4::rotation_z(0.5);
//!
//! // Matrix inverse
//! if let Some(inv) = m.try_inverse() {
//!     let product = m * inv;
//!     assert!(product.is_identity());
//! }
//!
//! // Transpose (for rotation matrices: transpose = inverse)
//! let transposed = m.transpose();
//!
//! // Determinant
//! let det = m.determinant();
//! ```
//!
//! ## Type-Safe Access
//!
//! ```
//! use flui_foundation::geometry::Matrix4;
//!
//! let mut m = Matrix4::identity();
//!
//! // Index access (linear, column-major)
//! assert_eq!(m[0], 1.0);
//!
//! // Row/column access
//! let value = m.get(0, 0);
//! *m.get_mut(0, 3) = 10.0; // Set translation
//!
//! // Zero-copy conversions
//! let array: [f64; 16] = m.into();
//! let m2 = Matrix4::from(array);
//! ```
//!
//! ## Approximate Equality
//!
//! ```
//! use flui_foundation::geometry::Matrix4;
//!
//! let m1 = Matrix4::translation(1.0, 2.0, 0.0);
//! let m2 = Matrix4::translation(1.0000001, 2.0, 0.0);
//!
//! // Exact equality (bitwise)
//! assert_ne!(m1, m2);
//!
//! // Approximate equality (with epsilon)
//! assert!(m1.approx_eq(&m2));
//! assert!(m1.approx_eq_eps(&m2, 0.001));
//! ```

use std::{
    fmt,
    ops::{Index, IndexMut, Mul, MulAssign},
};

use glam::DMat4;

use crate::geometry::Rect;

/// A 4x4 transformation matrix stored in column-major order.
///
/// Used for affine transformations including translation, rotation, scaling,
/// and skewing. The matrix is stored in column-major order to match OpenGL and
/// egui conventions.
///
/// # Memory Layout
///
/// The 16 floats are stored as: `[m0, m1, m2, m3, m4, ..., m15]` representing:
/// ```text
/// | m0  m4  m8  m12 |
/// | m1  m5  m9  m13 |
/// | m2  m6  m10 m14 |
/// | m3  m7  m11 m15 |
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Matrix4 {
    /// Matrix elements in column-major order (16 floats)
    pub m: [f64; 16],
}

impl Matrix4 {
    /// Borrows the column-major storage as a `glam::DMat4` for delegated math.
    ///
    /// Both types are column-major, so this is a direct reinterpret of the 16
    /// floats (Option D backend).
    #[inline]
    #[must_use]
    fn to_glam(self) -> DMat4 {
        DMat4::from_cols_array(&self.m)
    }

    /// Wraps a `glam::DMat4` result back into the column-major storage.
    #[inline]
    #[must_use]
    fn from_glam(m: DMat4) -> Self {
        Self {
            m: m.to_cols_array(),
        }
    }

    /// The column-major elements narrowed to `f32`, for upload to the GPU.
    ///
    /// The framework keeps transforms in `f64`; this is the one narrowing point for a
    /// matrix on its way to a shader (ADR-0098 §2).
    #[inline]
    #[must_use]
    pub fn to_cols_array_f32(&self) -> [f32; 16] {
        self.m.map(|v| v as f32)
    }

    /// Interpolates toward `other` by decomposition, the way CSS Transforms 2 interpolates
    /// 3D matrices ("Decomposing a 3D matrix").
    ///
    /// Each matrix is split into perspective, translation, scale, skew and a rotation
    /// quaternion. The parts interpolate linearly, except the rotation, which is slerped
    /// along the shorter arc; the result is recomposed. An element-wise lerp would instead
    /// shear a rotation (a 90° turn passes through a degenerate matrix half way).
    ///
    /// - `t == 0.0` returns `self` and `t == 1.0` returns `other`, bit for bit; other values,
    ///   including those outside `[0, 1]`, extrapolate.
    /// - An endpoint whose linear part collapses an axis (a zero scale) has no orientation
    ///   of its own and takes the other endpoint's rotation and skew, so a scale-in from
    ///   zero grows in place instead of spinning. Two collapsed endpoints interpolate
    ///   without rotation or skew.
    /// - A matrix that cannot be decomposed (`m33 == 0`, an `m33` so small that normalising by it overflows, or a perspective row over a
    ///   singular linear part) switches discretely: `self` for `t < 0.5`, `other` from
    ///   `t >= 0.5`.
    /// - Finite endpoints give a finite result for any finite `t` short of overflowing the
    ///   extrapolated parts. A non-finite element in either endpoint, or a NaN `t`, falls
    ///   back to element-wise interpolation, so the non-finite value carries through.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Matrix4;
    ///
    /// let mid = Matrix4::scaling(0.0, 0.0, 1.0).lerp(Matrix4::IDENTITY, 0.5);
    /// assert!(mid.approx_eq(&Matrix4::scaling(0.5, 0.5, 1.0)));
    /// ```
    #[must_use]
    pub fn lerp(self, other: Self, t: f64) -> Self {
        use super::matrix4_decompose::{Decomposed, decompose};

        if t == 0.0 {
            return self;
        }
        if t == 1.0 {
            return other;
        }
        if !t.is_nan() && self.m == other.m {
            // Identical endpoints are a constant; a collapsed rotated matrix would
            // otherwise lose the orientation its decomposition cannot recover.
            return other;
        }
        let finite = |m: &Self| m.m.iter().all(|v| v.is_finite());
        if t.is_nan() || !finite(&self) || !finite(&other) {
            return Self {
                m: std::array::from_fn(|i| self.m[i] + (other.m[i] - self.m[i]) * t),
            };
        }
        let (from, to) = match (decompose(&self), decompose(&other)) {
            (Decomposed::Singular, _) | (_, Decomposed::Singular) => {
                return if t < 0.5 { self } else { other };
            }
            (Decomposed::Full(from), Decomposed::Full(to))
            | (Decomposed::Collapsed(from), Decomposed::Collapsed(to)) => (from, to),
            (Decomposed::Full(from), Decomposed::Collapsed(to)) => {
                (from, to.with_orientation_of(&from))
            }
            (Decomposed::Collapsed(from), Decomposed::Full(to)) => {
                (from.with_orientation_of(&to), to)
            }
        };
        from.interpolate(&to, t).recompose()
    }

    /// Identity matrix constant (no transformation).
    ///
    /// This is a compile-time constant that can be used anywhere a `Matrix4` is
    /// needed.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_foundation::geometry::Matrix4;
    ///
    /// let transform = Matrix4::IDENTITY;
    /// assert!(transform.is_identity());
    /// ```
    pub const IDENTITY: Self = Self {
        m: [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    };

    /// Zero matrix constant (all elements are zero).
    ///
    /// # Example
    ///
    /// ```
    /// use flui_foundation::geometry::Matrix4;
    ///
    /// let zero = Matrix4::ZERO;
    /// assert_eq!(zero.determinant(), 0.0);
    /// ```
    pub const ZERO: Self = Self { m: [0.0; 16] };
}

impl Matrix4 {
    /// Creates a new matrix from 16 elements in column-major order.
    ///
    /// Parameters are named as `mRC` where R is row and C is column
    /// (0-indexed).
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        m00: f64,
        m01: f64,
        m02: f64,
        m03: f64,
        m10: f64,
        m11: f64,
        m12: f64,
        m13: f64,
        m20: f64,
        m21: f64,
        m22: f64,
        m23: f64,
        m30: f64,
        m31: f64,
        m32: f64,
        m33: f64,
    ) -> Self {
        Self {
            m: [
                m00, m01, m02, m03, m10, m11, m12, m13, m20, m21, m22, m23, m30, m31, m32, m33,
            ],
        }
    }

    /// Creates an identity matrix (no transformation).
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            m: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    /// Creates a translation matrix.
    ///
    /// For 2D transformations, use `z = 0.0`.
    #[inline]
    pub fn translation(x: f64, y: f64, z: f64) -> Self {
        Self::new(
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0,
        )
    }

    /// Creates a uniform or non-uniform scaling matrix.
    ///
    /// For 2D transformations, use `z = 1.0`.
    #[inline]
    pub fn scaling(x: f64, y: f64, z: f64) -> Self {
        Self::new(
            x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, z, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a rotation matrix around the Z axis (for 2D rotations).
    ///
    /// Angle is in radians. Positive values rotate counter-clockwise.
    #[inline]
    pub fn rotation_z(angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(
            cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a rotation matrix around the X axis.
    ///
    /// Angle is in radians. Positive values rotate counter-clockwise when
    /// looking down the axis.
    #[inline]
    pub fn rotation_x(angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(
            1.0, 0.0, 0.0, 0.0, 0.0, cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a rotation matrix around the Y axis.
    ///
    /// Angle is in radians. Positive values rotate counter-clockwise when
    /// looking down the axis.
    #[inline]
    pub fn rotation_y(angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(
            cos, 0.0, -sin, 0.0, 0.0, 1.0, 0.0, 0.0, sin, 0.0, cos, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a 2D skew (shear) matrix.
    ///
    /// - `skew_x`: Skew angle along the X axis (in radians)
    /// - `skew_y`: Skew angle along the Y axis (in radians)
    #[inline]
    pub fn skew_2d(skew_x: f64, skew_y: f64) -> Self {
        let tan_x = skew_x.tan();
        let tan_y = skew_y.tan();

        Self::new(
            1.0, tan_y, 0.0, 0.0, tan_x, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Alias for `skew_2d`.
    #[inline]
    pub fn skew(skew_x: f64, skew_y: f64) -> Self {
        Self::skew_2d(skew_x, skew_y)
    }

    /// Returns whether this is an identity matrix, using a default epsilon of
    /// `1e-5`.
    #[inline]
    pub fn is_identity(&self) -> bool {
        self.is_identity_with_epsilon(1e-5)
    }

    /// Returns whether this finite matrix is identity within `epsilon`.
    ///
    /// The tolerance must be finite and nonnegative; otherwise returns false.
    pub fn is_identity_with_epsilon(&self, epsilon: f64) -> bool {
        self.approx_eq_eps(&Self::IDENTITY, epsilon)
    }

    /// This matrix as a pure 2D translation `(dx, dy)`, or `None` if it is
    /// anything else.
    ///
    /// Deliberately **exact** rather than epsilon-tolerant, unlike [`is_translation_only`](Self::is_translation_only):
    /// the caller's next move is to drop the matrix entirely and shift the
    /// child by `(dx, dy)` instead, so a matrix that is merely *near* a
    /// translation must take the general path. Accepting it would silently
    /// discard the residual scale or skew.
    ///
    /// A translation along z also returns `None` — the result is a 2D offset,
    /// and there is no honest way to express a z component in one.
    #[must_use]
    pub fn as_translation(&self) -> Option<(f64, f64)> {
        // Column-major: columns 0..2 must be the identity basis, the
        // perspective row zero, and the translation column's z entry zero.
        let is_identity_basis = self.m[0] == 1.0
            && self.m[1] == 0.0
            && self.m[2] == 0.0
            && self.m[3] == 0.0
            && self.m[4] == 0.0
            && self.m[5] == 1.0
            && self.m[6] == 0.0
            && self.m[7] == 0.0
            && self.m[8] == 0.0
            && self.m[9] == 0.0
            && self.m[10] == 1.0
            && self.m[11] == 0.0
            && self.m[14] == 0.0
            && self.m[15] == 1.0;

        is_identity_basis.then(|| (self.m[12], self.m[13]))
    }

    /// Returns whether this matrix represents only a translation, using
    /// `f64::EPSILON` as the comparison tolerance.
    #[inline]
    pub fn is_translation_only(&self) -> bool {
        self.is_translation_only_with_epsilon(f64::EPSILON)
    }

    /// Returns whether this matrix represents only a translation with custom
    /// epsilon.
    ///
    /// Checks that the 3x3 upper-left submatrix is identity (within epsilon)
    /// and the perspective row is [0, 0, 0, 1].
    pub fn is_translation_only_with_epsilon(&self, epsilon: f64) -> bool {
        // Column-major layout:
        // m[0..3]   = column 0 (should be [1, 0, 0, 0])
        // m[4..7]   = column 1 (should be [0, 1, 0, 0])
        // m[8..11]  = column 2 (should be [0, 0, 1, 0])
        // m[12..15] = column 3 (translation: [tx, ty, tz, 1])

        // Check diagonal elements (should be 1.0)
        (self.m[0] - 1.0).abs() < epsilon
            && (self.m[5] - 1.0).abs() < epsilon
            && (self.m[10] - 1.0).abs() < epsilon
            && (self.m[15] - 1.0).abs() < epsilon
            // Check off-diagonal elements in upper-left 3x3 (should be 0.0)
            && self.m[1].abs() < epsilon
            && self.m[2].abs() < epsilon
            && self.m[4].abs() < epsilon
            && self.m[6].abs() < epsilon
            && self.m[8].abs() < epsilon
            && self.m[9].abs() < epsilon
            // Check perspective row elements (should be 0.0)
            && self.m[3].abs() < epsilon
            && self.m[7].abs() < epsilon
            && self.m[11].abs() < epsilon
    }

    /// Extracts the translation component (x, y, z) from the matrix.
    #[inline]
    pub fn translation_component(&self) -> (f64, f64, f64) {
        (self.m[12], self.m[13], self.m[14])
    }

    /// Sets the translation component without affecting other transformations.
    #[inline]
    pub fn set_translation(&mut self, x: f64, y: f64, z: f64) {
        self.m[12] = x;
        self.m[13] = y;
        self.m[14] = z;
    }

    /// Applies a translation to this matrix (modifies in place).
    #[inline]
    pub fn translate(&mut self, x: f64, y: f64, z: f64) {
        *self = Matrix4::translation(x, y, z) * *self;
    }

    /// Applies a scaling to this matrix (modifies in place).
    #[inline]
    pub fn scale(&mut self, x: f64, y: f64, z: f64) {
        *self = Matrix4::scaling(x, y, z) * *self;
    }

    /// Applies a Z-axis rotation to this matrix (modifies in place).
    #[inline]
    pub fn rotate_z(&mut self, angle: f64) {
        *self = Matrix4::rotation_z(angle) * *self;
    }

    /// Transforms a 2D point (x, y) by this matrix.
    ///
    /// Uses homogeneous coordinates: (x, y, 0, 1) → (x', y', z', w')
    /// Returns (x'/w', y'/w').
    pub fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        let x_out = self.m[0] * x + self.m[4] * y + self.m[12];
        let y_out = self.m[1] * x + self.m[5] * y + self.m[13];
        let w_out = self.m[3] * x + self.m[7] * y + self.m[15];

        if w_out.abs() > f64::EPSILON {
            ((x_out / w_out), (y_out / w_out))
        } else {
            (x_out, y_out)
        }
    }

    /// Unprojects a screen point onto the local `z = 0` plane.
    ///
    /// **The receiver is the inverse global-to-local matrix**, usually obtained
    /// from the forward paint transform with [`Self::try_inverse`]. A projected
    /// screen point does not specify its depth: this method intersects its ray
    /// with the local plane instead of assuming that screen depth is zero.
    ///
    /// Returns `None` for a non-finite matrix or point, a parallel ray, an
    /// intersection at infinity, a point behind the camera (forward homogeneous
    /// `w <= 0`), or arithmetic that cannot publish finite local coordinates.
    /// Homogeneous cancellation within floating-point precision is refused.
    /// [`Self::transform_point`] retains its separate forward projection semantics.
    #[must_use]
    pub fn unproject_to_plane(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        if !x.is_finite() || !y.is_finite() || self.m.iter().any(|value| !value.is_finite()) {
            return None;
        }
        let matrix_scale = self.m.iter().fold(0.0_f64, |scale, value| scale.max(value.abs()));
        if matrix_scale == 0.0 {
            return None;
        }
        // Both normalizations are positive, so they preserve visibility and the
        // final projective quotient while keeping every product bounded.
        let m = self.m.map(|value| value / matrix_scale);
        let point_scale = x.abs().max(y.abs()).max(1.0);
        let sx = x / point_scale;
        let sy = y / point_scale;
        let sw = 1.0 / point_scale;
        let ray = [
            m[0] * sx + m[4] * sy + m[12] * sw,
            m[1] * sx + m[5] * sy + m[13] * sw,
            m[2] * sx + m[6] * sy + m[14] * sw,
            m[3] * sx + m[7] * sy + m[15] * sw,
        ];
        // Changing screen depth moves along inverse column 2. Eliminate
        // screen depth in homogeneous coordinates before dividing by w.
        let depth = m[10];
        if depth == 0.0 {
            return None;
        }
        let weight_at_origin = ray[3] * depth;
        let weight_along_ray = m[11] * ray[2];
        let weight = weight_at_origin - weight_along_ray;
        let ray_weight_bound = (m[3] * sx).abs() + (m[7] * sy).abs() + (m[15] * sw).abs();
        let ray_depth_bound = (m[2] * sx).abs() + (m[6] * sy).abs() + (m[14] * sw).abs();
        let uncertainty = f64::EPSILON
            * (ray_weight_bound * depth.abs() + m[11].abs() * ray_depth_bound);
        if weight.abs() <= uncertainty || weight.is_sign_positive() != depth.is_sign_positive() {
            return None;
        }
        let local_x = (ray[0] * depth - m[8] * ray[2]) / weight;
        let local_y = (ray[1] * depth - m[9] * ray[2]) / weight;
        (local_x.is_finite() && local_y.is_finite()).then_some((local_x, local_y))
    }

    /// Transforms a rectangle by this matrix, returning the bounding box of the
    /// result.
    ///
    /// Transforms all four corners and computes the axis-aligned bounding box.
    #[must_use]
    pub fn transform_rect(&self, rect: &Rect<f64>) -> Rect<f64> {
        // Transform all four corners
        let (x0, y0) = self.transform_point(rect.min.x, rect.min.y); // Top-left
        let (x1, y1) = self.transform_point(rect.max.x, rect.min.y); // Top-right
        let (x2, y2) = self.transform_point(rect.min.x, rect.max.y); // Bottom-left
        let (x3, y3) = self.transform_point(rect.max.x, rect.max.y); // Bottom-right

        // Find min/max of all transformed corners
        let min_x = x0.min(x1).min(x2).min(x3);
        let min_y = y0.min(y1).min(y2).min(y3);
        let max_x = x0.max(x1).max(x2).max(x3);
        let max_y = y0.max(y1).max(y2).max(y3);

        Rect::from_ltrb(min_x, min_y, max_x, max_y)
    }

    /// Returns the matrix as a column-major array (zero-copy).
    #[must_use]
    pub const fn to_col_major_array(&self) -> [f64; 16] {
        self.m
    }

    /// Returns the transpose of this matrix.
    ///
    /// For rotation matrices, the transpose is equal to the inverse.
    #[must_use]
    pub fn transpose(&self) -> Self {
        let m = &self.m;
        Self::new(
            m[0], m[4], m[8], m[12], m[1], m[5], m[9], m[13], m[2], m[6], m[10], m[14], m[3], m[7],
            m[11], m[15],
        )
    }

    /// Transposes this matrix in place (zero-allocation).
    ///
    /// This method swaps elements in place without creating a temporary matrix.
    pub fn transpose_in_place(&mut self) {
        // Swap off-diagonal elements (column-major indexing)
        for row in 0..4 {
            for col in (row + 1)..4 {
                let idx1 = col * 4 + row;
                let idx2 = row * 4 + col;
                self.m.swap(idx1, idx2);
            }
        }
    }

    /// Converts the matrix to a 2D array in row-major order.
    #[must_use]
    pub fn to_row_major_2d(&self) -> [[f64; 4]; 4] {
        [
            [self.m[0], self.m[4], self.m[8], self.m[12]],
            [self.m[1], self.m[5], self.m[9], self.m[13]],
            [self.m[2], self.m[6], self.m[10], self.m[14]],
            [self.m[3], self.m[7], self.m[11], self.m[15]],
        ]
    }

    /// Converts the matrix to a 2D array in column-major order.
    #[must_use]
    pub fn to_col_major_2d(&self) -> [[f64; 4]; 4] {
        [
            [self.m[0], self.m[1], self.m[2], self.m[3]],
            [self.m[4], self.m[5], self.m[6], self.m[7]],
            [self.m[8], self.m[9], self.m[10], self.m[11]],
            [self.m[12], self.m[13], self.m[14], self.m[15]],
        ]
    }

    /// Gets the matrix element at the specified row and column.
    ///
    /// # Panics
    /// Panics if row or column is >= 4.
    #[must_use]
    pub fn get(&self, row: usize, col: usize) -> f64 {
        assert!(row < 4 && col < 4, "Matrix index out of bounds");
        self.m[col * 4 + row]
    }

    /// Gets a mutable reference to the matrix element at the specified row and
    /// column.
    ///
    /// # Panics
    /// Panics if row or column is >= 4.
    #[inline]
    pub fn get_mut(&mut self, row: usize, col: usize) -> &mut f64 {
        assert!(row < 4 && col < 4, "Matrix index out of bounds");
        &mut self.m[col * 4 + row]
    }

    /// Returns whether this matrix has an admitted finite computed inverse.
    ///
    /// This computes the same inverse as [`try_inverse`](Self::try_inverse).
    /// Call that method directly when the inverse is also needed.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Matrix4;
    ///
    /// assert!(Matrix4::identity().is_invertible());
    /// assert!(Matrix4::translation(3.0, -1.0, 0.0).is_invertible());
    /// assert!(Matrix4::scaling(1e-9, 1e-9, 1.0).is_invertible());
    /// assert!(!Matrix4::scaling(0.0, 1.0, 1.0).is_invertible());
    /// ```
    #[must_use]
    pub fn is_invertible(&self) -> bool {
        self.try_inverse().is_some()
    }

    /// Attempts to compute a finite inverse using glam.
    ///
    /// Returns `None` for non-finite input, a zero or non-finite computed
    /// determinant, or non-finite computed inverse entries. There is no
    /// absolute determinant tolerance. Floating-point intermediate range
    /// limits can still refuse a mathematically invertible matrix.
    pub fn try_inverse(&self) -> Option<Self> {
        let matrix = self.to_glam();
        if !matrix.is_finite() {
            return None;
        }
        let determinant = matrix.determinant();
        if determinant == 0.0 || !determinant.is_finite() {
            return None;
        }
        let inverse = matrix.try_inverse()?;
        if inverse.is_finite() {
            Some(Self::from_glam(inverse))
        } else {
            None
        }
    }

    /// Inverts this matrix in place.
    ///
    /// Returns `true` if a finite computed inverse is admitted. On failure,
    /// returns `false` and leaves the matrix unchanged.
    pub fn invert(&mut self) -> bool {
        if let Some(inv) = self.try_inverse() {
            *self = inv;
            true
        } else {
            false
        }
    }

    /// Returns the determinant of this matrix.
    pub fn determinant(&self) -> f64 {
        self.to_glam().determinant()
    }
}

impl Default for Matrix4 {
    fn default() -> Self {
        Self::identity()
    }
}

/// Exact equality comparison (bitwise).
///
/// For floating-point tolerance comparison, use `approx_eq` or `approx_eq_eps`.
impl PartialEq for Matrix4 {
    fn eq(&self, other: &Self) -> bool {
        self.m == other.m
    }
}

impl Eq for Matrix4 {}

impl Matrix4 {
    /// Checks approximate equality with a custom epsilon.
    ///
    /// Returns true if all finite elements differ by at most `epsilon`.
    /// The tolerance must be finite and nonnegative; otherwise returns false.
    #[must_use]
    pub fn approx_eq_eps(&self, other: &Self, epsilon: f64) -> bool {
        epsilon.is_finite()
            && epsilon >= 0.0
            && self
                .m
                .iter()
                .zip(other.m.iter())
                .all(|(&a, &b)| a.is_finite() && b.is_finite() && (a - b).abs() <= epsilon)
    }

    /// Checks approximate equality with default epsilon (1e-5).
    #[must_use]
    pub fn approx_eq(&self, other: &Self) -> bool {
        self.approx_eq_eps(other, 1e-5)
    }
}

/// Matrix multiplication: `C = A * B`.
///
/// Matrices are applied right-to-left: `A * B` transforms first by `B`, then by
/// `A`. Delegates to `glam::DMat4`'s SIMD-accelerated column-major product
/// (Option D — replaces the hand-rolled scalar/SSE/NEON paths).
impl Mul for Matrix4 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        Self::from_glam(self.to_glam() * rhs.to_glam())
    }
}

impl MulAssign for Matrix4 {
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

/// Access matrix elements by linear index (0..16) in column-major order.
///
/// # Example
/// ```
/// use flui_foundation::geometry::Matrix4;
/// let m = Matrix4::identity();
/// assert_eq!(m[0], 1.0); // m00
/// assert_eq!(m[5], 1.0); // m11
/// ```
impl Index<usize> for Matrix4 {
    type Output = f64;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.m[index]
    }
}

/// Mutably access matrix elements by linear index (0..16) in column-major
/// order.
impl IndexMut<usize> for Matrix4 {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.m[index]
    }
}

/// Construct from column-major array.
impl From<[f64; 16]> for Matrix4 {
    #[inline]
    fn from(m: [f64; 16]) -> Self {
        Self { m }
    }
}

/// Convert to column-major array (zero-copy).
impl From<Matrix4> for [f64; 16] {
    #[inline]
    fn from(matrix: Matrix4) -> Self {
        matrix.m
    }
}

/// Construct from column-major 2D array.
impl From<[[f64; 4]; 4]> for Matrix4 {
    fn from(arr: [[f64; 4]; 4]) -> Self {
        Self {
            m: [
                arr[0][0], arr[0][1], arr[0][2], arr[0][3], arr[1][0], arr[1][1], arr[1][2],
                arr[1][3], arr[2][0], arr[2][1], arr[2][2], arr[2][3], arr[3][0], arr[3][1],
                arr[3][2], arr[3][3],
            ],
        }
    }
}

/// Convert to column-major 2D array.
impl From<Matrix4> for [[f64; 4]; 4] {
    #[inline]
    fn from(matrix: Matrix4) -> Self {
        matrix.to_col_major_2d()
    }
}

/// Borrow as slice for efficient read access.
impl AsRef<[f64; 16]> for Matrix4 {
    #[inline]
    fn as_ref(&self) -> &[f64; 16] {
        &self.m
    }
}

/// Mutably borrow as slice for efficient write access.
impl AsMut<[f64; 16]> for Matrix4 {
    #[inline]
    fn as_mut(&mut self) -> &mut [f64; 16] {
        &mut self.m
    }
}

impl fmt::Display for Matrix4 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Matrix4 [")?;
        for row in 0..4 {
            write!(f, "  [")?;
            for col in 0..4 {
                if col > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{:8.3}", self.m[col * 4 + row])?;
            }
            writeln!(f, " ]")?;
        }
        write!(f, "]")
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Matrix4 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.m.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Matrix4 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let m = <[f64; 16]>::deserialize(deserializer)?;
        Ok(Self { m })
    }
}

#[cfg(test)]
mod glam_backend_tests {
    use super::*;

    #[test]
    fn gpu_upload_narrows_the_f64_storage_to_f32_column_major() {
        // The framework keeps f64; the GPU takes f32, narrowed once at upload.
        assert_eq!(
            std::mem::size_of::<Matrix4>(),
            16 * std::mem::size_of::<f64>()
        );
        let m = Matrix4::translation(3.0, 4.0, 5.0);
        let cols = m.to_cols_array_f32();
        assert_eq!(&cols[12..15], &[3.0_f32, 4.0, 5.0]);
        assert_eq!(cols[0], 1.0_f32);
        assert_eq!(std::mem::size_of_val(&cols), 64);
    }
}
