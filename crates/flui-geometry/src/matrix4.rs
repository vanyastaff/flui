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
//! use flui_geometry::{Matrix4, px};
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
//! let rotate = Matrix4::rotation_z(std::f32::consts::PI / 4.0); // 45 degrees
//!
//! // Combine transformations (right-to-left application)
//! let combined = translate * rotate * scale;
//!
//! // Transform a point
//! let (x, y) = combined.transform_point(px(1.0), px(0.0));
//! ```
//!
//! ## Advanced Operations
//!
//! ```
//! use flui_geometry::Matrix4;
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
//! use flui_geometry::Matrix4;
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
//! let array: [f32; 16] = m.into();
//! let m2 = Matrix4::from(array);
//! ```
//!
//! ## Approximate Equality
//!
//! ```
//! use flui_geometry::Matrix4;
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

use glam::Mat4;

use super::Pixels;
use crate::Rect;

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
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Matrix4 {
    /// Matrix elements in column-major order (16 floats)
    pub m: [f32; 16],
}

impl Matrix4 {
    /// Borrows the column-major storage as a `glam::Mat4` for delegated math.
    ///
    /// Both types are column-major, so this is a direct reinterpret of the 16
    /// floats (Option D backend).
    #[inline]
    #[must_use]
    fn to_glam(self) -> Mat4 {
        Mat4::from_cols_array(&self.m)
    }

    /// Wraps a `glam::Mat4` result back into the column-major storage.
    #[inline]
    #[must_use]
    fn from_glam(m: Mat4) -> Self {
        Self {
            m: m.to_cols_array(),
        }
    }

    /// Interpolates toward `other` by decomposing both matrices into
    /// scale / rotation / translation, lerping the scale and translation and
    /// **slerping** the rotation, then recomposing.
    ///
    /// This is the correct way to interpolate an affine transform: a naive
    /// component-wise lerp of the 16 elements shears and distorts rotation
    /// (a 90° rotation lerped element-wise collapses through a degenerate
    /// matrix at `t = 0.5`). Matches Flutter's `Matrix4Tween`, which likewise
    /// decomposes. `t` is not clamped, so it extrapolates.
    ///
    /// Decomposition assumes an SRT-composable matrix (the common UI case:
    /// translate/rotate/scale); skew and perspective components are not
    /// preserved, exactly as in Flutter.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let (scale_a, rot_a, trans_a) = self.to_glam().to_scale_rotation_translation();
        let (scale_b, rot_b, trans_b) = other.to_glam().to_scale_rotation_translation();
        Self::from_glam(Mat4::from_scale_rotation_translation(
            scale_a.lerp(scale_b, t),
            rot_a.slerp(rot_b, t),
            trans_a.lerp(trans_b, t),
        ))
    }

    /// Identity matrix constant (no transformation).
    ///
    /// This is a compile-time constant that can be used anywhere a `Matrix4` is
    /// needed.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_geometry::Matrix4;
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
    /// use flui_geometry::Matrix4;
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        m00: f32,
        m01: f32,
        m02: f32,
        m03: f32,
        m10: f32,
        m11: f32,
        m12: f32,
        m13: f32,
        m20: f32,
        m21: f32,
        m22: f32,
        m23: f32,
        m30: f32,
        m31: f32,
        m32: f32,
        m33: f32,
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
    pub fn translation(x: f32, y: f32, z: f32) -> Self {
        Self::new(
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0,
        )
    }

    /// Creates a uniform or non-uniform scaling matrix.
    ///
    /// For 2D transformations, use `z = 1.0`.
    #[inline]
    pub fn scaling(x: f32, y: f32, z: f32) -> Self {
        Self::new(
            x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, z, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a rotation matrix around the Z axis (for 2D rotations).
    ///
    /// Angle is in radians. Positive values rotate counter-clockwise.
    #[inline]
    pub fn rotation_z(angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(
            cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a rotation matrix around the Z axis (type-safe version).
    #[inline]
    pub fn rotation_z_radians(angle: crate::Radians) -> Self {
        Self::rotation_z(angle.0)
    }

    /// Creates a rotation matrix around the X axis.
    ///
    /// Angle is in radians. Positive values rotate counter-clockwise when
    /// looking down the axis.
    #[inline]
    pub fn rotation_x(angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(
            1.0, 0.0, 0.0, 0.0, 0.0, cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a rotation matrix around the X axis (type-safe version).
    #[inline]
    pub fn rotation_x_radians(angle: crate::Radians) -> Self {
        Self::rotation_x(angle.0)
    }

    /// Creates a rotation matrix around the Y axis.
    ///
    /// Angle is in radians. Positive values rotate counter-clockwise when
    /// looking down the axis.
    #[inline]
    pub fn rotation_y(angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(
            cos, 0.0, -sin, 0.0, 0.0, 1.0, 0.0, 0.0, sin, 0.0, cos, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a rotation matrix around the Y axis (type-safe version).
    #[inline]
    pub fn rotation_y_radians(angle: crate::Radians) -> Self {
        Self::rotation_y(angle.0)
    }

    /// Creates a 2D skew (shear) matrix.
    ///
    /// - `skew_x`: Skew angle along the X axis (in radians)
    /// - `skew_y`: Skew angle along the Y axis (in radians)
    #[inline]
    pub fn skew_2d(skew_x: f32, skew_y: f32) -> Self {
        let tan_x = skew_x.tan();
        let tan_y = skew_y.tan();

        Self::new(
            1.0, tan_y, 0.0, 0.0, tan_x, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Alias for `skew_2d`.
    #[inline]
    pub fn skew(skew_x: f32, skew_y: f32) -> Self {
        Self::skew_2d(skew_x, skew_y)
    }

    /// Returns whether this is an identity matrix, using a default epsilon of
    /// `1e-5`.
    #[inline]
    pub fn is_identity(&self) -> bool {
        self.is_identity_with_epsilon(1e-5)
    }

    /// Returns whether this is an identity matrix with custom epsilon.
    pub fn is_identity_with_epsilon(&self, epsilon: f32) -> bool {
        for i in 0..16 {
            if (self.m[i] - Self::IDENTITY.m[i]).abs() > epsilon {
                return false;
            }
        }
        true
    }

    /// Returns whether this matrix represents only a translation, using
    /// `f32::EPSILON` as the comparison tolerance.
    #[inline]
    pub fn is_translation_only(&self) -> bool {
        self.is_translation_only_with_epsilon(f32::EPSILON)
    }

    /// Returns whether this matrix represents only a translation with custom
    /// epsilon.
    ///
    /// Checks that the 3x3 upper-left submatrix is identity (within epsilon)
    /// and the perspective row is [0, 0, 0, 1].
    pub fn is_translation_only_with_epsilon(&self, epsilon: f32) -> bool {
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
    pub fn translation_component(&self) -> (f32, f32, f32) {
        (self.m[12], self.m[13], self.m[14])
    }

    /// Sets the translation component without affecting other transformations.
    #[inline]
    pub fn set_translation(&mut self, x: f32, y: f32, z: f32) {
        self.m[12] = x;
        self.m[13] = y;
        self.m[14] = z;
    }

    /// Applies a translation to this matrix (modifies in place).
    #[inline]
    pub fn translate(&mut self, x: f32, y: f32, z: f32) {
        *self = Matrix4::translation(x, y, z) * *self;
    }

    /// Applies a scaling to this matrix (modifies in place).
    #[inline]
    pub fn scale(&mut self, x: f32, y: f32, z: f32) {
        *self = Matrix4::scaling(x, y, z) * *self;
    }

    /// Applies a Z-axis rotation to this matrix (modifies in place).
    #[inline]
    pub fn rotate_z(&mut self, angle: f32) {
        *self = Matrix4::rotation_z(angle) * *self;
    }

    /// Applies a Z-axis rotation to this matrix (type-safe version, modifies in
    /// place).
    #[inline]
    pub fn rotate_z_radians(&mut self, angle: crate::Radians) {
        self.rotate_z(angle.0);
    }

    /// Transforms a 2D point (x, y) by this matrix.
    ///
    /// Uses homogeneous coordinates: (x, y, 0, 1) → (x', y', z', w')
    /// Returns (x'/w', y'/w').
    pub fn transform_point(&self, x: Pixels, y: Pixels) -> (Pixels, Pixels) {
        let x_out = self.m[0] * x.0 + self.m[4] * y.0 + self.m[12];
        let y_out = self.m[1] * x.0 + self.m[5] * y.0 + self.m[13];
        let w_out = self.m[3] * x.0 + self.m[7] * y.0 + self.m[15];

        if w_out.abs() > f32::EPSILON {
            (Pixels(x_out / w_out), Pixels(y_out / w_out))
        } else {
            (Pixels(x_out), Pixels(y_out))
        }
    }

    /// Transforms a rectangle by this matrix, returning the bounding box of the
    /// result.
    ///
    /// Transforms all four corners and computes the axis-aligned bounding box.
    #[must_use]
    pub fn transform_rect(&self, rect: &Rect<Pixels>) -> Rect<Pixels> {
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
    pub const fn to_col_major_array(&self) -> [f32; 16] {
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
    pub fn to_row_major_2d(&self) -> [[f32; 4]; 4] {
        [
            [self.m[0], self.m[4], self.m[8], self.m[12]],
            [self.m[1], self.m[5], self.m[9], self.m[13]],
            [self.m[2], self.m[6], self.m[10], self.m[14]],
            [self.m[3], self.m[7], self.m[11], self.m[15]],
        ]
    }

    /// Converts the matrix to a 2D array in column-major order.
    #[must_use]
    pub fn to_col_major_2d(&self) -> [[f32; 4]; 4] {
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
    pub fn get(&self, row: usize, col: usize) -> f32 {
        assert!(row < 4 && col < 4, "Matrix index out of bounds");
        self.m[col * 4 + row]
    }

    /// Gets a mutable reference to the matrix element at the specified row and
    /// column.
    ///
    /// # Panics
    /// Panics if row or column is >= 4.
    #[inline]
    pub fn get_mut(&mut self, row: usize, col: usize) -> &mut f32 {
        assert!(row < 4 && col < 4, "Matrix index out of bounds");
        &mut self.m[col * 4 + row]
    }

    /// Attempts to invert this matrix.
    ///
    /// Returns `None` if the matrix is singular (determinant is zero).
    /// Uses Gauss-Jordan elimination for general 4x4 matrices.
    ///
    /// For simple transformations (translation, rotation, uniform scaling),
    /// consider using specialized inverse methods if available.
    pub fn try_inverse(&self) -> Option<Self> {
        // Guard singularity explicitly: `glam::Mat4::inverse` returns a matrix
        // of NaNs/inf for a non-invertible matrix rather than signalling, so we
        // gate on the determinant to preserve the `Option` contract.
        let g = self.to_glam();
        if g.determinant().abs() < f32::EPSILON {
            None
        } else {
            Some(Self::from_glam(g.inverse()))
        }
    }

    /// Inverts this matrix in place.
    ///
    /// Returns `true` if successful, `false` if the matrix is singular.
    pub fn invert(&mut self) -> bool {
        if let Some(inv) = self.try_inverse() {
            *self = inv;
            true
        } else {
            false
        }
    }

    /// Returns the determinant of this matrix.
    pub fn determinant(&self) -> f32 {
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
    /// Returns true if all elements differ by at most `epsilon`.
    #[must_use]
    pub fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        for i in 0..16 {
            if (self.m[i] - other.m[i]).abs() > epsilon {
                return false;
            }
        }
        true
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
/// `A`. Delegates to `glam::Mat4`'s SIMD-accelerated column-major product
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
/// use flui_geometry::Matrix4;
/// let m = Matrix4::identity();
/// assert_eq!(m[0], 1.0); // m00
/// assert_eq!(m[5], 1.0); // m11
/// ```
impl Index<usize> for Matrix4 {
    type Output = f32;

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
impl From<[f32; 16]> for Matrix4 {
    #[inline]
    fn from(m: [f32; 16]) -> Self {
        Self { m }
    }
}

/// Convert to column-major array (zero-copy).
impl From<Matrix4> for [f32; 16] {
    #[inline]
    fn from(matrix: Matrix4) -> Self {
        matrix.m
    }
}

/// Construct from column-major 2D array.
impl From<[[f32; 4]; 4]> for Matrix4 {
    fn from(arr: [[f32; 4]; 4]) -> Self {
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
impl From<Matrix4> for [[f32; 4]; 4] {
    #[inline]
    fn from(matrix: Matrix4) -> Self {
        matrix.to_col_major_2d()
    }
}

/// Borrow as slice for efficient read access.
impl AsRef<[f32; 16]> for Matrix4 {
    #[inline]
    fn as_ref(&self) -> &[f32; 16] {
        &self.m
    }
}

/// Mutably borrow as slice for efficient write access.
impl AsMut<[f32; 16]> for Matrix4 {
    #[inline]
    fn as_mut(&mut self) -> &mut [f32; 16] {
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
        let m = <[f32; 16]>::deserialize(deserializer)?;
        Ok(Self { m })
    }
}

#[cfg(test)]
mod glam_backend_tests {
    use super::*;

    #[test]
    fn matrix4_is_pod_64_bytes() {
        // The column-major [f32; 16] storage is Pod, so the engine can upload a
        // Matrix4 to a wgpu buffer via `bytemuck::cast_slice` with no shim.
        assert_eq!(std::mem::size_of::<Matrix4>(), 64);
        assert_eq!(
            std::mem::size_of::<Matrix4>(),
            16 * std::mem::size_of::<f32>()
        );
        let m = Matrix4::translation(3.0, 4.0, 5.0);
        let bytes: &[u8] = bytemuck::bytes_of(&m);
        assert_eq!(bytes.len(), 64);
    }

    #[test]
    fn glam_round_trip_is_identity() {
        let m = Matrix4::rotation_z(0.7) * Matrix4::translation(10.0, -3.0, 0.0);
        let round = Matrix4::from_glam(m.to_glam());
        assert!(m.approx_eq(&round));
    }

    #[test]
    fn mul_matches_manual_column_major_product() {
        // Guards the glam-delegated `Mul` against the previous scalar product.
        let a = Matrix4::scaling(2.0, 3.0, 1.0);
        let b = Matrix4::translation(5.0, 7.0, 0.0);
        let c = a * b; // apply b first, then a
        let (x, y) = c.transform_point(Pixels(1.0), Pixels(1.0));
        // b: (1,1)->(6,8); a: scale -> (12, 24)
        assert!((x.0 - 12.0).abs() < 1e-5, "x={}", x.0);
        assert!((y.0 - 24.0).abs() < 1e-5, "y={}", y.0);
    }

    #[test]
    fn singular_matrix_has_no_inverse() {
        let singular = Matrix4::scaling(0.0, 1.0, 1.0);
        assert!(singular.try_inverse().is_none());
        assert!(Matrix4::identity().try_inverse().is_some());
    }
}
