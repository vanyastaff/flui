//! Canvas transform operations: translate, scale, rotate, skew,
//! transform, set_transform, transform_matrix.
//!
//! These were extracted from the 3,305-LOC `canvas.rs`
//! god module. Each method mutates `self.transform` directly; the
//! current transform is baked into every `DrawCommand` at emission
//! time so the GPU backend can apply it without consulting external
//! state.

use flui_types::geometry::Matrix4;

use super::Canvas;

impl Canvas {
    /// Translates the coordinate system.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas.translate(50.0, 100.0);
    /// canvas.draw_rect(rect, &paint); // Drawn at (50, 100) offset
    /// ```
    #[inline]
    pub fn translate(&mut self, dx: f32, dy: f32) {
        debug_assert!(dx.is_finite(), "Canvas::translate dx must be finite");
        debug_assert!(dy.is_finite(), "Canvas::translate dy must be finite");
        let translation = Matrix4::translation(dx, dy, 0.0);
        self.transform *= translation;
    }

    /// Scales the coordinate system uniformly.
    #[inline]
    pub fn scale_uniform(&mut self, factor: f32) {
        debug_assert!(
            factor.is_finite(),
            "Canvas::scale_uniform factor must be finite"
        );
        let scaling = Matrix4::scaling(factor, factor, 1.0);
        self.transform *= scaling;
    }

    /// Scales the coordinate system with separate factors for each axis.
    #[inline]
    pub fn scale_xy(&mut self, sx: f32, sy: f32) {
        debug_assert!(sx.is_finite(), "Canvas::scale_xy sx must be finite");
        debug_assert!(sy.is_finite(), "Canvas::scale_xy sy must be finite");
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

    /// Rotates the coordinate system around a specified pivot point.
    ///
    /// Equivalent to translating to the pivot, rotating, then
    /// translating back.
    #[inline]
    pub fn rotate_around(&mut self, radians: f32, pivot_x: f32, pivot_y: f32) {
        debug_assert!(
            radians.is_finite(),
            "Canvas::rotate_around radians must be finite"
        );
        debug_assert!(
            pivot_x.is_finite(),
            "Canvas::rotate_around pivot_x must be finite"
        );
        debug_assert!(
            pivot_y.is_finite(),
            "Canvas::rotate_around pivot_y must be finite"
        );
        self.translate(pivot_x, pivot_y);
        self.rotate(radians);
        self.translate(-pivot_x, -pivot_y);
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
