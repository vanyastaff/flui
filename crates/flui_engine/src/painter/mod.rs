//! Backend-agnostic painting abstraction
//!
//! The Painter trait defines a backend-agnostic interface for rendering.
//! Different backends (egui, wgpu, skia) implement this trait to provide
//! actual rendering capabilities.

use flui_types::{Offset, Rect, Point};

// Backend implementations
#[cfg(feature = "egui")]
pub mod egui;

#[cfg(feature = "wgpu")]
pub mod wgpu;




/// Paint style information
#[derive(Debug, Clone)]
pub struct Paint {
    /// Fill color (RGBA)
    pub color: [f32; 4],

    /// Stroke width (0.0 = fill only)
    pub stroke_width: f32,

    /// Anti-aliasing enabled
    pub anti_alias: bool,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            color: [0.0, 0.0, 0.0, 1.0], // Black
            stroke_width: 0.0,
            anti_alias: true,
        }
    }
}

/// Rounded rectangle
#[derive(Debug, Clone, Copy)]
pub struct RRect {
    pub rect: Rect,
    pub corner_radius: f32,
}

/// Backend-agnostic painter trait
///
/// This trait abstracts over different rendering backends (egui, wgpu, skia, etc).
/// Implementations provide the actual drawing primitives.
///
/// # Design Philosophy
///
/// - **Backend Agnostic**: RenderObjects paint to this trait, not to concrete backends
/// - **Layered**: Paint operations build up a scene graph, not immediate rendering
/// - **Flexible**: Easy to add new backends by implementing this trait
///
/// # Example
///
/// ```rust,ignore
/// fn paint(&self, painter: &mut dyn Painter) {
///     let paint = Paint {
///         color: [1.0, 0.0, 0.0, 1.0], // Red
///         ..Default::default()
///     };
///     painter.rect(Rect::from_ltwh(0.0, 0.0, 100.0, 50.0), &paint);
/// }
/// ```
pub trait Painter {
    // ========== Drawing Primitives ==========

    /// Draw a filled or stroked rectangle
    fn rect(&mut self, rect: Rect, paint: &Paint);

    /// Draw a rounded rectangle
    fn rrect(&mut self, rrect: RRect, paint: &Paint);

    /// Draw a circle
    fn circle(&mut self, center: Point, radius: f32, paint: &Paint);

    /// Draw a line
    fn line(&mut self, p1: Point, p2: Point, paint: &Paint);

    /// Draw an ellipse
    ///
    /// # Parameters
    /// - `center`: Center point of the ellipse
    /// - `radius_x`: Horizontal radius
    /// - `radius_y`: Vertical radius
    /// - `paint`: Paint style
    fn ellipse(&mut self, center: Point, radius_x: f32, radius_y: f32, paint: &Paint) {
        // Default implementation approximates with circle
        let avg_radius = (radius_x + radius_y) * 0.5;
        self.circle(center, avg_radius, paint);
    }

    /// Draw an arc
    ///
    /// # Parameters
    /// - `center`: Center point of the arc
    /// - `radius`: Radius of the arc
    /// - `start_angle`: Start angle in radians
    /// - `end_angle`: End angle in radians
    /// - `paint`: Paint style
    fn arc(&mut self, center: Point, radius: f32, start_angle: f32, end_angle: f32, paint: &Paint) {
        // Default implementation draws lines
        let segments = 32;
        let angle_range = end_angle - start_angle;

        for i in 0..segments {
            let t1 = i as f32 / segments as f32;
            let t2 = (i + 1) as f32 / segments as f32;

            let angle1 = start_angle + angle_range * t1;
            let angle2 = start_angle + angle_range * t2;

            let p1 = Point::new(
                center.x + radius * angle1.cos(),
                center.y + radius * angle1.sin(),
            );
            let p2 = Point::new(
                center.x + radius * angle2.cos(),
                center.y + radius * angle2.sin(),
            );

            self.line(p1, p2, paint);
        }
    }

    /// Draw a polygon from a list of points
    ///
    /// # Parameters
    /// - `points`: List of points forming the polygon
    /// - `paint`: Paint style
    fn polygon(&mut self, points: &[Point], paint: &Paint) {
        if points.len() < 3 {
            return;
        }

        // Draw lines connecting all points
        for i in 0..points.len() {
            let p1 = points[i];
            let p2 = points[(i + 1) % points.len()];
            self.line(p1, p2, paint);
        }
    }

    /// Draw a polyline (open path) from a list of points
    ///
    /// # Parameters
    /// - `points`: List of points forming the polyline
    /// - `paint`: Paint style
    fn polyline(&mut self, points: &[Point], paint: &Paint) {
        if points.len() < 2 {
            return;
        }

        for i in 0..(points.len() - 1) {
            self.line(points[i], points[i + 1], paint);
        }
    }

    /// Draw text at a given position
    ///
    /// # Parameters
    /// - `text`: The text string to draw
    /// - `position`: Top-left position of the text
    /// - `font_size`: Font size in pixels
    /// - `paint`: Paint style (uses color from paint)
    fn text(&mut self, text: &str, position: Point, font_size: f32, paint: &Paint) {
        // Default implementation is no-op (for backends that don't support text yet)
        let _ = (text, position, font_size, paint);
    }

    // ========== Transform Stack ==========

    /// Save current transform state
    fn save(&mut self);

    /// Restore previous transform state
    fn restore(&mut self);

    /// Translate coordinate system
    fn translate(&mut self, offset: Offset);

    /// Rotate coordinate system (radians)
    fn rotate(&mut self, angle: f32);

    /// Scale coordinate system
    fn scale(&mut self, sx: f32, sy: f32);

    // ========== Clipping ==========

    /// Clip to rectangle (intersects with current clip)
    fn clip_rect(&mut self, rect: Rect);

    /// Clip to rounded rectangle
    fn clip_rrect(&mut self, rrect: RRect);

    // ========== Advanced ==========

    /// Set opacity for subsequent draws (0.0 = transparent, 1.0 = opaque)
    fn set_opacity(&mut self, opacity: f32);

    // ========== Convenience Methods (Default Implementations) ==========

    /// Draw a rectangle with a drop shadow
    ///
    /// # Parameters
    /// - `rect`: The rectangle to draw
    /// - `paint`: Paint style for the rectangle
    /// - `shadow_offset`: Offset of the shadow (dx, dy)
    /// - `shadow_blur`: Blur radius of the shadow
    /// - `shadow_color`: Color of the shadow (RGBA)
    fn rect_with_shadow(
        &mut self,
        rect: Rect,
        paint: &Paint,
        shadow_offset: Offset,
        shadow_blur: f32,
        shadow_color: [f32; 4],
    ) {
        // Draw shadow with multiple layers for blur effect
        let layers = 8;
        for i in 0..layers {
            let t = i as f32 / (layers - 1) as f32;
            let falloff = 1.0 - t;
            let opacity = falloff * falloff;

            let offset_scale = 1.0 + t * shadow_blur / 10.0;
            let shadow_rect = Rect::from_center_size(
                Point::new(
                    rect.center().x + shadow_offset.dx * offset_scale,
                    rect.center().y + shadow_offset.dy * offset_scale,
                ),
                rect.size(),
            );

            let shadow_paint = Paint {
                color: [
                    shadow_color[0],
                    shadow_color[1],
                    shadow_color[2],
                    shadow_color[3] * opacity,
                ],
                ..Default::default()
            };

            self.rect(shadow_rect, &shadow_paint);
        }

        // Draw main rectangle
        self.rect(rect, paint);
    }

    /// Draw a circle with a smooth radial glow effect
    ///
    /// # Parameters
    /// - `center`: Center point of the circle
    /// - `radius`: Radius of the circle
    /// - `paint`: Paint style for the circle
    /// - `glow_radius`: Additional radius for the glow effect
    /// - `glow_intensity`: Intensity of the glow (0.0 to 1.0)
    fn circle_with_glow(
        &mut self,
        center: Point,
        radius: f32,
        paint: &Paint,
        glow_radius: f32,
        glow_intensity: f32,
    ) {
        // Draw radial gradient from outside to inside
        let layers = 40;
        for i in (0..layers).rev() {
            let t = i as f32 / (layers - 1) as f32;
            let falloff = 1.0 - t;
            let eased = falloff * falloff * falloff; // Cubic easing

            let glow_color = [
                paint.color[0],
                paint.color[1],
                paint.color[2],
                paint.color[3] * eased * glow_intensity,
            ];

            let current_radius = radius + (1.0 - eased) * glow_radius;
            self.circle(
                center,
                current_radius,
                &Paint {
                    color: glow_color,
                    ..Default::default()
                },
            );
        }

        // Draw solid core
        self.circle(center, radius, paint);
    }

    /// Draw text with a drop shadow
    ///
    /// # Parameters
    /// - `text`: The text string to draw
    /// - `position`: Top-left position of the text
    /// - `font_size`: Font size in pixels
    /// - `paint`: Paint style for the text
    /// - `shadow_offset`: Offset of the shadow (dx, dy)
    /// - `shadow_color`: Color of the shadow (RGBA)
    fn text_with_shadow(
        &mut self,
        text: &str,
        position: Point,
        font_size: f32,
        paint: &Paint,
        shadow_offset: Offset,
        shadow_color: [f32; 4],
    ) {
        // Draw shadow
        let shadow_pos = Point::new(
            position.x + shadow_offset.dx,
            position.y + shadow_offset.dy,
        );
        self.text(
            text,
            shadow_pos,
            font_size,
            &Paint {
                color: shadow_color,
                ..*paint
            },
        );

        // Draw main text
        self.text(text, position, font_size, paint);
    }

    /// Draw a horizontal gradient
    ///
    /// # Parameters
    /// - `rect`: Rectangle area to fill with gradient
    /// - `start_color`: Color at the left edge (RGBA)
    /// - `end_color`: Color at the right edge (RGBA)
    fn horizontal_gradient(&mut self, rect: Rect, start_color: [f32; 4], end_color: [f32; 4]) {
        let steps = 50;
        let step_width = rect.width() / steps as f32;

        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;

            let color = [
                start_color[0] + t * (end_color[0] - start_color[0]),
                start_color[1] + t * (end_color[1] - start_color[1]),
                start_color[2] + t * (end_color[2] - start_color[2]),
                start_color[3] + t * (end_color[3] - start_color[3]),
            ];

            let x = rect.left() + i as f32 * step_width;
            let strip = Rect::from_xywh(x, rect.top(), step_width, rect.height());

            self.rect(strip, &Paint { color, ..Default::default() });
        }
    }

    /// Draw a vertical gradient
    ///
    /// # Parameters
    /// - `rect`: Rectangle area to fill with gradient
    /// - `start_color`: Color at the top edge (RGBA)
    /// - `end_color`: Color at the bottom edge (RGBA)
    fn vertical_gradient(&mut self, rect: Rect, start_color: [f32; 4], end_color: [f32; 4]) {
        let steps = 60;
        let step_height = rect.height() / steps as f32;

        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;

            let color = [
                start_color[0] + t * (end_color[0] - start_color[0]),
                start_color[1] + t * (end_color[1] - start_color[1]),
                start_color[2] + t * (end_color[2] - start_color[2]),
                start_color[3] + t * (end_color[3] - start_color[3]),
            ];

            let y = rect.top() + i as f32 * step_height;
            let strip = Rect::from_xywh(rect.left(), y, rect.width(), step_height);

            self.rect(strip, &Paint { color, ..Default::default() });
        }
    }

    /// Draw a radial gradient
    ///
    /// # Parameters
    /// - `center`: Center point of the gradient
    /// - `inner_radius`: Radius where start_color begins
    /// - `outer_radius`: Radius where end_color ends
    /// - `start_color`: Color at the center (RGBA)
    /// - `end_color`: Color at the outer edge (RGBA)
    fn radial_gradient(
        &mut self,
        center: Point,
        inner_radius: f32,
        outer_radius: f32,
        start_color: [f32; 4],
        end_color: [f32; 4],
    ) {
        let steps = 30;

        // Draw from outside to inside for proper layering
        for i in (0..steps).rev() {
            let t = i as f32 / (steps - 1) as f32;

            let color = [
                start_color[0] + t * (end_color[0] - start_color[0]),
                start_color[1] + t * (end_color[1] - start_color[1]),
                start_color[2] + t * (end_color[2] - start_color[2]),
                start_color[3] + t * (end_color[3] - start_color[3]),
            ];

            let radius = inner_radius + t * (outer_radius - inner_radius);

            self.circle(center, radius, &Paint { color, ..Default::default() });
        }
    }

    /// Draw a rounded rectangle with a drop shadow
    ///
    /// # Parameters
    /// - `rrect`: The rounded rectangle to draw
    /// - `paint`: Paint style for the rectangle
    /// - `shadow_offset`: Offset of the shadow (dx, dy)
    /// - `shadow_blur`: Blur radius of the shadow
    /// - `shadow_color`: Color of the shadow (RGBA)
    fn rrect_with_shadow(
        &mut self,
        rrect: RRect,
        paint: &Paint,
        shadow_offset: Offset,
        shadow_blur: f32,
        shadow_color: [f32; 4],
    ) {
        // Draw shadow with multiple layers for blur effect
        let layers = 8;
        for i in 0..layers {
            let t = i as f32 / (layers - 1) as f32;
            let falloff = 1.0 - t;
            let opacity = falloff * falloff;

            let offset_scale = 1.0 + t * shadow_blur / 10.0;
            let shadow_rrect = RRect {
                rect: Rect::from_center_size(
                    Point::new(
                        rrect.rect.center().x + shadow_offset.dx * offset_scale,
                        rrect.rect.center().y + shadow_offset.dy * offset_scale,
                    ),
                    rrect.rect.size(),
                ),
                corner_radius: rrect.corner_radius,
            };

            let shadow_paint = Paint {
                color: [
                    shadow_color[0],
                    shadow_color[1],
                    shadow_color[2],
                    shadow_color[3] * opacity,
                ],
                ..Default::default()
            };

            self.rrect(shadow_rrect, &shadow_paint);
        }

        // Draw main rounded rectangle
        self.rrect(rrect, paint);
    }
}




