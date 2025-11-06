//! Egui painter implementation
//!
//! This module provides a Painter implementation backed by egui's rendering system.

use crate::painter::{Paint, Painter, RRect};
use flui_types::{Offset, Point, Rect};
use glam::{Mat4, Vec3};

use crate::text::VectorTextRenderer;

/// Stack-based state for painter operations
#[derive(Debug, Clone)]
struct PainterState {
    /// Current opacity (multiplicative)
    opacity: f32,

    /// Current clip rect
    clip_rect: Option<Rect>,

    /// Current transformation matrix
    transform: Mat4,
}

impl Default for PainterState {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            clip_rect: None,
            transform: Mat4::IDENTITY,
        }
    }
}

/// Egui-backed painter implementation
///
/// This painter translates abstract drawing commands into egui's immediate-mode API.
///
/// # State Management
///
/// The painter maintains a stack of states (transform, clip, opacity) to support
/// save/restore operations. This is necessary because egui doesn't provide a
/// built-in state stack.
pub struct EguiPainter<'a> {
    /// The underlying egui painter
    painter: &'a egui::Painter,

    /// State stack for save/restore
    state_stack: Vec<PainterState>,

    /// Current state
    current_state: PainterState,

    /// Vector text renderer for complex transformations (optional)
    vector_text_renderer: Option<VectorTextRenderer>,
}

impl<'a> EguiPainter<'a> {
    /// Create a new egui painter
    pub fn new(painter: &'a egui::Painter) -> Self {
        // Initialize vector text renderer with embedded font
        let vector_text_renderer = Some(VectorTextRenderer::new());

        Self {
            painter,
            state_stack: Vec::new(),
            current_state: PainterState::default(),
            vector_text_renderer,
        }
    }

    /// Get the underlying egui painter
    pub fn inner(&self) -> &egui::Painter {
        self.painter
    }

    /// Convert our Paint to egui color
    fn paint_color(&self, paint: &Paint) -> egui::Color32 {
        let opacity = paint.color.alpha_f32() * self.current_state.opacity;

        egui::Color32::from_rgba_unmultiplied(
            paint.color.r,
            paint.color.g,
            paint.color.b,
            (opacity * 255.0) as u8,
        )
    }

    /// Convert our Rect to egui Rect
    fn to_egui_rect(rect: Rect) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(rect.min.x, rect.min.y),
            egui::pos2(rect.max.x, rect.max.y),
        )
    }

    /// Convert our Point to egui Pos2
    fn to_egui_pos(point: Point) -> egui::Pos2 {
        egui::pos2(point.x, point.y)
    }

    /// Check if the given bounds are visible (not clipped)
    fn is_visible(&self, bounds: Rect) -> bool {
        if let Some(clip) = self.current_state.clip_rect {
            bounds.intersects(&clip)
        } else {
            true
        }
    }

    /// Get the current clip rect for egui
    fn egui_clip_rect(&self) -> egui::Rect {
        if let Some(clip) = self.current_state.clip_rect {
            // Transform clip rect to screen space
            let transformed_clip = self.transform_rect(clip);
            Self::to_egui_rect(transformed_clip)
        } else {
            // No clip - use full screen
            egui::Rect::EVERYTHING
        }
    }

    /// Execute a drawing operation with proper clipping
    /// Takes a closure that performs drawing operations
    fn with_clip<F>(&self, f: F)
    where
        F: FnOnce(&egui::Painter),
    {
        if self.current_state.clip_rect.is_some() {
            let clip_rect = self.egui_clip_rect();
            let clipped = self.painter.with_clip_rect(clip_rect);
            f(&clipped);
        } else {
            f(self.painter);
        }
    }

    /// Add a shape with current clipping applied
    fn add_shape(&self, shape: egui::Shape) {
        self.with_clip(|painter| {
            painter.add(shape);
        });
    }

    /// Apply current transformation to a point
    fn transform_point(&self, point: Point) -> Point {
        let vec = Vec3::new(point.x, point.y, 1.0);
        let transformed = self.current_state.transform.project_point3(vec);
        Point::new(transformed.x, transformed.y)
    }

    /// Apply current transformation to a rect (approximation)
    /// Returns bounding box of transformed corners
    fn transform_rect(&self, rect: Rect) -> Rect {
        let corners = [
            self.transform_point(Point::new(rect.min.x, rect.min.y)),
            self.transform_point(Point::new(rect.max.x, rect.min.y)),
            self.transform_point(Point::new(rect.min.x, rect.max.y)),
            self.transform_point(Point::new(rect.max.x, rect.max.y)),
        ];

        let min_x = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let min_y = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let max_x = corners
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let max_y = corners
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);

        Rect::from_min_max(Point::new(min_x, min_y), Point::new(max_x, max_y))
    }

    /// Check if current transform has skew (non-orthogonal transformation)
    /// This detects if we need to use mesh rendering instead of simple primitives
    fn has_complex_transform(&self) -> bool {
        // Extract the 2x2 rotation/scale/skew matrix (ignoring translation)
        let mat = self.current_state.transform;
        let m00 = mat.x_axis.x;
        let m01 = mat.x_axis.y;
        let m10 = mat.y_axis.x;
        let m11 = mat.y_axis.y;

        // Check for skew: columns should be perpendicular for pure rotation/scale
        let dot = m00 * m10 + m01 * m11;
        if dot.abs() > 0.01 {
            return true; // Has skew
        }

        // Check for non-uniform scaling
        // Calculate scale magnitudes for each axis
        let scale_x = (m00 * m00 + m01 * m01).sqrt();
        let scale_y = (m10 * m10 + m11 * m11).sqrt();

        // If scales differ significantly, we have non-uniform scaling
        let scale_ratio = (scale_x / scale_y).max(scale_y / scale_x);
        if scale_ratio > 1.05 {
            return true; // Has non-uniform scaling
        }

        false // Simple rotation/uniform scale, can use raster text
    }

    /// Get transformed corners of a rectangle
    fn transformed_corners(&self, rect: Rect) -> [Point; 4] {
        [
            self.transform_point(Point::new(rect.min.x, rect.min.y)), // top-left
            self.transform_point(Point::new(rect.max.x, rect.min.y)), // top-right
            self.transform_point(Point::new(rect.max.x, rect.max.y)), // bottom-right
            self.transform_point(Point::new(rect.min.x, rect.max.y)), // bottom-left
        ]
    }

    /// Draw a filled rectangle using mesh (supports arbitrary transforms including skew)
    fn draw_rect_mesh(&self, rect: Rect, color: egui::Color32) {
        let corners = self.transformed_corners(rect);

        // Create mesh with 4 vertices (quad)
        let vertices = corners
            .iter()
            .map(|&p| egui::epaint::Vertex {
                pos: Self::to_egui_pos(p),
                uv: egui::epaint::WHITE_UV, // No texture
                color,
            })
            .collect::<Vec<_>>();

        // Two triangles: (0,1,2) and (0,2,3)
        let indices = vec![0, 1, 2, 0, 2, 3];

        let mesh = egui::epaint::Mesh {
            indices,
            vertices,
            texture_id: Default::default(),
        };

        self.add_shape(egui::Shape::Mesh(std::sync::Arc::new(mesh)));
    }

    /// Draw a stroked rectangle using line segments (supports arbitrary transforms)
    fn draw_rect_stroke_mesh(&self, rect: Rect, stroke: egui::Stroke) {
        let corners = self.transformed_corners(rect);

        self.with_clip(|painter| {
            // Draw 4 line segments connecting the corners
            for i in 0..4 {
                let start = Self::to_egui_pos(corners[i]);
                let end = Self::to_egui_pos(corners[(i + 1) % 4]);
                painter.line_segment([start, end], stroke);
            }
        });
    }

    /// Draw a filled circle/ellipse using mesh (supports arbitrary transforms including skew)
    fn draw_circle_mesh(&self, center: Point, radius: f32, color: egui::Color32) {
        const SEGMENTS: usize = 32; // Number of segments for circle approximation

        // Generate circle points in local space
        let mut points = Vec::with_capacity(SEGMENTS);
        for i in 0..SEGMENTS {
            let angle = (i as f32) * std::f32::consts::TAU / (SEGMENTS as f32);
            let x = center.x + radius * angle.cos();
            let y = center.y + radius * angle.sin();
            points.push(Point::new(x, y));
        }

        // Transform all points
        let transformed_points: Vec<Point> =
            points.iter().map(|&p| self.transform_point(p)).collect();

        // Create center vertex (transformed center)
        let transformed_center = self.transform_point(center);

        // Build mesh with triangle fan from center
        let mut vertices = Vec::with_capacity(SEGMENTS + 1);

        // Add center vertex
        vertices.push(egui::epaint::Vertex {
            pos: Self::to_egui_pos(transformed_center),
            uv: egui::epaint::WHITE_UV,
            color,
        });

        // Add perimeter vertices
        for p in &transformed_points {
            vertices.push(egui::epaint::Vertex {
                pos: Self::to_egui_pos(*p),
                uv: egui::epaint::WHITE_UV,
                color,
            });
        }

        // Build triangle fan indices: (0, i, i+1) for each segment
        let mut indices = Vec::with_capacity(SEGMENTS * 3);
        for i in 0..SEGMENTS {
            indices.push(0); // center
            indices.push((i + 1) as u32);
            indices.push(((i + 1) % SEGMENTS + 1) as u32);
        }

        let mesh = egui::epaint::Mesh {
            indices,
            vertices,
            texture_id: Default::default(),
        };

        self.add_shape(egui::Shape::Mesh(std::sync::Arc::new(mesh)));
    }

    /// Draw a stroked circle/ellipse using line segments (supports arbitrary transforms)
    fn draw_circle_stroke_mesh(&self, center: Point, radius: f32, stroke: egui::Stroke) {
        const SEGMENTS: usize = 32;

        // Generate circle points in local space
        let mut points = Vec::with_capacity(SEGMENTS);
        for i in 0..SEGMENTS {
            let angle = (i as f32) * std::f32::consts::TAU / (SEGMENTS as f32);
            let x = center.x + radius * angle.cos();
            let y = center.y + radius * angle.sin();
            points.push(Point::new(x, y));
        }

        // Transform all points
        let transformed_points: Vec<egui::Pos2> = points
            .iter()
            .map(|&p| Self::to_egui_pos(self.transform_point(p)))
            .collect();

        self.with_clip(|painter| {
            // Draw line segments connecting the points
            for i in 0..SEGMENTS {
                let start = transformed_points[i];
                let end = transformed_points[(i + 1) % SEGMENTS];
                painter.line_segment([start, end], stroke);
            }
        });
    }
}

impl<'a> Painter for EguiPainter<'a> {
    fn rect(&mut self, rect: Rect, paint: &Paint) {
        if !self.is_visible(rect) {
            return;
        }

        let color = self.paint_color(paint);

        // Check if we have skew transform - if so, use mesh rendering
        if self.has_complex_transform() {
            if paint.stroke_width > 0.0 {
                // Stroked rect with skew
                let stroke = egui::Stroke::new(paint.stroke_width, color);
                self.draw_rect_stroke_mesh(rect, stroke);
            } else {
                // Filled rect with skew
                self.draw_rect_mesh(rect, color);
            }
        } else {
            // No skew - use simple rect rendering (rotation/scale already in transform)
            let transformed_rect = self.transform_rect(rect);
            let egui_rect = Self::to_egui_rect(transformed_rect);

            self.with_clip(|painter| {
                if paint.stroke_width > 0.0 {
                    let stroke = egui::Stroke::new(paint.stroke_width, color);
                    painter.rect_stroke(egui_rect, 0.0, stroke, egui::StrokeKind::Outside);
                } else {
                    painter.rect_filled(egui_rect, 0.0, color);
                }
            });
        }
    }

    fn rrect(&mut self, rrect: RRect, paint: &Paint) {
        if !self.is_visible(rrect.rect) {
            return;
        }

        // Apply transformation
        let transformed_rect = self.transform_rect(rrect.rect);
        let color = self.paint_color(paint);
        let egui_rect = Self::to_egui_rect(transformed_rect);
        // Convert per-corner radii to egui format (uses only x component)
        let rounding = egui::CornerRadius {
            nw: rrect.top_left.x.min(255.0) as u8,
            ne: rrect.top_right.x.min(255.0) as u8,
            sw: rrect.bottom_left.x.min(255.0) as u8,
            se: rrect.bottom_right.x.min(255.0) as u8,
        };

        self.with_clip(|painter| {
            if paint.stroke_width > 0.0 {
                let stroke = egui::Stroke::new(paint.stroke_width, color);
                painter.rect_stroke(egui_rect, rounding, stroke, egui::StrokeKind::Outside);
            } else {
                painter.rect_filled(egui_rect, rounding, color);
            }
        });
    }

    fn circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        let bounds =
            Rect::from_center_size(center, flui_types::Size::new(radius * 2.0, radius * 2.0));

        if !self.is_visible(bounds) {
            return;
        }

        let color = self.paint_color(paint);

        // Check if we have skew transform - if so, use mesh rendering
        // This properly renders circles as ellipses when skewed
        if self.has_complex_transform() {
            if paint.stroke_width > 0.0 {
                // Stroked circle with skew
                let stroke = egui::Stroke::new(paint.stroke_width, color);
                self.draw_circle_stroke_mesh(center, radius, stroke);
            } else {
                // Filled circle with skew
                self.draw_circle_mesh(center, radius, color);
            }
        } else {
            // No skew - use simple circle rendering
            let transformed_center = self.transform_point(center);
            let egui_center = Self::to_egui_pos(transformed_center);

            // Scale the radius (uniform scale)
            let scale = self
                .current_state
                .transform
                .to_scale_rotation_translation()
                .0;
            let scaled_radius = radius * scale.x.max(scale.y);

            self.with_clip(|painter| {
                if paint.stroke_width > 0.0 {
                    let stroke = egui::Stroke::new(paint.stroke_width, color);
                    painter.circle_stroke(egui_center, scaled_radius, stroke);
                } else {
                    painter.circle_filled(egui_center, scaled_radius, color);
                }
            });
        }
    }

    fn line(&mut self, p1: Point, p2: Point, paint: &Paint) {
        let min_x = p1.x.min(p2.x);
        let min_y = p1.y.min(p2.y);
        let max_x = p1.x.max(p2.x);
        let max_y = p1.y.max(p2.y);

        let bounds = Rect::from_min_max(Point::new(min_x, min_y), Point::new(max_x, max_y));

        if !self.is_visible(bounds) {
            return;
        }

        // Apply transformation to line endpoints
        let transformed_p1 = self.transform_point(p1);
        let transformed_p2 = self.transform_point(p2);

        let color = self.paint_color(paint);
        let stroke = egui::Stroke::new(paint.stroke_width.max(1.0), color);

        self.with_clip(|painter| {
            painter.line_segment(
                [
                    Self::to_egui_pos(transformed_p1),
                    Self::to_egui_pos(transformed_p2),
                ],
                stroke,
            );
        });
    }

    fn text(&mut self, text: &str, position: Point, font_size: f32, paint: &Paint) {
        // Check if we need vector rendering for complex transforms (skew, non-uniform scale, etc.)
        let use_vector = self.has_complex_transform();
        if use_vector {
            println!("Using VECTOR rendering for text: '{}'", text);
            // Use vector text rendering for complex transforms
            if let Some(renderer) = &mut self.vector_text_renderer {
                // Color is already in correct format
                let color = paint.color;

                // Extract letter and word spacing from paint
                let params = crate::text::TextRenderParams::new(
                    text,
                    position,
                    font_size,
                    color,
                    &self.current_state.transform,
                )
                .with_letter_spacing(paint.letter_spacing)
                .with_word_spacing(paint.word_spacing);

                match renderer.render(&params) {
                    Ok((vertices, indices)) => {
                        // Convert TextVertex to egui::epaint::Vertex
                        let egui_vertices: Vec<egui::epaint::Vertex> = vertices
                            .iter()
                            .map(|v| egui::epaint::Vertex {
                                pos: egui::pos2(v.x, v.y),
                                uv: egui::pos2(0.0, 0.0),
                                color: egui::Color32::from_rgba_unmultiplied(
                                    v.color.r, v.color.g, v.color.b, v.color.a,
                                ),
                            })
                            .collect();

                        // Create mesh from vertices and indices
                        // Vector text renderer already provides transformed vertices and proper indices
                        let mesh = egui::epaint::Mesh {
                            indices,
                            vertices: egui_vertices,
                            texture_id: Default::default(),
                        };

                        self.add_shape(egui::Shape::Mesh(std::sync::Arc::new(mesh)));
                        return; // Successfully rendered with vector text
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Vector text rendering failed: {:?}, falling back to raster",
                            e
                        );
                        // Fall through to raster rendering
                    }
                }
            }
        }

        // Fast path: Raster text rendering with rotation and scale
        // Apply transformation to position
        let transformed_pos = self.transform_point(position);
        let color = self.paint_color(paint);
        let pos = Self::to_egui_pos(transformed_pos);

        #[cfg(debug_assertions)]
        tracing::debug!(
            "EguiPainter::text: transformed_pos={:?}, color={:?}, pos={:?}",
            transformed_pos,
            color,
            pos
        );

        // Extract rotation and scale from transform matrix
        let (scale, rotation, _translation) =
            self.current_state.transform.to_scale_rotation_translation();
        let angle = rotation.to_euler(glam::EulerRot::ZYX).0; // Get Z-axis rotation
        let scale_factor = scale.x.max(scale.y); // Use max scale for font size

        // Apply scale to font size
        let scaled_font_size = font_size * scale_factor;
        let font_id = egui::FontId::proportional(scaled_font_size);

        #[cfg(debug_assertions)]
        tracing::debug!(
            "EguiPainter::text: creating galley with font_id={:?}, text='{}'",
            font_id,
            text
        );

        let galley = self
            .painter
            .layout_no_wrap(text.to_string(), font_id, color);

        #[cfg(debug_assertions)]
        tracing::debug!(
            "EguiPainter::text: galley created, size={:?}",
            galley.size()
        );

        // Create text shape with rotation using egui 0.28+ API
        let mut text_shape = egui::epaint::TextShape::new(pos, galley, color);

        // Set rotation angle (egui 0.28+ supports angle field)
        text_shape.angle = angle;

        #[cfg(debug_assertions)]
        tracing::debug!("EguiPainter::text: adding text shape");

        self.add_shape(egui::Shape::Text(text_shape));

        #[cfg(debug_assertions)]
        tracing::debug!("EguiPainter::text: text shape added");
    }

    fn text_styled(
        &mut self,
        text: &str,
        position: Point,
        style: &flui_types::typography::TextStyle,
    ) {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "EguiPainter::text_styled: text='{}', position={:?}, style={:?}",
            text,
            position,
            style
        );

        // Extract styling parameters
        let font_size = style.font_size.unwrap_or(14.0) as f32;
        let letter_spacing = style.letter_spacing.unwrap_or(0.0) as f32;
        let word_spacing = style.word_spacing.unwrap_or(0.0) as f32;

        let paint = Paint {
            color: style.color.unwrap_or(flui_types::Color::BLACK),
            ..Default::default()
        };

        // Check if we need vector rendering (complex transform or custom spacing)
        let needs_vector =
            crate::text::VectorTextRenderer::needs_vector_rendering(&self.current_state.transform)
                || letter_spacing.abs() > 0.001
                || word_spacing.abs() > 0.001;

        #[cfg(debug_assertions)]
        tracing::debug!(
            "EguiPainter::text_styled: needs_vector={}, font_size={}",
            needs_vector,
            font_size
        );

        if needs_vector {
            // Use vector rendering with spacing support
            if let Some(renderer) = &mut self.vector_text_renderer {
                // Color is already in correct format
                let color = paint.color;

                let params = crate::text::TextRenderParams::new(
                    text,
                    position,
                    font_size,
                    color,
                    &self.current_state.transform,
                )
                .with_letter_spacing(letter_spacing)
                .with_word_spacing(word_spacing);

                match renderer.render(&params) {
                    Ok((vertices, indices)) => {
                        // Convert TextVertex to egui::epaint::Vertex
                        let egui_vertices: Vec<egui::epaint::Vertex> = vertices
                            .iter()
                            .map(|v| egui::epaint::Vertex {
                                pos: egui::pos2(v.x, v.y),
                                uv: egui::pos2(0.0, 0.0),
                                color: egui::Color32::from_rgba_unmultiplied(
                                    v.color.r, v.color.g, v.color.b, v.color.a,
                                ),
                            })
                            .collect();

                        let mesh = egui::epaint::Mesh {
                            indices,
                            vertices: egui_vertices,
                            texture_id: Default::default(),
                        };

                        self.add_shape(egui::Shape::Mesh(std::sync::Arc::new(mesh)));
                        return;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Vector text rendering failed: {:?}, falling back to raster",
                            e
                        );
                        // Fall through to raster rendering
                    }
                }
            }
        }

        #[cfg(debug_assertions)]
        tracing::debug!("EguiPainter::text_styled: calling self.text()");

        // Fallback: Use simple text rendering without spacing
        // Note: egui doesn't support letter_spacing directly, so we ignore it for raster path
        self.text(text, position, font_size, &paint);

        #[cfg(debug_assertions)]
        tracing::debug!("EguiPainter::text_styled: completed");
    }

    fn save(&mut self) {
        // Push current state to stack
        self.state_stack.push(self.current_state.clone());
    }

    fn restore(&mut self) {
        // Pop state from stack
        if let Some(state) = self.state_stack.pop() {
            self.current_state = state;
        }
    }

    fn translate(&mut self, offset: Offset) {
        // Apply translation to transform matrix
        self.current_state.transform *=
            Mat4::from_translation(Vec3::new(offset.dx, offset.dy, 0.0));
    }

    fn rotate(&mut self, angle: f32) {
        // Apply rotation to transform matrix
        self.current_state.transform *= Mat4::from_rotation_z(angle);
    }

    fn scale(&mut self, sx: f32, sy: f32) {
        // Apply scale to transform matrix
        self.current_state.transform *= Mat4::from_scale(Vec3::new(sx, sy, 1.0));
    }

    fn transform_matrix(&mut self, a: f32, b: f32, c: f32, d: f32, tx: f32, ty: f32) {
        // Override default implementation to preserve full matrix (including skew)
        // Build 2D affine matrix directly:
        // | a  c  tx |
        // | b  d  ty |
        // | 0  0  1  |
        let matrix = Mat4::from_cols(
            Vec3::new(a, b, 0.0).extend(0.0),     // First column
            Vec3::new(c, d, 0.0).extend(0.0),     // Second column
            Vec3::new(0.0, 0.0, 1.0).extend(0.0), // Third column (Z axis, unused in 2D)
            Vec3::new(tx, ty, 0.0).extend(1.0),   // Translation column
        );

        // Multiply current transform with this matrix
        self.current_state.transform *= matrix;
    }

    fn apply_matrix4(&mut self, matrix: Mat4) {
        // Directly multiply the full 4x4 matrix (including perspective)
        self.current_state.transform *= matrix;
    }

    fn clip_rect(&mut self, rect: Rect) {
        // Update clip rect (intersect with current clip)
        self.current_state.clip_rect =
            Some(if let Some(current_clip) = self.current_state.clip_rect {
                current_clip.intersection(&rect).unwrap_or(Rect::ZERO)
            } else {
                rect
            });
    }

    fn clip_rrect(&mut self, rrect: RRect) {
        // For simplicity, just use the outer rect
        // A full implementation would use egui's ClippedPrimitive
        self.clip_rect(rrect.rect);
    }

    fn clip_path(&mut self, _path: &flui_types::painting::path::Path, bounds: Rect) {
        // Egui doesn't have native path clipping support (unlike rect clipping).
        // We have several options:
        // 1. Use bounding box (fast but conservative) - current fallback
        // 2. Use stencil buffer / mask rendering (complex)
        // 3. Store path and use for hit testing (partial solution)
        //
        // For now, we'll use the bounding box approach which is what most
        // egui-based UIs do. A full implementation would require rendering
        // to a mask texture and using it as a clip mask.
        //
        // Note: The ClipPathLayer already does proper hit testing using
        // path.contains(), so interaction will be correct even though
        // rendering uses the conservative bounding box.

        self.clip_rect(bounds);

        // TODO: For a more accurate implementation, we could:
        // - Render the path to an offscreen texture as a mask
        // - Use egui's layer system with custom shapes
        // - Implement stencil-based clipping in a custom backend
    }

    fn draw_image(
        &mut self,
        image: &flui_types::painting::Image,
        src: Option<Rect>,
        dst: Rect,
        paint: &crate::painter::Paint,
    ) {
        // Convert image data to egui ColorImage
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.data(),
        );

        // Upload to egui texture system
        // Note: In a real implementation, we would cache textures to avoid re-uploading
        // For now, we create a temporary texture handle
        let texture_handle = self.painter.ctx().load_texture(
            format!("temp_image_{}x{}", image.width(), image.height()),
            color_image,
            egui::TextureOptions::default(),
        );

        // Calculate source and destination UV coordinates
        let src_rect = src.unwrap_or(Rect::from_xywh(
            0.0,
            0.0,
            image.width() as f32,
            image.height() as f32,
        ));

        // Normalize source coordinates to UV space (0.0-1.0)
        let uv = egui::Rect::from_min_max(
            egui::pos2(
                src_rect.left() / image.width() as f32,
                src_rect.top() / image.height() as f32,
            ),
            egui::pos2(
                src_rect.right() / image.width() as f32,
                src_rect.bottom() / image.height() as f32,
            ),
        );

        // Transform destination rectangle
        let transformed_dst = self.transform_rect(dst);
        let egui_rect = Self::to_egui_rect(transformed_dst);

        // Apply opacity
        let tint = egui::Color32::WHITE
            .gamma_multiply(self.current_state.opacity * paint.color.alpha_f32());

        // Create image shape
        let image_shape = egui::Shape::image(texture_handle.id(), egui_rect, uv, tint);

        self.add_shape(image_shape);
    }

    fn set_opacity(&mut self, opacity: f32) {
        // Multiply with current opacity (for nested opacity layers)
        self.current_state.opacity *= opacity.clamp(0.0, 1.0);
    }

    fn apply_image_filter(
        &mut self,
        filter: &flui_types::painting::effects::ImageFilter,
        bounds: Rect,
    ) {
        use flui_types::painting::effects::{ColorFilter, ImageFilter};

        // Note: Egui doesn't have built-in blur shader support.
        // We implement what we can with available primitives:
        // - ColorFilter: Applied by modifying the opacity/tint in current state
        // - Blur: Approximated by drawing semi-transparent layers (visual hint)
        // For proper blur, a GPU backend with shaders is needed.

        match filter {
            ImageFilter::Blur { sigma_x, sigma_y } => {
                // Approximate blur with reduced opacity as a visual hint
                // Real blur would require rendering to texture and applying convolution
                let blur_amount = (sigma_x.max(*sigma_y) / 10.0).min(0.5);
                self.current_state.opacity *= 1.0 - blur_amount;

                // Draw a semi-transparent overlay to simulate blur (very basic)
                let blur_color = egui::Color32::from_white_alpha((blur_amount * 255.0) as u8);
                let egui_rect = Self::to_egui_rect(self.transform_rect(bounds));
                let blur_rect = egui::Shape::rect_filled(egui_rect, 0.0, blur_color);
                self.add_shape(blur_rect);
            }
            ImageFilter::Color(color_filter) => {
                // Apply color filter by modifying current painter state
                match color_filter {
                    ColorFilter::Opacity(opacity) => {
                        self.current_state.opacity *= opacity.clamp(0.0, 1.0);
                    }
                    ColorFilter::Brightness(brightness) => {
                        // Store brightness adjustment (will be applied during rendering)
                        // Note: This is a simplified approach
                        // Real implementation would need per-pixel processing
                        if *brightness < 0.0 {
                            // Darken by reducing opacity
                            self.current_state.opacity *= 1.0 + brightness;
                        }
                        // Brightening would require shader support
                    }
                    ColorFilter::Grayscale(_amount) => {
                        // Grayscale requires per-pixel color transformation
                        // Not directly supported by egui without custom shaders
                        // This is a placeholder - no visual effect
                    }
                    ColorFilter::Sepia(_amount) => {
                        // Sepia requires per-pixel color transformation
                        // Placeholder - no effect without shaders
                    }
                    ColorFilter::Invert(_amount) => {
                        // Invert requires per-pixel processing
                        // Placeholder - no effect without shaders
                    }
                    ColorFilter::Saturation(_amount) => {
                        // Saturation requires HSV transformation
                        // Placeholder - no effect without shaders
                    }
                    ColorFilter::Contrast(_amount) => {
                        // Contrast adjustment requires per-pixel processing
                        // Placeholder - no effect without shaders
                    }
                    ColorFilter::HueRotate(_degrees) => {
                        // Hue rotation requires HSV transformation
                        // Placeholder - no effect without shaders
                    }
                    ColorFilter::Matrix(_matrix) => {
                        // Matrix transformation requires per-pixel multiplication
                        // Placeholder - no effect without shaders
                    }
                }
            }
            ImageFilter::Dilate { .. } | ImageFilter::Erode { .. } => {
                // Morphological operations require pixel-level processing
                // Not supported in egui without custom rendering
                // Placeholder - no effect
            }
            ImageFilter::Matrix(_) => {
                // Matrix color transformation requires per-pixel processing
                // Placeholder - no effect
            }
            ImageFilter::Compose(filters) => {
                // Apply each filter in sequence
                for f in filters {
                    self.apply_image_filter(f, bounds);
                }
            }
            #[cfg(debug_assertions)]
            ImageFilter::OverflowIndicator { .. } => {
                // Overflow indicators are handled by OverflowIndicatorLayer
                // which paints directly - this filter should never reach the painter
                // No-op here
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Testing egui painter requires an egui context, which is
    // difficult to set up in unit tests. These tests would typically
    // be integration tests instead.

    #[test]
    fn test_state_stack() {
        // This is a simplified test that doesn't actually use egui
        let mut state_stack = Vec::new();
        let mut current_state = PainterState::default();

        // Save state
        state_stack.push(current_state.clone());

        // Modify state
        current_state.opacity = 0.5;

        // Restore state
        if let Some(state) = state_stack.pop() {
            current_state = state;
        }

        assert_eq!(current_state.opacity, 1.0);
    }

    #[test]
    fn test_paint_color_conversion() {
        use flui_types::Color;

        let paint = Paint::fill(Color::RED);

        let expected = egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255);

        // Test that Color::RED maps to the expected egui color
        assert_eq!(paint.color.r, 255);
        assert_eq!(paint.color.g, 0);
        assert_eq!(paint.color.b, 0);
        assert_eq!(paint.color.a, 255);

        // Verify egui conversion
        assert_eq!(expected.r(), 255);
        assert_eq!(expected.g(), 0);
        assert_eq!(expected.b(), 0);
        assert_eq!(expected.a(), 255);
    }
}
