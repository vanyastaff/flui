//! Transform types for 2D transformations
//!
//! This module contains types for 2D transformations like translation, rotation, and scaling.

use super::offset::Offset;
use super::scale::Scale;

/// 2D transformation matrix.
///
/// Simplified 2D transform using translation, rotation, and scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Translation (offset)
    pub translation: Offset,
    /// Rotation in radians
    pub rotation: f32,
    /// Scale factor
    pub scale: Scale,
}

impl Transform {
    /// Identity transform (no transformation).
    pub const IDENTITY: Transform = Transform {
        translation: Offset::ZERO,
        rotation: 0.0,
        scale: Scale::IDENTITY,
    };

    /// Create a new transform.
    pub fn new(translation: impl Into<Offset>, rotation: f32, scale: impl Into<Scale>) -> Self {
        Self {
            translation: translation.into(),
            rotation,
            scale: scale.into(),
        }
    }

    /// Create a translation-only transform.
    pub const fn translate(x: f32, y: f32) -> Self {
        Self {
            translation: Offset::new(x, y),
            rotation: 0.0,
            scale: Scale::IDENTITY,
        }
    }

    /// Create a translation transform from anything convertible to Offset.
    pub fn from_offset(offset: impl Into<Offset>) -> Self {
        Self {
            translation: offset.into(),
            rotation: 0.0,
            scale: Scale::IDENTITY,
        }
    }

    /// Create a rotation-only transform (in radians).
    pub const fn rotate(radians: f32) -> Self {
        Self {
            translation: Offset::ZERO,
            rotation: radians,
            scale: Scale::IDENTITY,
        }
    }

    /// Create a rotation transform from degrees.
    pub fn rotate_degrees(degrees: f32) -> Self {
        Self::rotate(degrees.to_radians())
    }

    /// Create a uniform scale transform.
    pub const fn scale_uniform(factor: f32) -> Self {
        Self {
            translation: Offset::ZERO,
            rotation: 0.0,
            scale: Scale::uniform(factor),
        }
    }

    /// Create a non-uniform scale transform.
    pub const fn scale(x: f32, y: f32) -> Self {
        Self {
            translation: Offset::ZERO,
            rotation: 0.0,
            scale: Scale::new(x, y),
        }
    }

    /// Create a scale transform from anything convertible to Scale.
    pub fn from_scale(scale: impl Into<Scale>) -> Self {
        Self {
            translation: Offset::ZERO,
            rotation: 0.0,
            scale: scale.into(),
        }
    }

    /// Apply translation.
    pub fn then_translate(mut self, x: f32, y: f32) -> Self {
        self.translation = self.translation + Offset::new(x, y);
        self
    }

    /// Apply translation from anything convertible to Offset.
    pub fn then_translate_offset(mut self, offset: impl Into<Offset>) -> Self {
        self.translation = self.translation + offset.into();
        self
    }

    /// Apply rotation (in radians).
    pub fn then_rotate(mut self, radians: f32) -> Self {
        self.rotation += radians;
        self
    }

    /// Apply rotation (in degrees).
    pub fn then_rotate_degrees(self, degrees: f32) -> Self {
        self.then_rotate(degrees.to_radians())
    }

    /// Apply scale.
    pub fn then_scale(mut self, x: f32, y: f32) -> Self {
        self.scale = self.scale.then(Scale::new(x, y));
        self
    }

    /// Apply scale from anything convertible to Scale.
    pub fn then_scale_factor(mut self, scale: impl Into<Scale>) -> Self {
        self.scale = self.scale.then(scale.into());
        self
    }

    /// Apply uniform scale.
    pub fn then_scale_uniform(mut self, factor: f32) -> Self {
        self.scale = self.scale.scale_uniform(factor);
        self
    }

    /// Transform an offset.
    pub fn transform_offset(&self, offset: Offset) -> Offset {
        // Apply scale
        let scaled = Offset::new(
            offset.dx * self.scale.x,
            offset.dy * self.scale.y,
        );

        // Apply rotation
        let cos = self.rotation.cos();
        let sin = self.rotation.sin();
        let rotated = Offset::new(
            scaled.dx * cos - scaled.dy * sin,
            scaled.dx * sin + scaled.dy * cos,
        );

        // Apply translation
        rotated + self.translation
    }

    /// Transform an offset without translation (useful for vectors/directions).
    pub fn transform_offset_no_translation(&self, offset: Offset) -> Offset {
        // Apply scale
        let scaled = Offset::new(
            offset.dx * self.scale.x,
            offset.dy * self.scale.y,
        );

        // Apply rotation
        let cos = self.rotation.cos();
        let sin = self.rotation.sin();
        Offset::new(
            scaled.dx * cos - scaled.dy * sin,
            scaled.dx * sin + scaled.dy * cos,
        )
    }

    /// Get the inverse transform.
    pub fn inverse(&self) -> Self {
        let inv_scale = self.scale.inverse();
        let inv_rotation = -self.rotation;

        // Inverse translation needs to account for rotation and scale
        let cos = inv_rotation.cos();
        let sin = inv_rotation.sin();
        let scaled_trans = Offset::new(
            -self.translation.dx * inv_scale.x,
            -self.translation.dy * inv_scale.y,
        );
        let inv_translation = Offset::new(
            scaled_trans.dx * cos - scaled_trans.dy * sin,
            scaled_trans.dx * sin + scaled_trans.dy * cos,
        );

        Self {
            translation: inv_translation,
            rotation: inv_rotation,
            scale: inv_scale,
        }
    }

    /// Check if this is the identity transform.
    pub fn is_identity(&self) -> bool {
        self.translation == Offset::ZERO
            && self.rotation == 0.0
            && self.scale.is_identity()
    }

    /// Get rotation in degrees.
    pub fn rotation_degrees(&self) -> f32 {
        self.rotation.to_degrees()
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

// Idiomatic conversions
impl From<Offset> for Transform {
    fn from(offset: Offset) -> Self {
        Self::from_offset(offset)
    }
}

impl From<Scale> for Transform {
    fn from(scale: Scale) -> Self {
        Self::from_scale(scale)
    }
}

impl From<f32> for Transform {
    /// Create a uniform scale transform from a scale factor.
    fn from(scale: f32) -> Self {
        Self::scale_uniform(scale)
    }
}

impl From<(f32, f32)> for Transform {
    /// Create a translation transform from (x, y).
    fn from((x, y): (f32, f32)) -> Self {
        Self::translate(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_identity() {
        let transform = Transform::IDENTITY;
        assert!(transform.is_identity());

        let offset = Offset::new(10.0, 20.0);
        let transformed = transform.transform_offset(offset);
        assert_eq!(transformed, offset);
    }

    #[test]
    fn test_transform_translate() {
        let transform = Transform::translate(10.0, 20.0);
        let offset = Offset::new(5.0, 5.0);
        let transformed = transform.transform_offset(offset);
        assert_eq!(transformed, Offset::new(15.0, 25.0));
    }

    #[test]
    fn test_transform_rotate() {
        let transform = Transform::rotate_degrees(90.0);
        let offset = Offset::new(1.0, 0.0);
        let transformed = transform.transform_offset(offset);

        // After 90° rotation, (1, 0) should be approximately (0, 1)
        assert!((transformed.dx - 0.0).abs() < 0.001);
        assert!((transformed.dy - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_scale() {
        let transform = Transform::scale_uniform(2.0);
        let offset = Offset::new(3.0, 4.0);
        let transformed = transform.transform_offset(offset);
        assert_eq!(transformed, Offset::new(6.0, 8.0));
    }

    #[test]
    fn test_transform_combined() {
        let transform = Transform::translate(10.0, 10.0)
            .then_scale_uniform(2.0)
            .then_rotate_degrees(0.0);

        let offset = Offset::new(5.0, 5.0);
        let transformed = transform.transform_offset(offset);

        // Scale (5,5) * 2 = (10,10), then translate +10,+10 = (20,20)
        assert_eq!(transformed, Offset::new(20.0, 20.0));
    }

    #[test]
    fn test_transform_offset_no_translation() {
        let transform = Transform::translate(100.0, 100.0).then_scale_uniform(2.0);
        let offset = Offset::new(3.0, 4.0);
        let transformed = transform.transform_offset_no_translation(offset);

        // Translation should be ignored, only scale applied
        assert_eq!(transformed, Offset::new(6.0, 8.0));
    }

    #[test]
    fn test_transform_rotation_degrees() {
        let transform = Transform::rotate_degrees(45.0);
        assert!((transform.rotation_degrees() - 45.0).abs() < 0.001);
    }

    #[test]
    fn test_non_uniform_scale() {
        let transform = Transform::scale(2.0, 3.0);
        let offset = Offset::new(4.0, 5.0);
        let transformed = transform.transform_offset(offset);
        assert_eq!(transformed, Offset::new(8.0, 15.0));
    }

    #[test]
    fn test_from_offset() {
        let offset = Offset::new(10.0, 20.0);
        let transform = Transform::from_offset(offset);
        assert_eq!(transform.translation, offset);
        assert_eq!(transform.rotation, 0.0);
        assert!(transform.scale.is_identity());
    }

    #[test]
    fn test_from_scale() {
        let scale = Scale::new(2.0, 3.0);
        let transform = Transform::from_scale(scale);
        assert_eq!(transform.translation, Offset::ZERO);
        assert_eq!(transform.rotation, 0.0);
        assert_eq!(transform.scale, scale);
    }

    #[test]
    fn test_from_conversions() {
        // From Offset
        let offset = Offset::new(10.0, 20.0);
        let transform: Transform = offset.into();
        assert_eq!(transform.translation, offset);

        // From Scale
        let scale = Scale::new(2.0, 3.0);
        let transform: Transform = scale.into();
        assert_eq!(transform.scale, scale);

        // From f32 (uniform scale)
        let transform: Transform = 2.5.into();
        assert_eq!(transform.scale, Scale::uniform(2.5));

        // From (f32, f32) (translation)
        let transform: Transform = (10.0, 20.0).into();
        assert_eq!(transform.translation, Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_into_parameters() {
        use egui::Vec2;

        // new() accepts Into<Offset> and Into<Scale>
        let t1 = Transform::new(
            Offset::new(10.0, 20.0),
            0.0,
            Scale::new(2.0, 3.0)
        );

        // Can use Vec2 directly (zero-cost)
        let t2 = Transform::new(
            Vec2::new(10.0, 20.0),  // Vec2 -> Offset
            0.0,
            Vec2::new(2.0, 3.0)     // Vec2 -> Scale
        );
        assert_eq!(t1.translation, t2.translation);
        assert_eq!(t1.scale, t2.scale);

        // from_offset accepts Into<Offset>
        let t3 = Transform::from_offset(Vec2::new(5.0, 5.0));
        assert_eq!(t3.translation, Offset::new(5.0, 5.0));

        // from_scale accepts Into<Scale>
        let t4 = Transform::from_scale(Vec2::new(2.0, 2.0));
        assert_eq!(t4.scale, Scale::new(2.0, 2.0));

        // then_translate_offset accepts Into<Offset>
        let t5 = Transform::IDENTITY.then_translate_offset(Vec2::new(10.0, 10.0));
        assert_eq!(t5.translation, Offset::new(10.0, 10.0));

        // then_scale_factor accepts Into<Scale>
        let t6 = Transform::IDENTITY.then_scale_factor(Vec2::new(2.0, 3.0));
        assert_eq!(t6.scale, Scale::new(2.0, 3.0));
    }
}
