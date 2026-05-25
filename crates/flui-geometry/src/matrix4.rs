//! 4x4 transformation matrix for 2D and 3D transformations.
//!
//! `Matrix4` is a `#[repr(transparent)]` newtype around [`glam::Mat4`].
//! It exposes the column-major Flutter-parity public API the rest of the
//! framework was written against while delegating the hot-path math to
//! glam's portable-SIMD-ready primitives.
//!
//! # Memory Layout
//!
//! `Matrix4` is `#[repr(transparent)]` over `glam::Mat4`, which is itself
//! `#[repr(C)]` and stores four column vectors back-to-back. The result is
//! identical to a `[f32; 16]` in column-major order:
//!
//! ```text
//! | m0  m4  m8  m12 |
//! | m1  m5  m9  m13 |
//! | m2  m6  m10 m14 |
//! | m3  m7  m11 m15 |
//! ```
//!
//! `size_of::<Matrix4>() == size_of::<glam::Mat4>() == 64` bytes, verified
//! by the `matrix4_repr_transparent_size_of` test in this file.
//!
//! # Design Philosophy
//!
//! Wrapping rather than re-implementing keeps the framework boundary
//! ("flui owns unit-typed wrappers for polish discipline; glam handles
//! SIMD math") explicit. The wrapper:
//!
//! - **Unit-typed surface**: `Pixels` / `Rect<Pixels>` / `Point<Pixels>`
//!   stay on the boundary; raw `f32` only crosses inside the wrapper.
//! - **Zero overhead**: `repr(transparent)` guarantees the wrapper costs
//!   nothing at runtime; the optimizer sees `glam::Mat4` directly.
//! - **One place to track upstream**: changes in glam's SIMD strategy or
//!   accelerator support land here transparently.
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
//! assert!(m1.approx_eq(&m2));
//! ```

use std::{
    fmt,
    ops::{Index, IndexMut, Mul, MulAssign},
};

use glam::{Mat4, Vec3};

use super::Pixels;
use crate::Rect;

/// A 4x4 transformation matrix stored in column-major order.
///
/// Used for affine transformations including translation, rotation, scaling,
/// and skewing. The matrix is a `#[repr(transparent)]` newtype around
/// [`glam::Mat4`]; see the module-level docs for the memory layout and the
/// rationale behind the wrapping.
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct Matrix4(pub Mat4);

impl Matrix4 {
    /// Identity matrix constant (no transformation).
    pub const IDENTITY: Self = Self(Mat4::IDENTITY);

    /// Zero matrix constant (all elements are zero).
    pub const ZERO: Self = Self(Mat4::ZERO);
}

impl Matrix4 {
    /// Creates a new matrix from 16 elements in column-major order.
    ///
    /// Parameters are named as `mRC` where R is the row index and C is the
    /// column index (both 0-indexed); the array layout follows OpenGL /
    /// Flutter `Float32List` convention.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
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
        Self(Mat4::from_cols_array(&[
            m00, m01, m02, m03, m10, m11, m12, m13, m20, m21, m22, m23, m30, m31, m32, m33,
        ]))
    }

    /// Creates an identity matrix (no transformation).
    #[inline]
    #[must_use]
    pub const fn identity() -> Self {
        Self(Mat4::IDENTITY)
    }

    /// Creates a translation matrix.
    ///
    /// For 2D transformations, use `z = 0.0`.
    #[inline]
    #[must_use]
    pub fn translation(x: f32, y: f32, z: f32) -> Self {
        Self(Mat4::from_translation(Vec3::new(x, y, z)))
    }

    /// Creates a uniform or non-uniform scaling matrix.
    ///
    /// For 2D transformations, use `z = 1.0`.
    #[inline]
    #[must_use]
    pub fn scaling(x: f32, y: f32, z: f32) -> Self {
        Self(Mat4::from_scale(Vec3::new(x, y, z)))
    }

    /// Creates a rotation matrix around the Z axis (for 2D rotations).
    ///
    /// Angle is in radians. Positive values rotate counter-clockwise.
    #[inline]
    #[must_use]
    pub fn rotation_z(angle: f32) -> Self {
        Self(Mat4::from_rotation_z(angle))
    }

    /// Creates a rotation matrix around the Z axis (type-safe version).
    #[inline]
    #[must_use]
    pub fn rotation_z_radians(angle: crate::Radians) -> Self {
        Self::rotation_z(angle.0)
    }

    /// Creates a rotation matrix around the X axis.
    ///
    /// Angle is in radians. Positive values rotate counter-clockwise when
    /// looking down the axis.
    #[inline]
    #[must_use]
    pub fn rotation_x(angle: f32) -> Self {
        Self(Mat4::from_rotation_x(angle))
    }

    /// Creates a rotation matrix around the X axis (type-safe version).
    #[inline]
    #[must_use]
    pub fn rotation_x_radians(angle: crate::Radians) -> Self {
        Self::rotation_x(angle.0)
    }

    /// Creates a rotation matrix around the Y axis.
    ///
    /// Angle is in radians. Positive values rotate counter-clockwise when
    /// looking down the axis.
    #[inline]
    #[must_use]
    pub fn rotation_y(angle: f32) -> Self {
        Self(Mat4::from_rotation_y(angle))
    }

    /// Creates a rotation matrix around the Y axis (type-safe version).
    #[inline]
    #[must_use]
    pub fn rotation_y_radians(angle: crate::Radians) -> Self {
        Self::rotation_y(angle.0)
    }

    /// Creates a 2D skew (shear) matrix.
    ///
    /// - `skew_x`: Skew angle along the X axis (in radians)
    /// - `skew_y`: Skew angle along the Y axis (in radians)
    #[inline]
    #[must_use]
    pub fn skew_2d(skew_x: f32, skew_y: f32) -> Self {
        Self::new(
            1.0,
            skew_y.tan(),
            0.0,
            0.0,
            skew_x.tan(),
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        )
    }

    /// Alias for `skew_2d`.
    #[inline]
    #[must_use]
    pub fn skew(skew_x: f32, skew_y: f32) -> Self {
        Self::skew_2d(skew_x, skew_y)
    }

    /// Returns whether this is an identity matrix within a 1e-5 epsilon.
    #[inline]
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.is_identity_with_epsilon(1e-5)
    }

    /// Returns whether this is an identity matrix with custom epsilon.
    #[must_use]
    pub fn is_identity_with_epsilon(&self, epsilon: f32) -> bool {
        self.approx_eq_eps(&Self::IDENTITY, epsilon)
    }

    /// Returns whether this matrix represents only a translation within a
    /// `f32::EPSILON` tolerance.
    #[inline]
    #[must_use]
    pub fn is_translation_only(&self) -> bool {
        self.is_translation_only_with_epsilon(f32::EPSILON)
    }

    /// Returns whether this matrix represents only a translation with
    /// custom epsilon.
    ///
    /// Checks that the 3x3 upper-left submatrix is identity (within
    /// epsilon) and the perspective row is `[0, 0, 0, 1]`.
    #[must_use]
    pub fn is_translation_only_with_epsilon(&self, epsilon: f32) -> bool {
        let a = self.to_col_major_array();
        let i = Self::IDENTITY.to_col_major_array();
        // Diagonal + upper-left 3x3 + perspective row checks. Indices 12..15
        // (translation column rows 0..3) intentionally skipped; index 15
        // checked because it must remain 1 even when translation is
        // present.
        for k in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 15] {
            if (a[k] - i[k]).abs() > epsilon {
                return false;
            }
        }
        true
    }

    /// Extracts the translation component `(x, y, z)` from the matrix.
    #[inline]
    #[must_use]
    pub fn translation_component(&self) -> (f32, f32, f32) {
        let t = self.0.w_axis;
        (t.x, t.y, t.z)
    }

    /// Sets the translation component without affecting other
    /// transformations.
    #[inline]
    pub fn set_translation(&mut self, x: f32, y: f32, z: f32) {
        self.0.w_axis.x = x;
        self.0.w_axis.y = y;
        self.0.w_axis.z = z;
    }

    /// Applies a translation to this matrix (modifies in place).
    #[inline]
    pub fn translate(&mut self, x: f32, y: f32, z: f32) {
        *self = Self::translation(x, y, z) * *self;
    }

    /// Applies a scaling to this matrix (modifies in place).
    #[inline]
    pub fn scale(&mut self, x: f32, y: f32, z: f32) {
        *self = Self::scaling(x, y, z) * *self;
    }

    /// Applies a Z-axis rotation to this matrix (modifies in place).
    #[inline]
    pub fn rotate_z(&mut self, angle: f32) {
        *self = Self::rotation_z(angle) * *self;
    }

    /// Applies a Z-axis rotation to this matrix (type-safe version,
    /// modifies in place).
    #[inline]
    pub fn rotate_z_radians(&mut self, angle: crate::Radians) {
        self.rotate_z(angle.0);
    }

    /// Transforms a 2D point `(x, y)` by this matrix.
    ///
    /// Uses homogeneous coordinates: `(x, y, 0, 1) → (x', y', z', w')`,
    /// returning `(x'/w', y'/w')`. When `w'` is within `f32::EPSILON` of
    /// zero the projection is skipped to avoid division-by-zero.
    #[must_use]
    pub fn transform_point(&self, x: Pixels, y: Pixels) -> (Pixels, Pixels) {
        let a = self.to_col_major_array();
        let x_out = a[0] * x.0 + a[4] * y.0 + a[12];
        let y_out = a[1] * x.0 + a[5] * y.0 + a[13];
        let w_out = a[3] * x.0 + a[7] * y.0 + a[15];

        if w_out.abs() > f32::EPSILON {
            (Pixels(x_out / w_out), Pixels(y_out / w_out))
        } else {
            (Pixels(x_out), Pixels(y_out))
        }
    }

    /// Transforms a rectangle by this matrix, returning the bounding box of
    /// the result.
    ///
    /// All four corners are transformed and an axis-aligned bounding box is
    /// computed around the result.
    #[must_use]
    pub fn transform_rect(&self, rect: &Rect<Pixels>) -> Rect<Pixels> {
        let (x0, y0) = self.transform_point(rect.min.x, rect.min.y);
        let (x1, y1) = self.transform_point(rect.max.x, rect.min.y);
        let (x2, y2) = self.transform_point(rect.min.x, rect.max.y);
        let (x3, y3) = self.transform_point(rect.max.x, rect.max.y);

        let min_x = x0.min(x1).min(x2).min(x3);
        let min_y = y0.min(y1).min(y2).min(y3);
        let max_x = x0.max(x1).max(x2).max(x3);
        let max_y = y0.max(y1).max(y2).max(y3);

        Rect::from_ltrb(min_x, min_y, max_x, max_y)
    }

    /// Returns the matrix as a column-major array.
    #[inline]
    #[must_use]
    pub fn to_col_major_array(&self) -> [f32; 16] {
        self.0.to_cols_array()
    }

    /// Returns the transpose of this matrix.
    #[inline]
    #[must_use]
    pub fn transpose(&self) -> Self {
        Self(self.0.transpose())
    }

    /// Transposes this matrix in place.
    #[inline]
    pub fn transpose_in_place(&mut self) {
        *self = self.transpose();
    }

    /// Returns the matrix as a row-major 4x4 array.
    #[inline]
    #[must_use]
    pub fn to_row_major_2d(&self) -> [[f32; 4]; 4] {
        self.transpose().to_col_major_2d()
    }

    /// Returns the matrix as a column-major 4x4 array.
    #[inline]
    #[must_use]
    pub fn to_col_major_2d(&self) -> [[f32; 4]; 4] {
        self.0.to_cols_array_2d()
    }

    /// Gets the element at `(row, col)`.
    ///
    /// # Panics
    ///
    /// Panics if `row` or `col` is greater than or equal to `4`.
    #[inline]
    #[must_use]
    pub fn get(&self, row: usize, col: usize) -> f32 {
        assert!(row < 4 && col < 4, "Matrix4 index out of bounds");
        self.to_col_major_array()[col * 4 + row]
    }

    /// Returns a mutable reference to the element at `(row, col)`.
    ///
    /// # Panics
    ///
    /// Panics if `row` or `col` is greater than or equal to `4`.
    #[inline]
    pub fn get_mut(&mut self, row: usize, col: usize) -> &mut f32 {
        assert!(row < 4 && col < 4, "Matrix4 index out of bounds");
        &mut self[col * 4 + row]
    }

    /// Returns the inverse of this matrix, or `None` if the matrix is
    /// singular (`|determinant| < f32::EPSILON`).
    #[must_use]
    pub fn try_inverse(&self) -> Option<Self> {
        let det = self.determinant();
        if det.abs() < f32::EPSILON {
            None
        } else {
            // glam's `inverse` is correct when the matrix is non-singular;
            // the determinant check above gates the call.
            Some(Self(self.0.inverse()))
        }
    }

    /// Inverts this matrix in place, returning `true` on success and
    /// `false` if the matrix was singular (in which case the matrix is
    /// left unchanged).
    pub fn invert(&mut self) -> bool {
        match self.try_inverse() {
            Some(inv) => {
                *self = inv;
                true
            }
            None => false,
        }
    }

    /// Returns the determinant of this matrix.
    #[inline]
    #[must_use]
    pub fn determinant(&self) -> f32 {
        self.0.determinant()
    }

    /// Checks approximate equality with a custom epsilon.
    ///
    /// Returns `true` if every element differs by at most `epsilon`.
    #[must_use]
    pub fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        let a = self.to_col_major_array();
        let b = other.to_col_major_array();
        for i in 0..16 {
            if (a[i] - b[i]).abs() > epsilon {
                return false;
            }
        }
        true
    }

    /// Checks approximate equality with the default `1e-5` epsilon.
    #[inline]
    #[must_use]
    pub fn approx_eq(&self, other: &Self) -> bool {
        self.approx_eq_eps(other, 1e-5)
    }
}

impl Default for Matrix4 {
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Exact equality comparison (bitwise on the underlying floats).
///
/// For floating-point tolerance comparison, use [`Matrix4::approx_eq`] or
/// [`Matrix4::approx_eq_eps`].
impl PartialEq for Matrix4 {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.to_col_major_array() == other.to_col_major_array()
    }
}

impl Eq for Matrix4 {}

/// Matrix multiplication: `C = A * B`.
///
/// Matrices are applied right-to-left: `(A * B)` transforms first by `B`,
/// then by `A`. Delegates to [`glam::Mat4`]'s portable-SIMD-aware
/// multiplication; the framework's hand-written SSE / NEON paths were
/// removed in U14 in favour of the upstream implementation.
impl Mul for Matrix4 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        Self(self.0 * rhs.0)
    }
}

impl MulAssign for Matrix4 {
    #[inline]
    fn mul_assign(&mut self, rhs: Self) {
        self.0 *= rhs.0;
    }
}

/// Access matrix elements by linear index `0..16` in column-major order.
///
/// # Panics
///
/// Panics if `index` is out of range, matching the underlying `[f32; 16]`
/// bounds check.
///
/// # Example
///
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
        // SAFETY: `Matrix4` is `#[repr(transparent)]` over `glam::Mat4`,
        // which is `#[repr(C)]` and laid out as four contiguous `Vec4`
        // columns. The resulting 64-byte block matches `[f32; 16]`
        // exactly, so reinterpreting `&self` as `&[f32; 16]` borrows the
        // same bytes with the same lifetime. The lint allowance below
        // explains the same reasoning machine-readable for clippy.
        #[allow(unsafe_code, reason = "repr(transparent) over glam::Mat4 == [f32; 16]")]
        let arr: &[f32; 16] = unsafe { &*std::ptr::from_ref::<Self>(self).cast::<[f32; 16]>() };
        &arr[index]
    }
}

/// Mutably access matrix elements by linear index `0..16` in column-major
/// order.
///
/// # Panics
///
/// Panics if `index` is out of range.
impl IndexMut<usize> for Matrix4 {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        // SAFETY: see the `Index` impl above. `&mut self` yields the same
        // 64-byte block as `&mut [f32; 16]`; both views are alias-free
        // because the borrow checker enforces unique mutable access here.
        #[allow(unsafe_code, reason = "repr(transparent) over glam::Mat4 == [f32; 16]")]
        let arr: &mut [f32; 16] =
            unsafe { &mut *std::ptr::from_mut::<Self>(self).cast::<[f32; 16]>() };
        &mut arr[index]
    }
}

/// Construct from column-major array.
impl From<[f32; 16]> for Matrix4 {
    #[inline]
    fn from(m: [f32; 16]) -> Self {
        Self(Mat4::from_cols_array(&m))
    }
}

/// Convert to column-major array.
impl From<Matrix4> for [f32; 16] {
    #[inline]
    fn from(matrix: Matrix4) -> Self {
        matrix.0.to_cols_array()
    }
}

/// Construct from column-major 2D array.
impl From<[[f32; 4]; 4]> for Matrix4 {
    #[inline]
    fn from(arr: [[f32; 4]; 4]) -> Self {
        Self(Mat4::from_cols_array_2d(&arr))
    }
}

/// Convert to column-major 2D array.
impl From<Matrix4> for [[f32; 4]; 4] {
    #[inline]
    fn from(matrix: Matrix4) -> Self {
        matrix.0.to_cols_array_2d()
    }
}

/// Borrow as a column-major `[f32; 16]` slice.
impl AsRef<[f32; 16]> for Matrix4 {
    #[inline]
    fn as_ref(&self) -> &[f32; 16] {
        // SAFETY: same justification as `Index<usize>` above.
        #[allow(unsafe_code, reason = "repr(transparent) over glam::Mat4 == [f32; 16]")]
        unsafe {
            &*std::ptr::from_ref::<Self>(self).cast::<[f32; 16]>()
        }
    }
}

/// Mutably borrow as a column-major `[f32; 16]` slice.
impl AsMut<[f32; 16]> for Matrix4 {
    #[inline]
    fn as_mut(&mut self) -> &mut [f32; 16] {
        // SAFETY: same justification as `IndexMut<usize>` above.
        #[allow(unsafe_code, reason = "repr(transparent) over glam::Mat4 == [f32; 16]")]
        unsafe {
            &mut *std::ptr::from_mut::<Self>(self).cast::<[f32; 16]>()
        }
    }
}

impl fmt::Display for Matrix4 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let a = self.to_col_major_array();
        writeln!(f, "Matrix4 [")?;
        for row in 0..4 {
            write!(f, "  [")?;
            for col in 0..4 {
                if col > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{:8.3}", a[col * 4 + row])?;
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
        self.to_col_major_array().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Matrix4 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let m = <[f32; 16]>::deserialize(deserializer)?;
        Ok(Self::from(m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// U14 acceptance test: `Matrix4` is byte-compatible with the wrapped
    /// glam type, so the `repr(transparent)` ptr-cast tricks used by
    /// `Index` / `IndexMut` / `AsRef` / `AsMut` are sound.
    #[test]
    fn matrix4_repr_transparent_size_of() {
        assert_eq!(
            std::mem::size_of::<Matrix4>(),
            std::mem::size_of::<glam::Mat4>(),
            "Matrix4 must be size-compatible with glam::Mat4 (repr(transparent))"
        );
        assert_eq!(std::mem::size_of::<Matrix4>(), 64);
        assert_eq!(
            std::mem::align_of::<Matrix4>(),
            std::mem::align_of::<glam::Mat4>()
        );
    }

    #[test]
    fn identity_is_identity() {
        let m = Matrix4::identity();
        assert!(m.is_identity());
        assert_eq!(m, Matrix4::IDENTITY);
    }

    #[test]
    fn translation_round_trip() {
        let t = Matrix4::translation(1.0, 2.0, 3.0);
        assert_eq!(t.translation_component(), (1.0, 2.0, 3.0));
    }

    #[test]
    fn scaling_diagonals() {
        let s = Matrix4::scaling(2.0, 3.0, 4.0);
        assert_eq!(s[0], 2.0);
        assert_eq!(s[5], 3.0);
        assert_eq!(s[10], 4.0);
        assert_eq!(s[15], 1.0);
    }

    #[test]
    fn rotation_z_inverse_is_transpose() {
        let r = Matrix4::rotation_z(0.5);
        let inv = r.try_inverse().expect("rotation is invertible");
        assert!(inv.approx_eq(&r.transpose()));
    }

    #[test]
    fn transform_point_translates() {
        let m = Matrix4::translation(10.0, 20.0, 0.0);
        let (x, y) = m.transform_point(Pixels(1.0), Pixels(2.0));
        assert_eq!(x, Pixels(11.0));
        assert_eq!(y, Pixels(22.0));
    }

    #[test]
    fn mul_by_identity_is_noop() {
        let r = Matrix4::rotation_z(0.7);
        let p = r * Matrix4::IDENTITY;
        assert!(p.approx_eq(&r));
        let q = Matrix4::IDENTITY * r;
        assert!(q.approx_eq(&r));
    }

    #[test]
    fn try_inverse_singular_returns_none() {
        assert!(Matrix4::ZERO.try_inverse().is_none());
    }

    #[test]
    fn index_matches_col_major_array() {
        let m = Matrix4::translation(7.0, 8.0, 9.0);
        let a = m.to_col_major_array();
        for i in 0..16 {
            assert_eq!(m[i], a[i], "index {i} mismatched");
        }
    }

    #[test]
    fn index_mut_writes_through() {
        let mut m = Matrix4::IDENTITY;
        m[12] = 5.0;
        assert_eq!(m.translation_component().0, 5.0);
    }

    #[test]
    fn from_into_array_round_trip() {
        let src: [f32; 16] = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
        ];
        let m = Matrix4::from(src);
        let out: [f32; 16] = m.into();
        assert_eq!(out, src);
    }

    #[test]
    fn transpose_involution() {
        let m = Matrix4::rotation_z(1.0);
        assert!(m.transpose().transpose().approx_eq(&m));
    }
}
