//! Backend-agnostic painting abstraction
//!
//! The Painter trait defines a backend-agnostic interface for rendering.
//! Different backends (egui, wgpu, skia) implement this trait to provide
//! actual rendering capabilities.
//!
//! **Note**: Backend implementations have been moved to the `crate::backends` module.
//! Use `crate::backends::egui::EguiPainter` or `crate::backends::wgpu::WgpuPainter`.

use flui_types::{
    Offset, Point, Rect,
    painting::{BlendMode, PaintingStyle, StrokeCap, StrokeJoin},
    styling::Color,
};

/// Paint style information
///
/// Represents all styling information needed to draw shapes, paths, and text.
/// Uses strong types from `flui_types` for type safety and consistency.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_engine::Paint;
/// use flui_types::Color;
///
/// // Fill paint
/// let fill = Paint::fill(Color::RED);
///
/// // Stroke paint
/// let stroke = Paint::stroke(2.0, Color::BLUE);
///
/// // Builder pattern
/// let paint = Paint::new()
///     .with_color(Color::GREEN)
///     .with_stroke_width(3.0)
///     .with_blend_mode(BlendMode::Multiply);
/// ```
#[derive(Debug, Clone)]
pub struct Paint {
    /// Fill/stroke color
    pub color: Color,

    /// Painting style (fill or stroke)
    pub style: PaintingStyle,

    /// Stroke width (only applies when style is Stroke)
    pub stroke_width: f32,

    /// Stroke cap style (only applies when style is Stroke)
    pub stroke_cap: StrokeCap,

    /// Stroke join style (only applies when style is Stroke)
    pub stroke_join: StrokeJoin,

    /// Stroke miter limit (only applies when stroke_join is Miter)
    pub stroke_miter_limit: f32,

    /// Anti-aliasing enabled
    pub anti_alias: bool,

    /// Blend mode for compositing
    pub blend_mode: BlendMode,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            style: PaintingStyle::Fill,
            stroke_width: 0.0,
            stroke_cap: StrokeCap::Butt,
            stroke_join: StrokeJoin::Miter,
            stroke_miter_limit: 4.0,
            anti_alias: true,
            blend_mode: BlendMode::SrcOver,
        }
    }
}

impl Paint {
    /// Create a new paint with default settings (black fill, anti-aliased)
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a fill paint with specified color
    ///
    /// # Example
    /// ```rust,ignore
    /// let paint = Paint::fill(Color::RED);
    /// ```
    pub fn fill(color: Color) -> Self {
        Self {
            color,
            style: PaintingStyle::Fill,
            ..Default::default()
        }
    }

    /// Create a stroke paint with specified width and color
    ///
    /// # Example
    /// ```rust,ignore
    /// let paint = Paint::stroke(2.0, Color::BLUE);
    /// ```
    pub fn stroke(width: f32, color: Color) -> Self {
        Self {
            color,
            style: PaintingStyle::Stroke,
            stroke_width: width,
            ..Default::default()
        }
    }

    /// Set color (builder pattern)
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Set painting style (builder pattern)
    pub fn with_style(mut self, style: PaintingStyle) -> Self {
        self.style = style;
        self
    }

    /// Set stroke width (builder pattern)
    pub fn with_stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = width;
        self
    }

    /// Set stroke cap (builder pattern)
    pub fn with_stroke_cap(mut self, cap: StrokeCap) -> Self {
        self.stroke_cap = cap;
        self
    }

    /// Set stroke join (builder pattern)
    pub fn with_stroke_join(mut self, join: StrokeJoin) -> Self {
        self.stroke_join = join;
        self
    }

    /// Set stroke miter limit (builder pattern)
    pub fn with_stroke_miter_limit(mut self, limit: f32) -> Self {
        self.stroke_miter_limit = limit;
        self
    }

    /// Enable or disable anti-aliasing (builder pattern)
    pub fn with_anti_alias(mut self, enabled: bool) -> Self {
        self.anti_alias = enabled;
        self
    }

    /// Set blend mode (builder pattern)
    pub fn with_blend_mode(mut self, mode: BlendMode) -> Self {
        self.blend_mode = mode;
        self
    }

    /// Check if this is a fill paint
    #[inline]
    pub fn is_fill(&self) -> bool {
        matches!(self.style, PaintingStyle::Fill)
    }

    /// Check if this is a stroke paint
    #[inline]
    pub fn is_stroke(&self) -> bool {
        matches!(self.style, PaintingStyle::Stroke)
    }

    /// Get color as RGBA f32 array for backend consumption
    ///
    /// This is a convenience method for backends that need array format.
    /// Prefer using the `color` field directly when possible.
    #[inline]
    pub fn color_array(&self) -> [f32; 4] {
        self.color.to_rgba_f32_array()
    }
}

// Re-export RRect from flui_types for convenience
// This avoids duplication and provides full per-corner radius support
pub use flui_types::geometry::RRect;

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
    ///
    /// # Per-Character Custom Effects
    ///
    /// For custom per-character transformations (wave, circle, pyramid text, etc.),
    /// render each character separately using helpers from `flui_types::text_path`:
    ///
    /// ```rust,ignore
    /// use flui_types::text_path::*;
    ///
    /// // Wave text
    /// for (i, ch) in text.chars().enumerate() {
    ///     let wave_y = wave_offset(i, 0.5, 10.0);
    ///     painter.text(&ch.to_string(), Point::new(x + i * 20.0, y + wave_y), 16.0, &paint);
    /// }
    ///
    /// // Circle text
    /// for (i, ch) in text.chars().enumerate() {
    ///     let transform = arc_position(i, text.len(), 100.0, 0.0, TAU);
    ///     painter.save();
    ///     painter.translate(Offset::new(transform.position.x, transform.position.y));
    ///     painter.rotate(transform.rotation);
    ///     painter.text(&ch.to_string(), Point::ZERO, 16.0, &paint);
    ///     painter.restore();
    /// }
    ///
    /// // Pyramid/trapezoid text (vertical gradient scaling)
    /// let lines: Vec<&str> = text.lines().collect();
    /// for (i, line) in lines.iter().enumerate() {
    ///     let y_norm = i as f32 / lines.len() as f32;
    ///     let scale_x = vertical_scale(y_norm, 0.5, 1.0); // narrow top, wide bottom
    ///     painter.save();
    ///     painter.scale(scale_x, 1.0);
    ///     painter.text(line, Point::new(x, y + i * line_height), 16.0, &paint);
    ///     painter.restore();
    /// }
    /// ```
    ///
    /// See `flui_types::text_path` module for more helper functions:
    /// - `arc_position()` - circular/arc text
    /// - `wave_offset()`, `wave_rotation()` - wave effects
    /// - `spiral_position()` - spiral text
    /// - `vertical_scale()` - pyramid/trapezoid scaling
    /// - `bezier_point()` - text along Bezier curves
    /// - `parametric_position()` - custom parametric paths
    fn text(&mut self, text: &str, position: Point, font_size: f32, paint: &Paint) {
        // Default implementation is no-op (for backends that don't support text yet)
        let _ = (text, position, font_size, paint);
    }

    /// Draw text with full style information
    ///
    /// # Parameters
    /// - `text`: The text string to draw
    /// - `position`: Top-left position of the text
    /// - `style`: Text style (font, size, color, etc.)
    fn text_styled(
        &mut self,
        text: &str,
        position: Point,
        style: &flui_types::typography::TextStyle,
    ) {
        // Default: extract font size and delegate to simple text()
        let font_size = style.font_size.unwrap_or(14.0) as f32;
        let paint = Paint::fill(style.color.unwrap_or(Color::BLACK));
        self.text(text, position, font_size, &paint);
    }

    /// Draw an image
    ///
    /// # Parameters
    /// - `image`: The image to draw
    /// - `src_rect`: Source rectangle in image coordinates
    /// - `dst_rect`: Destination rectangle on canvas
    /// - `paint`: Paint settings (opacity, blend mode, etc.)
    fn image(
        &mut self,
        _image: &flui_types::painting::Image,
        _src_rect: Rect,
        _dst_rect: Rect,
        _paint: &Paint,
    ) {
        // Default implementation is no-op (for backends that don't support images yet)
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

    /// Skew (shear) coordinate system
    ///
    /// # Parameters
    /// - `skew_x`: Horizontal skew angle in radians
    /// - `skew_y`: Vertical skew angle in radians
    ///
    /// # Default Implementation
    /// Uses a matrix transform equivalent to skew.
    /// Backends can override for more efficient implementation.
    fn skew(&mut self, skew_x: f32, skew_y: f32) {
        // Skew matrix:
        // | 1      tan(skew_x)  0 |
        // | tan(skew_y)  1      0 |
        // | 0      0      1 |
        let tan_x = skew_x.tan();
        let tan_y = skew_y.tan();
        self.transform_matrix(1.0, tan_y, tan_x, 1.0, 0.0, 0.0);
    }

    /// Apply arbitrary 2D affine transformation matrix
    ///
    /// # Parameters
    /// Matrix [a, b, c, d, tx, ty] represents:
    /// | a  c  tx |
    /// | b  d  ty |
    /// | 0  0  1  |
    ///
    /// # Default Implementation
    /// Composes the transform using translate, rotate, scale operations.
    /// Backends should override for direct matrix support.
    fn transform_matrix(&mut self, a: f32, b: f32, c: f32, d: f32, tx: f32, ty: f32) {
        // For backends without native matrix support, decompose the matrix
        // This is approximate and may not handle all edge cases perfectly

        // Apply translation
        self.translate(Offset::new(tx, ty));

        // Decompose matrix to rotation + scale
        // [a c]   [cos -sin] [sx  0 ]
        // [b d] = [sin  cos] [0   sy]

        let sx = (a * a + b * b).sqrt();
        let sy = (c * c + d * d).sqrt();

        if sx.abs() > 0.001 && sy.abs() > 0.001 {
            let cos_theta = a / sx;
            let sin_theta = b / sx;
            let angle = sin_theta.atan2(cos_theta);

            self.rotate(angle);
            self.scale(sx, sy);
        }
    }

    /// Apply full 4x4 transformation matrix (supports 3D perspective)
    ///
    /// # Parameters
    /// - `matrix`: 4x4 transformation matrix (glam::Mat4)
    ///
    /// This method applies a full 4x4 matrix transformation, including
    /// perspective projection. For standard 2D transforms, the matrix
    /// should have the form:
    /// ```text
    /// | sx  shx 0  px |
    /// | shy sy  0  py |
    /// | 0   0   1  0  |
    /// | tx  ty  0  1  |
    /// ```
    ///
    /// For 3D perspective, non-zero values in row 4 (tx, ty columns) create
    /// perspective division effects.
    ///
    /// # Default Implementation
    /// Backends without native Mat4 support should override this method.
    /// The default implementation extracts 2D affine part and ignores perspective.
    fn apply_matrix4(&mut self, matrix: glam::Mat4) {
        let m = matrix.to_cols_array_2d();
        // Extract 2D affine transform (ignore perspective)
        self.transform_matrix(m[0][0], m[0][1], m[1][0], m[1][1], m[3][0], m[3][1]);
    }

    // ========== Clipping ==========

    /// Clip to rectangle (intersects with current clip)
    fn clip_rect(&mut self, rect: Rect);

    /// Clip to rounded rectangle
    fn clip_rrect(&mut self, rrect: RRect);

    /// Clip to oval/ellipse
    ///
    /// # Parameters
    /// - `rect`: The bounding rectangle of the oval
    ///
    /// # Default Implementation
    /// Falls back to clip_rect (conservative approximation).
    /// Backends should override for proper oval clipping.
    fn clip_oval(&mut self, rect: Rect) {
        // Default: clip to bounding rect (conservative)
        self.clip_rect(rect);
    }

    /// Clip to an arbitrary path
    ///
    /// # Parameters
    /// - `path`: The path defining the clip region (passed by reference)
    /// - `bounds`: Pre-computed bounds of the path (avoids mutation)
    ///
    /// # Default Implementation
    /// Falls back to clip_rect of the path's bounding box.
    /// Backends should override for proper path clipping.
    fn clip_path(&mut self, _path: &flui_types::painting::path::Path, bounds: flui_types::Rect) {
        // Default: clip to bounding box (conservative)
        self.clip_rect(bounds);
    }

    // ========== Path Drawing ==========

    /// Draw a path with the given paint style.
    ///
    /// This is an optional optimization. Backends can provide efficient path rendering,
    /// or fall back to the default implementation which decomposes paths into primitives.
    ///
    /// # Parameters
    /// - `path`: The path to draw
    /// - `paint`: Paint style (color, stroke width, etc.)
    ///
    /// # Default Implementation
    /// The default implementation decomposes the path into individual drawing commands
    /// using the other primitives (line, circle, rect, etc.). This works but may be less
    /// efficient than native path rendering.
    fn path(&mut self, path: &flui_types::painting::path::Path, paint: &Paint) {
        use flui_types::painting::path::PathCommand;

        let commands = path.commands();
        let mut current_pos = Point::new(0.0, 0.0);
        let mut subpath_start = Point::new(0.0, 0.0);

        for cmd in commands {
            match cmd {
                PathCommand::MoveTo(p) => {
                    current_pos = *p;
                    subpath_start = *p;
                }
                PathCommand::LineTo(p) => {
                    self.line(current_pos, *p, paint);
                    current_pos = *p;
                }
                PathCommand::QuadraticTo(control, end) => {
                    // Convert quadratic to cubic Bézier
                    let c1 = Point::new(
                        current_pos.x + 2.0 / 3.0 * (control.x - current_pos.x),
                        current_pos.y + 2.0 / 3.0 * (control.y - current_pos.y),
                    );
                    let c2 = Point::new(
                        end.x + 2.0 / 3.0 * (control.x - end.x),
                        end.y + 2.0 / 3.0 * (control.y - end.y),
                    );

                    // Draw cubic Bézier with line approximation
                    const SEGMENTS: usize = 20;
                    let mut prev = current_pos;
                    for i in 1..=SEGMENTS {
                        let t = i as f32 / SEGMENTS as f32;
                        let t2 = t * t;
                        let t3 = t2 * t;
                        let mt = 1.0 - t;
                        let mt2 = mt * mt;
                        let mt3 = mt2 * mt;

                        let x = mt3 * current_pos.x
                            + 3.0 * mt2 * t * c1.x
                            + 3.0 * mt * t2 * c2.x
                            + t3 * end.x;
                        let y = mt3 * current_pos.y
                            + 3.0 * mt2 * t * c1.y
                            + 3.0 * mt * t2 * c2.y
                            + t3 * end.y;

                        let point = Point::new(x, y);
                        self.line(prev, point, paint);
                        prev = point;
                    }
                    current_pos = *end;
                }
                PathCommand::CubicTo(c1, c2, end) => {
                    // Draw cubic Bézier with line approximation
                    const SEGMENTS: usize = 20;
                    let mut prev = current_pos;
                    for i in 1..=SEGMENTS {
                        let t = i as f32 / SEGMENTS as f32;
                        let t2 = t * t;
                        let t3 = t2 * t;
                        let mt = 1.0 - t;
                        let mt2 = mt * mt;
                        let mt3 = mt2 * mt;

                        let x = mt3 * current_pos.x
                            + 3.0 * mt2 * t * c1.x
                            + 3.0 * mt * t2 * c2.x
                            + t3 * end.x;
                        let y = mt3 * current_pos.y
                            + 3.0 * mt2 * t * c1.y
                            + 3.0 * mt * t2 * c2.y
                            + t3 * end.y;

                        let point = Point::new(x, y);
                        self.line(prev, point, paint);
                        prev = point;
                    }
                    current_pos = *end;
                }
                PathCommand::Close => {
                    if current_pos != subpath_start {
                        self.line(current_pos, subpath_start, paint);
                    }
                    current_pos = subpath_start;
                }
                PathCommand::AddRect(rect) => {
                    if paint.stroke_width > 0.0 {
                        // Stroke
                        let corners = [
                            Point::new(rect.left(), rect.top()),
                            Point::new(rect.right(), rect.top()),
                            Point::new(rect.right(), rect.bottom()),
                            Point::new(rect.left(), rect.bottom()),
                        ];
                        for i in 0..4 {
                            self.line(corners[i], corners[(i + 1) % 4], paint);
                        }
                    } else {
                        // Fill
                        self.rect(*rect, paint);
                    }
                }
                PathCommand::AddCircle(center, radius) => {
                    self.circle(*center, *radius, paint);
                }
                PathCommand::AddOval(rect) => {
                    let rx = rect.width() / 2.0;
                    let ry = rect.height() / 2.0;
                    let center = Point::new(rect.left() + rx, rect.top() + ry);

                    if (rx - ry).abs() < 0.001 {
                        self.circle(center, rx, paint);
                    } else {
                        self.ellipse(center, rx, ry, paint);
                    }
                }
                PathCommand::AddArc(rect, start_angle, sweep_angle) => {
                    // Approximate arc with line segments
                    const SEGMENTS: usize = 32;
                    let center_x = rect.left() + rect.width() / 2.0;
                    let center_y = rect.top() + rect.height() / 2.0;
                    let rx = rect.width() / 2.0;
                    let ry = rect.height() / 2.0;

                    let angle_step = sweep_angle / SEGMENTS as f32;
                    let mut prev = Point::new(
                        center_x + rx * start_angle.cos(),
                        center_y + ry * start_angle.sin(),
                    );

                    for i in 1..=SEGMENTS {
                        let angle = start_angle + angle_step * i as f32;
                        let current =
                            Point::new(center_x + rx * angle.cos(), center_y + ry * angle.sin());
                        self.line(prev, current, paint);
                        prev = current;
                    }
                }
            }
        }
    }

    // ========== Advanced ==========

    /// Set opacity for subsequent draws (0.0 = transparent, 1.0 = opaque)
    fn set_opacity(&mut self, opacity: f32);

    // ========== Offscreen Rendering (Layer Support) ==========

    /// Begin rendering to an offscreen layer
    ///
    /// Creates an offscreen rendering context where all subsequent drawing operations
    /// are captured until `restore()` is called. The layer is then composited back to
    /// the main surface using the provided paint settings (opacity, blend mode, etc.).
    ///
    /// # Parameters
    /// - `bounds`: The rectangular region for the layer (in current coordinate space)
    /// - `paint`: Paint settings for compositing the layer (opacity, blend mode)
    ///
    /// # Usage Pattern
    /// ```rust,ignore
    /// painter.save_layer(bounds, &Paint::new().with_opacity(0.5));
    /// // ... draw content that should be in the layer ...
    /// painter.restore(); // Composites layer back with 50% opacity
    /// ```
    ///
    /// # Backend Requirements
    /// Full implementation requires GPU backend support for render-to-texture.
    /// The default implementation falls back to `save()` without offscreen rendering,
    /// meaning effects like opacity and blend modes are applied per-primitive instead
    /// of to the entire layer group.
    ///
    /// # Use Cases
    /// - Group opacity: Apply opacity to a group of shapes atomically
    /// - Color filters: Apply color transformations to child content
    /// - Advanced blend modes: Composite groups with special blend modes
    /// - Caching: Pre-render complex content to a texture
    ///
    /// # Default Implementation
    /// Falls back to regular `save()` for backends without offscreen support.
    fn save_layer(&mut self, _bounds: Rect, _paint: &Paint) {
        // Default: just save state (no actual offscreen rendering)
        // Backends with render-to-texture support should override
        self.save();
    }

    /// Begin rendering to a layer with backdrop capture
    ///
    /// Similar to `save_layer()`, but captures the current backdrop (what's already
    /// been painted behind this layer) for use in backdrop filters like blur.
    ///
    /// # Parameters
    /// - `bounds`: The rectangular region for the layer (in current coordinate space)
    ///
    /// # Usage Pattern
    /// ```rust,ignore
    /// painter.save_layer_backdrop(bounds);
    /// // ... apply backdrop filter (e.g., blur the captured backdrop) ...
    /// // ... draw child content on top ...
    /// painter.restore();
    /// ```
    ///
    /// # Backend Requirements
    /// Requires GPU backend with framebuffer read capabilities. This is a more
    /// advanced feature than basic `save_layer()` because it needs to capture
    /// the current rendered output before proceeding.
    ///
    /// # Use Cases
    /// - Backdrop blur (frosted glass effect)
    /// - Backdrop brightness/saturation adjustments
    /// - Content-aware backgrounds
    ///
    /// # Default Implementation
    /// Falls back to regular `save()` without backdrop capture.
    /// The backdrop filter effect will not be visible without proper backend support.
    fn save_layer_backdrop(&mut self, _bounds: Rect) {
        // Default: just save state (no backdrop capture)
        // Backends with framebuffer read support should override
        self.save();
    }

    // ========== Convenience Methods (Default Implementations) ==========

    /// Draw a rectangle with a drop shadow
    ///
    /// # Parameters
    /// - `rect`: The rectangle to draw
    /// - `paint`: Paint style for the rectangle
    /// - `shadow_offset`: Offset of the shadow (dx, dy)
    /// - `shadow_blur`: Blur radius of the shadow
    /// - `shadow_color`: Color of the shadow
    fn rect_with_shadow(
        &mut self,
        rect: Rect,
        paint: &Paint,
        shadow_offset: Offset,
        shadow_blur: f32,
        shadow_color: Color,
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

            let shadow_paint = Paint::fill(shadow_color.with_opacity(shadow_color.alpha_f32() * opacity));

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
        let base_color = paint.color.to_rgba_f32_array();
        for i in (0..layers).rev() {
            let t = i as f32 / (layers - 1) as f32;
            let falloff = 1.0 - t;
            let eased = falloff * falloff * falloff; // Cubic easing

            let glow_color = Color::from_rgba_f32_array([
                base_color[0],
                base_color[1],
                base_color[2],
                base_color[3] * eased * glow_intensity,
            ]);

            let current_radius = radius + (1.0 - eased) * glow_radius;
            self.circle(
                center,
                current_radius,
                &Paint::fill(glow_color),
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
    /// - `shadow_color`: Color of the shadow
    fn text_with_shadow(
        &mut self,
        text: &str,
        position: Point,
        font_size: f32,
        paint: &Paint,
        shadow_offset: Offset,
        shadow_color: Color,
    ) {
        // Draw shadow
        let shadow_pos = Point::new(position.x + shadow_offset.dx, position.y + shadow_offset.dy);
        let shadow_paint = paint.clone().with_color(shadow_color);
        self.text(text, shadow_pos, font_size, &shadow_paint);

        // Draw main text
        self.text(text, position, font_size, paint);
    }

    /// Draw a horizontal gradient
    ///
    /// # Parameters
    /// - `rect`: Rectangle area to fill with gradient
    /// - `start_color`: Color at the left edge
    /// - `end_color`: Color at the right edge
    fn horizontal_gradient(&mut self, rect: Rect, start_color: Color, end_color: Color) {
        let steps = 50;
        let step_width = rect.width() / steps as f32;

        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let color = Color::lerp(start_color, end_color, t);

            let x = rect.left() + i as f32 * step_width;
            let strip = Rect::from_xywh(x, rect.top(), step_width, rect.height());

            self.rect(strip, &Paint::fill(color));
        }
    }

    /// Draw a vertical gradient
    ///
    /// # Parameters
    /// - `rect`: Rectangle area to fill with gradient
    /// - `start_color`: Color at the top edge
    /// - `end_color`: Color at the bottom edge
    fn vertical_gradient(&mut self, rect: Rect, start_color: Color, end_color: Color) {
        let steps = 60;
        let step_height = rect.height() / steps as f32;

        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let color = Color::lerp(start_color, end_color, t);

            let y = rect.top() + i as f32 * step_height;
            let strip = Rect::from_xywh(rect.left(), y, rect.width(), step_height);

            self.rect(strip, &Paint::fill(color));
        }
    }

    /// Draw a simple radial gradient (legacy helper method)
    ///
    /// # Parameters
    /// - `center`: Center point of the gradient
    /// - `inner_radius`: Radius where start_color begins
    /// - `outer_radius`: Radius where end_color ends
    /// - `start_color`: Color at the center
    /// - `end_color`: Color at the outer edge
    fn radial_gradient_simple(
        &mut self,
        center: Point,
        inner_radius: f32,
        outer_radius: f32,
        start_color: Color,
        end_color: Color,
    ) {
        let steps = 30;

        // Draw from outside to inside for proper layering
        for i in (0..steps).rev() {
            let t = i as f32 / (steps - 1) as f32;
            let color = Color::lerp(start_color, end_color, t);
            let radius = inner_radius + t * (outer_radius - inner_radius);

            self.circle(center, radius, &Paint::fill(color));
        }
    }

    /// Draw a rounded rectangle with a drop shadow
    ///
    /// # Parameters
    /// - `rrect`: The rounded rectangle to draw
    /// - `paint`: Paint style for the rectangle
    /// - `shadow_offset`: Offset of the shadow (dx, dy)
    /// - `shadow_blur`: Blur radius of the shadow
    /// - `shadow_color`: Color of the shadow
    fn rrect_with_shadow(
        &mut self,
        rrect: RRect,
        paint: &Paint,
        shadow_offset: Offset,
        shadow_blur: f32,
        shadow_color: Color,
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
                top_left: rrect.top_left,
                top_right: rrect.top_right,
                bottom_right: rrect.bottom_right,
                bottom_left: rrect.bottom_left,
            };

            let shadow_paint = Paint::fill(shadow_color.with_opacity((shadow_color.alpha_f32() * opacity).min(1.0)));

            self.rrect(shadow_rrect, &shadow_paint);
        }

        // Draw main rounded rectangle
        self.rrect(rrect, paint);
    }
}
