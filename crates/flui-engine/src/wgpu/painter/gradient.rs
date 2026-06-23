// =============================================================================
// Advanced Effects API (Gradients, Shadows, Blur)
// =============================================================================
//
// Moved from `painter.rs` into `painter/gradient.rs` as part of the
// C1 LOC-cap refactor.  Zero behaviour changes.

use flui_types::{Rect, geometry::Pixels};

use super::super::batches::DrawBatcher;
use super::super::effects::{GradientStop, ShadowParams};
use super::WgpuPainter;

#[allow(clippy::cast_possible_truncation)]
impl WgpuPainter {
    /// Draw a rectangle with a linear gradient.
    ///
    /// # Arguments
    /// * `bounds`          - Rectangle bounds
    /// * `gradient_start`  - Gradient start point (local coordinates)
    /// * `gradient_end`    - Gradient end point (local coordinates)
    /// * `stops`           - Gradient color stops (max 8)
    /// * `corner_radius`   - Corner radius (uniform, 0.0 = sharp corners)
    ///
    /// # Example
    /// ```ignore
    /// // Vertical gradient from red to blue
    /// painter.gradient_rect(
    ///     Rect::from_ltrb(10.0, 10.0, 210.0, 110.0),
    ///     glam::Vec2::new(0.0, 0.0),   // Top
    ///     glam::Vec2::new(0.0, 100.0), // Bottom
    ///     &[
    ///         GradientStop::start(Color::RED),
    ///         GradientStop::end(Color::BLUE),
    ///     ],
    ///     12.0, // Rounded corners
    /// );
    /// ```
    pub fn gradient_rect(
        &mut self,
        bounds: Rect<Pixels>,
        gradient_start: glam::Vec2,
        gradient_end: glam::Vec2,
        stops: &[GradientStop],
        corner_radius: f32,
    ) {
        DrawBatcher::gradient_rect(
            &mut self.current_segment,
            &self.state,
            bounds,
            gradient_start,
            gradient_end,
            stops,
            corner_radius,
        );
    }

    /// Draw a rectangle with a radial gradient.
    ///
    /// # Arguments
    /// * `bounds`         - Rectangle bounds
    /// * `center`         - Gradient center point (local coordinates)
    /// * `radius`         - Gradient radius
    /// * `stops`          - Gradient color stops (max 8)
    /// * `corner_radius`  - Corner radius (uniform, 0.0 = sharp corners)
    ///
    /// # Example
    /// ```ignore
    /// // Radial gradient from white center to transparent edge
    /// painter.radial_gradient_rect(
    ///     Rect::from_ltrb(10.0, 10.0, 110.0, 110.0),
    ///     glam::Vec2::new(50.0, 50.0), // Center
    ///     50.0,                         // Radius
    ///     &[
    ///         GradientStop::start(Color::WHITE),
    ///         GradientStop::end(Color::TRANSPARENT),
    ///     ],
    ///     0.0, // Sharp corners
    /// );
    /// ```
    pub fn radial_gradient_rect(
        &mut self,
        bounds: Rect<Pixels>,
        center: glam::Vec2,
        radius: f32,
        stops: &[GradientStop],
        corner_radius: f32,
    ) {
        DrawBatcher::radial_gradient_rect(
            &mut self.current_segment,
            &self.state,
            bounds,
            center,
            radius,
            stops,
            corner_radius,
        );
    }

    /// Draw a rectangle with a sweep (angular/conic) gradient.
    ///
    /// # Arguments
    /// * `bounds`        - Rectangle bounds
    /// * `center`        - Gradient center point (local coordinates)
    /// * `start_angle`   - Start angle in radians
    /// * `end_angle`     - End angle in radians
    /// * `stops`         - Gradient color stops (max 8)
    /// * `corner_radius` - Corner radius (uniform, 0.0 = sharp corners)
    pub fn sweep_gradient_rect(
        &mut self,
        bounds: Rect<Pixels>,
        center: glam::Vec2,
        start_angle: f32,
        end_angle: f32,
        stops: &[GradientStop],
        corner_radius: f32,
    ) {
        DrawBatcher::sweep_gradient_rect(
            &mut self.current_segment,
            &self.state,
            bounds,
            center,
            start_angle,
            end_angle,
            stops,
            corner_radius,
        );
    }

    /// Draw a shadow for a rectangle.
    ///
    /// Renders an analytical shadow using Evan Wallace's technique.
    /// Single-pass O(1) rendering with quality indistinguishable from real
    /// Gaussian.
    ///
    /// # Arguments
    /// * `rect_pos`       - Rectangle position [x, y]
    /// * `rect_size`      - Rectangle size [width, height]
    /// * `corner_radius`  - Corner radius (uniform)
    /// * `params`         - Shadow parameters (offset, blur, color)
    ///
    /// # Example
    /// ```ignore
    /// use flui_engine::painter::effects::ShadowParams;
    /// use flui_types::styling::Color;
    /// use glam::Vec2;
    ///
    /// // Material Design elevation 2 shadow (offset.y=2, sigma=4, ~0.16 alpha)
    /// painter.shadow_rect(
    ///     [10.0, 10.0],
    ///     [200.0, 100.0],
    ///     12.0,
    ///     &ShadowParams::new(Vec2::new(0.0, 2.0), 4.0, Color::rgba(0, 0, 0, 41)),
    /// );
    /// ```
    pub fn shadow_rect(
        &mut self,
        rect_pos: [f32; 2],
        rect_size: [f32; 2],
        corner_radius: f32,
        params: &ShadowParams,
    ) {
        DrawBatcher::shadow_rect(
            &mut self.current_segment,
            rect_pos,
            rect_size,
            corner_radius,
            params,
        );
    }
}
