//! Canvas transforms. Each method mutates the current matrix, which every
//! `DrawCommand` records at emission so the engine needs no external state.

use flui_types::geometry::Matrix4;

use super::Canvas;

impl Canvas {
    /// Translates the coordinate system.
    #[inline]
    pub fn translate(&mut self, dx: f32, dy: f32) {
        debug_assert!(dx.is_finite(), "Canvas::translate dx must be finite");
        debug_assert!(dy.is_finite(), "Canvas::translate dy must be finite");
        let translation = Matrix4::translation(dx, dy, 0.0);
        self.transform *= translation;
    }

    /// Scales the coordinate system with separate factors for each axis.
    #[inline]
    pub fn scale(&mut self, sx: f32, sy: f32) {
        debug_assert!(sx.is_finite(), "Canvas::scale sx must be finite");
        debug_assert!(sy.is_finite(), "Canvas::scale sy must be finite");
        let scaling = Matrix4::scaling(sx, sy, 1.0);
        self.transform *= scaling;
    }

    /// Rotates the coordinate system around the origin.
    #[inline]
    pub fn rotate(&mut self, radians: f32) {
        debug_assert!(radians.is_finite(), "Canvas::rotate radians must be finite");
        let rotation = Matrix4::rotation_z(radians);
        self.transform *= rotation;
    }

    /// Skews the coordinate system along the X and Y axes.
    ///
    /// Useful for italic text effects, parallax, and perspective-like
    /// distortions.
    #[inline]
    pub fn skew(&mut self, sx: f32, sy: f32) {
        debug_assert!(sx.is_finite(), "Canvas::skew sx must be finite");
        debug_assert!(sy.is_finite(), "Canvas::skew sy must be finite");
        let skew_matrix = Matrix4::new(
            1.0, sx, 0.0, 0.0, sy, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        );
        self.transform *= skew_matrix;
    }

    /// Applies a transform to the current coordinate system.
    ///
    /// Accepts both `Transform` and `Matrix4` types via the `Into`
    /// trait, allowing for idiomatic Rust usage with the high-level
    /// `Transform` API.
    pub fn transform<T: Into<Matrix4>>(&mut self, transform: T) {
        let matrix = transform.into();
        self.transform *= matrix;
    }

    /// Sets the transform matrix directly.
    pub fn set_transform<T: Into<Matrix4>>(&mut self, transform: T) {
        self.transform = transform.into();
    }

    /// Returns the current transform matrix.
    #[inline]
    #[must_use]
    pub fn transform_matrix(&self) -> Matrix4 {
        self.transform
    }
}
