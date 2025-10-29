//! 4x4 transformation matrix for 2D and 3D transformations.
//!
//! Matrix4 represents a 4x4 matrix stored in column-major order (like OpenGL/egui).
//! Used for affine transformations: translation, rotation, scaling, skewing, perspective.
//!
//! # Design Philosophy
//!
//! This implementation prioritizes:
//! - **Memory Safety**: No unsafe code, bounds-checked access
//! - **Type Safety**: Strong typing with `#[must_use]` annotations
//! - **Zero Allocations**: All operations use stack-allocated arrays
//! - **Idiomatic Rust**: Implements standard traits (`From`, `Into`, `Index`, etc.)
//! - **Performance**: Inline functions, const methods, zero-copy conversions
//! - **Mathematical Correctness**: Extensively tested with edge cases
//!
//! # Examples
//!
//! ## Basic Transformations
//!
//! ```
//! use flui_types::Matrix4;
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
//! let (x, y) = combined.transform_point(1.0, 0.0);
//! ```
//!
//! ## Advanced Operations
//!
//! ```
//! use flui_types::Matrix4;
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
//! use flui_types::Matrix4;
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
//! use flui_types::Matrix4;
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

use std::fmt;
use std::ops::{Index, IndexMut, Mul, MulAssign};

/// 4x4 transformation matrix stored in column-major order.
///
/// The matrix is stored as 16 f32 values in column-major order:
/// ```text
/// [ m[0]  m[4]  m[8]   m[12] ]   [ m00  m10  m20  m30 ]
/// [ m[1]  m[5]  m[9]   m[13] ] = [ m01  m11  m21  m31 ]
/// [ m[2]  m[6]  m[10]  m[14] ]   [ m02  m12  m22  m32 ]
/// [ m[3]  m[7]  m[11]  m[15] ]   [ m03  m13  m23  m33 ]
/// ```
///
/// For 2D transformations (in homogeneous coordinates):
/// ```text
/// [ sx   shy  0   tx ]
/// [ shx  sy   0   ty ]
/// [ 0    0    1   0  ]
/// [ 0    0    0   1  ]
/// ```
/// where sx/sy = scale, shx/shy = shear, tx/ty = translation
///
/// # Memory Layout
/// - Size: 64 bytes (16 × f32)
/// - Alignment: Natural alignment for f32
/// - Zero-copy conversions available via `From`/`Into` traits
#[derive(Debug, Clone, Copy)]
pub struct Matrix4 {
    /// Matrix elements in column-major order (16 floats)
    pub m: [f32; 16],
}

impl Matrix4 {
    /// Creates a new matrix from 16 values in column-major order.
    ///
    /// # Arguments
    /// * Values are specified in column-major order (column by column)
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        m00: f32, m01: f32, m02: f32, m03: f32,
        m10: f32, m11: f32, m12: f32, m13: f32,
        m20: f32, m21: f32, m22: f32, m23: f32,
        m30: f32, m31: f32, m32: f32, m33: f32,
    ) -> Self {
        Self {
            m: [
                m00, m01, m02, m03,
                m10, m11, m12, m13,
                m20, m21, m22, m23,
                m30, m31, m32, m33,
            ],
        }
    }

    /// Creates an identity matrix (no transformation).
    ///
    /// ```text
    /// [ 1  0  0  0 ]
    /// [ 0  1  0  0 ]
    /// [ 0  0  1  0 ]
    /// [ 0  0  0  1 ]
    /// ```
    #[inline]
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            m: [
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    /// Creates a translation matrix.
    ///
    /// For 2D, use `z = 0.0`.
    #[inline]
    pub fn translation(x: f32, y: f32, z: f32) -> Self {
        Self::new(
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            x,   y,   z,   1.0,
        )
    }

    /// Creates a scaling matrix.
    ///
    /// For 2D uniform scaling, use `scaling(s, s, 1.0)`.
    #[inline]
    pub fn scaling(x: f32, y: f32, z: f32) -> Self {
        Self::new(
            x,   0.0, 0.0, 0.0,
            0.0, y,   0.0, 0.0,
            0.0, 0.0, z,   0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a rotation matrix around the Z axis (for 2D rotation).
    ///
    /// # Arguments
    /// * `angle` - Rotation angle in radians (counter-clockwise)
    #[inline]
    pub fn rotation_z(angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(
            cos, sin, 0.0, 0.0,
            -sin, cos, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a rotation matrix around the X axis.
    ///
    /// # Arguments
    /// * `angle` - Rotation angle in radians
    #[inline]
    pub fn rotation_x(angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(
            1.0, 0.0, 0.0, 0.0,
            0.0, cos, sin, 0.0,
            0.0, -sin, cos, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a rotation matrix around the Y axis.
    ///
    /// # Arguments
    /// * `angle` - Rotation angle in radians
    #[inline]
    pub fn rotation_y(angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(
            cos, 0.0, -sin, 0.0,
            0.0, 1.0, 0.0, 0.0,
            sin, 0.0, cos, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Creates a 2D skew (shear) transformation matrix.
    ///
    /// Skewing tilts the coordinate space while keeping sides parallel (parallelogram effect).
    /// Commonly used for italic-style text or perspective-like distortions.
    ///
    /// # Arguments
    /// * `skew_x` - Horizontal skew angle in radians (affects X based on Y)
    /// * `skew_y` - Vertical skew angle in radians (affects Y based on X)
    ///
    /// # Matrix Form
    /// ```text
    /// | 1         tan(skew_x)  0  0 |
    /// | tan(skew_y)  1         0  0 |
    /// | 0            0         1  0 |
    /// | 0            0         0  1 |
    /// ```
    ///
    /// # Examples
    /// ```
    /// use flui_types::Matrix4;
    ///
    /// // Horizontal skew (italic effect)
    /// let italic = Matrix4::skew_2d(0.3, 0.0);
    ///
    /// // Vertical skew
    /// let vertical_skew = Matrix4::skew_2d(0.0, 0.2);
    ///
    /// // Combined skew (parallelogram)
    /// let combined = Matrix4::skew_2d(0.4, -0.2);
    /// ```
    #[inline]
    pub fn skew_2d(skew_x: f32, skew_y: f32) -> Self {
        let tan_x = skew_x.tan();
        let tan_y = skew_y.tan();

        Self::new(
            1.0,   tan_y, 0.0, 0.0,
            tan_x, 1.0,   0.0, 0.0,
            0.0,   0.0,   1.0, 0.0,
            0.0,   0.0,   0.0, 1.0,
        )
    }

    /// Returns whether this is an identity matrix.
    ///
    /// Uses epsilon comparison for floating-point tolerance.
    #[inline]
    pub fn is_identity(&self) -> bool {
        self.is_identity_with_epsilon(1e-5)
    }

    /// Returns whether this is an identity matrix with custom epsilon.
    pub fn is_identity_with_epsilon(&self, epsilon: f32) -> bool {
        let identity = Self::identity();
        for i in 0..16 {
            if (self.m[i] - identity.m[i]).abs() > epsilon {
                return false;
            }
        }
        true
    }

    /// Returns the translation component (tx, ty, tz).
    #[inline]
    pub fn translation_component(&self) -> (f32, f32, f32) {
        (self.m[12], self.m[13], self.m[14])
    }

    /// Sets the translation component.
    #[inline]
    pub fn set_translation(&mut self, x: f32, y: f32, z: f32) {
        self.m[12] = x;
        self.m[13] = y;
        self.m[14] = z;
    }

    /// Translates this matrix by (x, y, z).
    ///
    /// This is equivalent to `self = Matrix4::translation(x, y, z) * self`.
    #[inline]
    pub fn translate(&mut self, x: f32, y: f32, z: f32) {
        *self = Matrix4::translation(x, y, z) * *self;
    }

    /// Scales this matrix by (x, y, z).
    ///
    /// This is equivalent to `self = Matrix4::scaling(x, y, z) * self`.
    #[inline]
    pub fn scale(&mut self, x: f32, y: f32, z: f32) {
        *self = Matrix4::scaling(x, y, z) * *self;
    }

    /// Rotates this matrix around the Z axis.
    ///
    /// This is equivalent to `self = Matrix4::rotation_z(angle) * self`.
    #[inline]
    pub fn rotate_z(&mut self, angle: f32) {
        *self = Matrix4::rotation_z(angle) * *self;
    }

    /// Transforms a 2D point (x, y) by this matrix.
    ///
    /// Uses homogeneous coordinates: (x, y, 0, 1) → (x', y', z', w')
    /// Returns (x'/w', y'/w').
    pub fn transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        let x_out = self.m[0] * x + self.m[4] * y + self.m[12];
        let y_out = self.m[1] * x + self.m[5] * y + self.m[13];
        let w_out = self.m[3] * x + self.m[7] * y + self.m[15];

        if w_out.abs() > f32::EPSILON {
            (x_out / w_out, y_out / w_out)
        } else {
            (x_out, y_out)
        }
    }

    /// Converts to column-major array (egui format).
    #[inline]
    #[must_use]
    pub const fn to_col_major_array(&self) -> [f32; 16] {
        self.m
    }

    /// Returns the transpose of this matrix (rows ↔ columns).
    ///
    /// For orthogonal matrices (rotations), transpose equals inverse.
    #[must_use]
    pub fn transpose(&self) -> Self {
        let m = &self.m;
        Self::new(
            m[0], m[4], m[8],  m[12],
            m[1], m[5], m[9],  m[13],
            m[2], m[6], m[10], m[14],
            m[3], m[7], m[11], m[15],
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

    /// Converts to row-major 2D array `[[f32; 4]; 4]`.
    ///
    /// Useful for interoperability with row-major libraries.
    #[must_use]
    pub fn to_row_major_2d(&self) -> [[f32; 4]; 4] {
        [
            [self.m[0], self.m[4], self.m[8],  self.m[12]],
            [self.m[1], self.m[5], self.m[9],  self.m[13]],
            [self.m[2], self.m[6], self.m[10], self.m[14]],
            [self.m[3], self.m[7], self.m[11], self.m[15]],
        ]
    }

    /// Converts to column-major 2D array `[[f32; 4]; 4]`.
    #[must_use]
    pub fn to_col_major_2d(&self) -> [[f32; 4]; 4] {
        [
            [self.m[0], self.m[1], self.m[2],  self.m[3]],
            [self.m[4], self.m[5], self.m[6],  self.m[7]],
            [self.m[8], self.m[9], self.m[10], self.m[11]],
            [self.m[12], self.m[13], self.m[14], self.m[15]],
        ]
    }

    /// Returns a reference to element at (row, col).
    ///
    /// # Panics
    /// Panics if row or col >= 4.
    #[inline]
    #[must_use]
    pub fn get(&self, row: usize, col: usize) -> f32 {
        assert!(row < 4 && col < 4, "Matrix index out of bounds");
        self.m[col * 4 + row]
    }

    /// Returns a mutable reference to element at (row, col).
    ///
    /// # Panics
    /// Panics if row or col >= 4.
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
        let mut result = *self;
        let mut inv = Self::identity();

        // Gauss-Jordan elimination with partial pivoting
        for i in 0..4 {
            // Find pivot
            let mut max_row = i;
            let mut max_val = result.m[i * 4 + i].abs();

            for k in (i + 1)..4 {
                let val = result.m[i * 4 + k].abs();
                if val > max_val {
                    max_val = val;
                    max_row = k;
                }
            }

            // Check for singular matrix
            if max_val < f32::EPSILON {
                return None;
            }

            // Swap rows if needed
            if max_row != i {
                for j in 0..4 {
                    result.m.swap(j * 4 + i, j * 4 + max_row);
                    inv.m.swap(j * 4 + i, j * 4 + max_row);
                }
            }

            // Scale pivot row
            let pivot = result.m[i * 4 + i];
            for j in 0..4 {
                result.m[j * 4 + i] /= pivot;
                inv.m[j * 4 + i] /= pivot;
            }

            // Eliminate column
            for k in 0..4 {
                if k != i {
                    let factor = result.m[i * 4 + k];
                    for j in 0..4 {
                        result.m[j * 4 + k] -= factor * result.m[j * 4 + i];
                        inv.m[j * 4 + k] -= factor * inv.m[j * 4 + i];
                    }
                }
            }
        }

        Some(inv)
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
        // Cofactor expansion along first row
        let m = &self.m;

        let a0 = m[0] * (m[5] * (m[10] * m[15] - m[11] * m[14]) -
                         m[9] * (m[6] * m[15] - m[7] * m[14]) +
                         m[13] * (m[6] * m[11] - m[7] * m[10]));

        let a1 = m[4] * (m[1] * (m[10] * m[15] - m[11] * m[14]) -
                         m[9] * (m[2] * m[15] - m[3] * m[14]) +
                         m[13] * (m[2] * m[11] - m[3] * m[10]));

        let a2 = m[8] * (m[1] * (m[6] * m[15] - m[7] * m[14]) -
                         m[5] * (m[2] * m[15] - m[3] * m[14]) +
                         m[13] * (m[2] * m[7] - m[3] * m[6]));

        let a3 = m[12] * (m[1] * (m[6] * m[11] - m[7] * m[10]) -
                          m[5] * (m[2] * m[11] - m[3] * m[10]) +
                          m[9] * (m[2] * m[7] - m[3] * m[6]));

        a0 - a1 + a2 - a3
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
    /// Returns whether two matrices are approximately equal within epsilon.
    ///
    /// Uses element-wise comparison with the specified tolerance.
    ///
    /// # Example
    /// ```
    /// use flui_types::Matrix4;
    ///
    /// let m1 = Matrix4::translation(1.0, 2.0, 0.0);
    /// let m2 = Matrix4::translation(1.0000001, 2.0, 0.0);
    ///
    /// assert!(m1.approx_eq_eps(&m2, 0.001));
    /// ```
    #[must_use]
    pub fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        for i in 0..16 {
            if (self.m[i] - other.m[i]).abs() > epsilon {
                return false;
            }
        }
        true
    }

    /// Returns whether two matrices are approximately equal (epsilon = 1e-5).
    #[inline]
    #[must_use]
    pub fn approx_eq(&self, other: &Self) -> bool {
        self.approx_eq_eps(other, 1e-5)
    }
}

/// Matrix multiplication: C = A * B
///
/// Matrices are applied right-to-left: (A * B) transforms first by B, then by A.
impl Mul for Matrix4 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        let mut result = [0.0; 16];

        // Column-major matrix multiplication
        // result[col][row] = sum of self[k][row] * rhs[col][k]
        for col in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    // Column-major: m[col*4 + row]
                    sum += self.m[k * 4 + row] * rhs.m[col * 4 + k];
                }
                result[col * 4 + row] = sum;
            }
        }

        Self { m: result }
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
/// use flui_types::Matrix4;
/// let m = Matrix4::identity();
/// assert_eq!(m[0], 1.0);  // m00
/// assert_eq!(m[5], 1.0);  // m11
/// ```
impl Index<usize> for Matrix4 {
    type Output = f32;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.m[index]
    }
}

/// Mutably access matrix elements by linear index (0..16) in column-major order.
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
                arr[0][0], arr[0][1], arr[0][2], arr[0][3],
                arr[1][0], arr[1][1], arr[1][2], arr[1][3],
                arr[2][0], arr[2][1], arr[2][2], arr[2][3],
                arr[3][0], arr[3][1], arr[3][2], arr[3][3],
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
mod tests {
    use super::*;

    #[test]
    fn test_matrix4_identity() {
        let m = Matrix4::identity();
        assert_eq!(m.m[0], 1.0);
        assert_eq!(m.m[5], 1.0);
        assert_eq!(m.m[10], 1.0);
        assert_eq!(m.m[15], 1.0);
        assert!(m.is_identity());
    }

    #[test]
    fn test_matrix4_default() {
        let m = Matrix4::default();
        assert!(m.is_identity());
    }

    #[test]
    fn test_matrix4_translation() {
        let m = Matrix4::translation(10.0, 20.0, 0.0);
        assert_eq!(m.m[12], 10.0);
        assert_eq!(m.m[13], 20.0);
        assert_eq!(m.m[14], 0.0);

        let (tx, ty, tz) = m.translation_component();
        assert_eq!(tx, 10.0);
        assert_eq!(ty, 20.0);
        assert_eq!(tz, 0.0);
    }

    #[test]
    fn test_matrix4_scaling() {
        let m = Matrix4::scaling(2.0, 3.0, 1.0);
        assert_eq!(m.m[0], 2.0);
        assert_eq!(m.m[5], 3.0);
        assert_eq!(m.m[10], 1.0);
    }

    #[test]
    fn test_matrix4_rotation_z() {
        let m = Matrix4::rotation_z(std::f32::consts::PI / 2.0); // 90 degrees

        // cos(90°) ≈ 0, sin(90°) ≈ 1
        assert!((m.m[0] - 0.0).abs() < 0.0001);
        assert!((m.m[1] - 1.0).abs() < 0.0001);
        assert!((m.m[4] - (-1.0)).abs() < 0.0001);
        assert!((m.m[5] - 0.0).abs() < 0.0001);
    }

    #[test]
    fn test_matrix4_transform_point() {
        // Translation
        let translate = Matrix4::translation(10.0, 20.0, 0.0);
        let (x, y) = translate.transform_point(5.0, 3.0);
        assert_eq!(x, 15.0);
        assert_eq!(y, 23.0);

        // Scaling
        let scale = Matrix4::scaling(2.0, 3.0, 1.0);
        let (x, y) = scale.transform_point(4.0, 5.0);
        assert_eq!(x, 8.0);
        assert_eq!(y, 15.0);
    }

    #[test]
    fn test_matrix4_multiply() {
        let translate = Matrix4::translation(10.0, 20.0, 0.0);
        let scale = Matrix4::scaling(2.0, 2.0, 1.0);

        // Combine: translate then scale
        let combined = translate * scale;

        // Transform point (0, 0): scale first -> (0, 0), then translate -> (10, 20)
        let (x, y) = combined.transform_point(0.0, 0.0);
        assert_eq!(x, 10.0);
        assert_eq!(y, 20.0);
    }

    #[test]
    fn test_matrix4_set_translation() {
        let mut m = Matrix4::identity();
        m.set_translation(5.0, 10.0, 15.0);

        let (tx, ty, tz) = m.translation_component();
        assert_eq!(tx, 5.0);
        assert_eq!(ty, 10.0);
        assert_eq!(tz, 15.0);
    }

    #[test]
    fn test_matrix4_translate() {
        let mut m = Matrix4::identity();
        m.translate(10.0, 20.0, 0.0);

        let (x, y) = m.transform_point(0.0, 0.0);
        assert_eq!(x, 10.0);
        assert_eq!(y, 20.0);
    }

    #[test]
    fn test_matrix4_scale() {
        let mut m = Matrix4::identity();
        m.scale(2.0, 3.0, 1.0);

        let (x, y) = m.transform_point(4.0, 5.0);
        assert_eq!(x, 8.0);
        assert_eq!(y, 15.0);
    }

    #[test]
    fn test_matrix4_rotate_z() {
        let mut m = Matrix4::identity();
        m.rotate_z(std::f32::consts::PI / 2.0); // 90 degrees

        // Rotate point (1, 0) by 90° -> (0, 1)
        let (x, y) = m.transform_point(1.0, 0.0);
        assert!((x - 0.0).abs() < 0.0001);
        assert!((y - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_matrix4_mul_assign() {
        let mut m = Matrix4::identity();
        let translate = Matrix4::translation(5.0, 10.0, 0.0);

        m *= translate;

        let (x, y) = m.transform_point(0.0, 0.0);
        assert_eq!(x, 5.0);
        assert_eq!(y, 10.0);
    }

    #[test]
    fn test_matrix4_to_col_major_array() {
        let m = Matrix4::translation(1.0, 2.0, 3.0);
        let arr = m.to_col_major_array();

        assert_eq!(arr.len(), 16);
        assert_eq!(arr[12], 1.0); // tx
        assert_eq!(arr[13], 2.0); // ty
        assert_eq!(arr[14], 3.0); // tz
    }

    #[test]
    fn test_matrix4_combined_transformations() {
        // Translate (10, 20), then scale (2, 2), then rotate 90°
        let translate = Matrix4::translation(10.0, 20.0, 0.0);
        let scale = Matrix4::scaling(2.0, 2.0, 1.0);
        let rotate = Matrix4::rotation_z(std::f32::consts::PI / 2.0);

        let combined = rotate * scale * translate;

        // Transform point (0, 0)
        let (x, y) = combined.transform_point(0.0, 0.0);

        // Expected: (0,0) -> translate -> (10, 20) -> scale -> (20, 40) -> rotate 90° -> (-40, 20)
        assert!((x - (-40.0)).abs() < 0.01);
        assert!((y - 20.0).abs() < 0.01);
    }

    #[test]
    fn test_matrix4_inverse_identity() {
        let m = Matrix4::identity();
        let inv = m.try_inverse().unwrap();
        assert!(inv.is_identity());
    }

    #[test]
    fn test_matrix4_inverse_translation() {
        let m = Matrix4::translation(10.0, 20.0, 0.0);
        let inv = m.try_inverse().unwrap();

        // Inverse of translation(10, 20) should be translation(-10, -20)
        let (tx, ty, _) = inv.translation_component();
        assert!((tx - (-10.0)).abs() < 0.001);
        assert!((ty - (-20.0)).abs() < 0.001);

        // Verify: m * inv = identity
        let product = m * inv;
        assert!(product.is_identity());
    }

    #[test]
    fn test_matrix4_inverse_scaling() {
        let m = Matrix4::scaling(2.0, 4.0, 1.0);
        let inv = m.try_inverse().unwrap();

        // Inverse of scaling(2, 4, 1) should be scaling(0.5, 0.25, 1)
        assert!((inv.m[0] - 0.5).abs() < 0.001);
        assert!((inv.m[5] - 0.25).abs() < 0.001);
        assert!((inv.m[10] - 1.0).abs() < 0.001);

        // Verify: m * inv = identity
        let product = m * inv;
        assert!(product.is_identity());
    }

    #[test]
    fn test_matrix4_inverse_rotation() {
        let angle = std::f32::consts::PI / 4.0; // 45 degrees
        let m = Matrix4::rotation_z(angle);
        let inv = m.try_inverse().unwrap();

        // Verify: m * inv = identity
        let product = m * inv;
        assert!(product.is_identity());

        // Inverse rotation should rotate back
        let (x, y) = m.transform_point(1.0, 0.0);
        let (x2, y2) = inv.transform_point(x, y);
        assert!((x2 - 1.0).abs() < 0.001);
        assert!((y2 - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_matrix4_invert_in_place() {
        let mut m = Matrix4::translation(5.0, 10.0, 0.0);
        let original = m;

        assert!(m.invert());

        // Verify: m is now inverted
        let product = original * m;
        assert!(product.is_identity());
    }

    #[test]
    fn test_matrix4_determinant_identity() {
        let m = Matrix4::identity();
        assert!((m.determinant() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_matrix4_determinant_translation() {
        let m = Matrix4::translation(10.0, 20.0, 0.0);
        // Translation matrices have determinant = 1
        assert!((m.determinant() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_matrix4_determinant_scaling() {
        let m = Matrix4::scaling(2.0, 3.0, 4.0);
        // Determinant of scaling matrix = product of scale factors
        assert!((m.determinant() - 24.0).abs() < 0.001);
    }

    #[test]
    fn test_matrix4_display() {
        let m = Matrix4::translation(1.0, 2.0, 3.0);
        let display = format!("{}", m);
        assert!(display.contains("Matrix4"));
        assert!(display.contains("1.000"));
        assert!(display.contains("2.000"));
        assert!(display.contains("3.000"));
    }

    #[test]
    fn test_matrix4_transpose() {
        let m = Matrix4::new(
            1.0,  2.0,  3.0,  4.0,
            5.0,  6.0,  7.0,  8.0,
            9.0,  10.0, 11.0, 12.0,
            13.0, 14.0, 15.0, 16.0,
        );

        let t = m.transpose();

        // Verify transpose swaps rows and columns
        assert_eq!(t.get(0, 0), m.get(0, 0)); // diagonal unchanged
        assert_eq!(t.get(0, 1), m.get(1, 0));
        assert_eq!(t.get(1, 0), m.get(0, 1));
        assert_eq!(t.get(2, 3), m.get(3, 2));
    }

    #[test]
    fn test_matrix4_transpose_rotation() {
        // For rotation matrices, transpose should equal inverse
        let angle = std::f32::consts::PI / 4.0;
        let m = Matrix4::rotation_z(angle);
        let t = m.transpose();
        let inv = m.try_inverse().unwrap();

        for i in 0..16 {
            assert!((t.m[i] - inv.m[i]).abs() < 0.001);
        }
    }

    #[test]
    fn test_matrix4_index() {
        let m = Matrix4::identity();
        assert_eq!(m[0], 1.0);
        assert_eq!(m[5], 1.0);
        assert_eq!(m[10], 1.0);
        assert_eq!(m[15], 1.0);
    }

    #[test]
    fn test_matrix4_index_mut() {
        let mut m = Matrix4::identity();
        m[12] = 5.0;
        m[13] = 10.0;

        let (tx, ty, _) = m.translation_component();
        assert_eq!(tx, 5.0);
        assert_eq!(ty, 10.0);
    }

    #[test]
    fn test_matrix4_get() {
        let m = Matrix4::translation(10.0, 20.0, 30.0);
        assert_eq!(m.get(0, 3), 10.0); // tx
        assert_eq!(m.get(1, 3), 20.0); // ty
        assert_eq!(m.get(2, 3), 30.0); // tz
    }

    #[test]
    fn test_matrix4_get_mut() {
        let mut m = Matrix4::identity();
        *m.get_mut(0, 3) = 15.0;

        let (tx, _, _) = m.translation_component();
        assert_eq!(tx, 15.0);
    }

    #[test]
    fn test_matrix4_from_array() {
        let arr = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            5.0, 10.0, 0.0, 1.0,
        ];
        let m = Matrix4::from(arr);

        let (tx, ty, _) = m.translation_component();
        assert_eq!(tx, 5.0);
        assert_eq!(ty, 10.0);
    }

    #[test]
    fn test_matrix4_into_array() {
        let m = Matrix4::translation(3.0, 6.0, 9.0);
        let arr: [f32; 16] = m.into();

        assert_eq!(arr[12], 3.0);
        assert_eq!(arr[13], 6.0);
        assert_eq!(arr[14], 9.0);
    }

    #[test]
    fn test_matrix4_from_2d_array() {
        let arr = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [7.0, 8.0, 9.0, 1.0],
        ];
        let m = Matrix4::from(arr);

        let (tx, ty, tz) = m.translation_component();
        assert_eq!(tx, 7.0);
        assert_eq!(ty, 8.0);
        assert_eq!(tz, 9.0);
    }

    #[test]
    fn test_matrix4_to_row_major_2d() {
        let m = Matrix4::translation(1.0, 2.0, 3.0);
        let arr = m.to_row_major_2d();

        assert_eq!(arr[0][3], 1.0); // tx
        assert_eq!(arr[1][3], 2.0); // ty
        assert_eq!(arr[2][3], 3.0); // tz
    }

    #[test]
    fn test_matrix4_as_ref() {
        let m = Matrix4::identity();
        let slice: &[f32; 16] = m.as_ref();
        assert_eq!(slice[0], 1.0);
        assert_eq!(slice[15], 1.0);
    }

    #[test]
    fn test_matrix4_as_mut() {
        let mut m = Matrix4::identity();
        let slice: &mut [f32; 16] = m.as_mut();
        slice[12] = 42.0;

        let (tx, _, _) = m.translation_component();
        assert_eq!(tx, 42.0);
    }

    #[test]
    fn test_matrix4_const_identity() {
        // Verify identity() is const
        const IDENTITY: Matrix4 = Matrix4::identity();
        assert_eq!(IDENTITY.m[0], 1.0);
        assert_eq!(IDENTITY.m[5], 1.0);
    }

    #[test]
    fn test_matrix4_transpose_in_place() {
        let mut m = Matrix4::new(
            1.0,  2.0,  3.0,  4.0,
            5.0,  6.0,  7.0,  8.0,
            9.0,  10.0, 11.0, 12.0,
            13.0, 14.0, 15.0, 16.0,
        );

        let original = m;
        m.transpose_in_place();

        // Verify transpose swaps rows and columns
        assert_eq!(m.get(0, 1), original.get(1, 0));
        assert_eq!(m.get(1, 0), original.get(0, 1));
        assert_eq!(m.get(2, 3), original.get(3, 2));

        // Transpose twice should give original
        m.transpose_in_place();
        assert_eq!(m, original);
    }

    #[test]
    fn test_matrix4_approx_eq() {
        let m1 = Matrix4::translation(1.0, 2.0, 0.0);
        let m2 = Matrix4::translation(1.0 + 1e-6, 2.0, 0.0);

        assert!(m1.approx_eq(&m2));
        assert!(!m1.eq(&m2)); // Exact equality should fail
    }

    #[test]
    fn test_matrix4_approx_eq_eps() {
        let m1 = Matrix4::scaling(2.0, 3.0, 1.0);
        let m2 = Matrix4::scaling(2.01, 3.0, 1.0);

        assert!(!m1.approx_eq(&m2)); // Default epsilon too small
        assert!(m1.approx_eq_eps(&m2, 0.02)); // Larger epsilon passes
    }

    #[test]
    fn test_matrix4_eq_exact() {
        let m1 = Matrix4::identity();
        let m2 = Matrix4::identity();

        assert_eq!(m1, m2);
    }
}
