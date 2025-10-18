//! 4x4 transformation matrix for 2D and 3D transformations.
//!
//! Matrix4 represents a 4x4 matrix stored in column-major order (like OpenGL/egui).
//! Used for affine transformations: translation, rotation, scaling, skewing, perspective.
//!
//! # Examples
//!
//! ```
//! use flui_types::Matrix4;
//!
//! // Identity matrix
//! let identity = Matrix4::identity();
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
//! ```

use std::ops::{Mul, MulAssign};

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
#[derive(Debug, Clone, Copy, PartialEq)]
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
    pub fn identity() -> Self {
        Self::new(
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
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

    /// Returns whether this is an identity matrix.
    #[inline]
    pub fn is_identity(&self) -> bool {
        *self == Self::identity()
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
    pub fn to_col_major_array(&self) -> [f32; 16] {
        self.m
    }
}

impl Default for Matrix4 {
    fn default() -> Self {
        Self::identity()
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
}
