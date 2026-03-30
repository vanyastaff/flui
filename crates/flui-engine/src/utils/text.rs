//! Vector text rendering using ttf-parser and lyon
//!
//! This module provides vector-based text rendering that supports full matrix
//! transformations including skew and perspective. It converts font glyphs to
//! vector paths and tessellates them using Lyon.
//!
//! This is slower than raster text but supports arbitrary transformations.

use std::sync::Arc;

use flui_types::{Color, Pixels, Point, geometry::px};
use glam::Mat4;
use thiserror::Error;

/// Simple 2D vertex with position and color
#[derive(Debug, Clone, Copy)]
pub struct TextVertex {
    /// X coordinate
    pub x: f32,
    /// Y coordinate
    pub y: f32,
    /// Vertex color
    pub color: Color,
}

/// Parameters for text rendering
#[derive(Debug, Clone)]
pub struct TextRenderParams<'a> {
    /// Text to render
    pub text: &'a str,
    /// Starting position
    pub position: Point<Pixels>,
    /// Font size in pixels
    pub font_size: f32,
    /// Text color
    pub color: Color,
    /// Full 4x4 transformation matrix
    pub transform: &'a Mat4,
    /// Additional spacing between letters (default 0.0)
    pub letter_spacing: f32,
    /// Additional spacing between words (default 0.0)
    pub word_spacing: f32,
}

impl<'a> TextRenderParams<'a> {
    /// Create new text render parameters
    #[must_use]
    pub fn new(
        text: &'a str,
        position: Point<Pixels>,
        font_size: f32,
        color: Color,
        transform: &'a Mat4,
    ) -> Self {
        Self {
            text,
            position,
            font_size,
            color,
            transform,
            letter_spacing: 0.0,
            word_spacing: 0.0,
        }
    }

    /// Set letter spacing
    #[must_use]
    pub fn with_letter_spacing(mut self, spacing: f32) -> Self {
        self.letter_spacing = spacing;
        self
    }

    /// Set word spacing
    #[must_use]
    pub fn with_word_spacing(mut self, spacing: f32) -> Self {
        self.word_spacing = spacing;
        self
    }
}

/// Vector text renderer that converts glyphs to paths and tessellates them
///
/// Note: Debug is not derived because `FillTessellator` doesn't implement
/// Debug.
#[allow(missing_debug_implementations)]
pub struct VectorTextRenderer {
    /// Cached font face data
    font_data: Arc<Vec<u8>>,
    /// Lyon tessellator for converting paths to triangles
    fill_tessellator: lyon::tessellation::FillTessellator,
}

impl VectorTextRenderer {
    /// Create a new vector text renderer with the given font data
    #[must_use]
    pub fn new(font_data: Vec<u8>) -> Self {
        Self {
            font_data: Arc::new(font_data),
            fill_tessellator: lyon::tessellation::FillTessellator::new(),
        }
    }

    /// Load a different font
    pub fn load_font(&mut self, font_data: Vec<u8>) {
        self.font_data = Arc::new(font_data);
    }

    /// Get the current font data
    #[must_use]
    pub fn font_data(&self) -> &[u8] {
        &self.font_data
    }

    /// Render text as vector paths and return vertices and indices
    ///
    /// # Errors
    /// Returns error if font parsing or tessellation fails
    pub fn render(
        &mut self,
        params: &TextRenderParams<'_>,
    ) -> Result<(Vec<TextVertex>, Vec<u32>), VectorTextError> {
        // Parse font face
        let face = ttf_parser::Face::parse(&self.font_data, 0)
            .map_err(|_| VectorTextError::InvalidFont)?;

        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut x_offset = params.position.x;

        // Process each character
        for ch in params.text.chars() {
            // Get glyph ID for character
            let glyph_id = face
                .glyph_index(ch)
                .ok_or(VectorTextError::GlyphNotFound(ch))?;

            // Calculate scale for this glyph
            let scale = params.font_size / f32::from(face.units_per_em());

            // Create a new path builder for this glyph
            let mut path_builder = lyon::path::Builder::new();

            // Extract glyph outline in LOCAL space (no offsets yet)
            let mut outline_builder = LyonOutlineBuilder {
                builder: &mut path_builder,
                scale,
                offset_x: 0.0,
                offset_y: 0.0,
            };

            // Try to get outline - whitespace characters don't have one
            let has_outline = face.outline_glyph(glyph_id, &mut outline_builder).is_some();

            if !has_outline {
                // No outline (e.g., space character) - skip rendering but advance cursor
                if let Some(advance) = face.glyph_hor_advance(glyph_id) {
                    x_offset += px(f32::from(advance) * scale);
                }
                x_offset += px(params.letter_spacing);
                if ch.is_whitespace() {
                    x_offset += px(params.word_spacing);
                }
                continue;
            }

            // Build path
            let path = path_builder.build();

            // Tessellate path into triangles
            let mut buffers: lyon::tessellation::VertexBuffers<lyon::math::Point, u16> =
                lyon::tessellation::VertexBuffers::new();

            self.fill_tessellator
                .tessellate_path(
                    &path,
                    &lyon::tessellation::FillOptions::default(),
                    &mut lyon::tessellation::BuffersBuilder::new(
                        &mut buffers,
                        |vertex: lyon::tessellation::FillVertex<'_>| vertex.position(),
                    ),
                )
                .map_err(|_| VectorTextError::TessellationFailed)?;

            // Convert vertices with transform applied
            #[allow(clippy::cast_possible_truncation)]
            let vertex_offset = vertices.len() as u32;

            for point in &buffers.vertices {
                let local_x = point.x + x_offset.0;
                let local_y = point.y + params.position.y.0;

                // Apply full 4x4 transformation with perspective division
                let m = params.transform.to_cols_array_2d();

                let x = m[0][0] * local_x + m[1][0] * local_y + m[3][0];
                let y = m[0][1] * local_x + m[1][1] * local_y + m[3][1];
                let w = m[0][3] * local_x + m[1][3] * local_y + m[3][3];

                // Perspective division
                let (transformed_x, transformed_y) = if (w - 1.0).abs() > 0.001 {
                    (x / w, y / w)
                } else {
                    (x, y)
                };

                vertices.push(TextVertex {
                    x: transformed_x,
                    y: transformed_y,
                    color: params.color,
                });
            }

            // Add indices with offset
            for index in &buffers.indices {
                indices.push(vertex_offset + u32::from(*index));
            }

            // Advance cursor
            if let Some(advance) = face.glyph_hor_advance(glyph_id) {
                x_offset += px(f32::from(advance) * scale);
            }

            x_offset += px(params.letter_spacing);

            if ch.is_whitespace() {
                x_offset += px(params.word_spacing);
            }
        }

        Ok((vertices, indices))
    }

    /// Check if vector rendering is needed based on transform
    ///
    /// Returns true if the transform has skew, perspective, or non-uniform
    /// scale
    #[must_use]
    pub fn needs_vector_rendering(transform: &Mat4) -> bool {
        let m = transform.to_cols_array_2d();

        let m00 = m[0][0];
        let m01 = m[0][1];
        let m10 = m[1][0];
        let m11 = m[1][1];

        let dot = m00 * m10 + m01 * m11;

        // Check for skew
        let has_skew = dot.abs() > 0.01;

        // Check for perspective
        let has_perspective = m[0][3].abs() > 0.001 || m[1][3].abs() > 0.001;

        // Check for non-uniform scale
        let scale_x = (m00 * m00 + m01 * m01).sqrt();
        let scale_y = (m10 * m10 + m11 * m11).sqrt();
        let scale_ratio = if scale_x > scale_y {
            scale_x / scale_y
        } else {
            scale_y / scale_x
        };
        let has_nonuniform_scale = scale_ratio > 1.05;

        has_skew || has_perspective || has_nonuniform_scale
    }
}

/// Converts ttf-parser outline commands to lyon paths
struct LyonOutlineBuilder<'a> {
    builder: &'a mut lyon::path::Builder,
    scale: f32,
    offset_x: f32,
    offset_y: f32,
}

impl ttf_parser::OutlineBuilder for LyonOutlineBuilder<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let px = x * self.scale + self.offset_x;
        let py = -y * self.scale + self.offset_y; // Flip Y
        self.builder.begin(lyon::math::point(px, py));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let px = x * self.scale + self.offset_x;
        let py = -y * self.scale + self.offset_y;
        self.builder.line_to(lyon::math::point(px, py));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let px1 = x1 * self.scale + self.offset_x;
        let py1 = -y1 * self.scale + self.offset_y;
        let px = x * self.scale + self.offset_x;
        let py = -y * self.scale + self.offset_y;
        self.builder
            .quadratic_bezier_to(lyon::math::point(px1, py1), lyon::math::point(px, py));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let px1 = x1 * self.scale + self.offset_x;
        let py1 = -y1 * self.scale + self.offset_y;
        let px2 = x2 * self.scale + self.offset_x;
        let py2 = -y2 * self.scale + self.offset_y;
        let px = x * self.scale + self.offset_x;
        let py = -y * self.scale + self.offset_y;
        self.builder.cubic_bezier_to(
            lyon::math::point(px1, py1),
            lyon::math::point(px2, py2),
            lyon::math::point(px, py),
        );
    }

    fn close(&mut self) {
        self.builder.end(true);
    }
}

/// Errors that can occur during vector text rendering
#[derive(Debug, Error)]
pub enum VectorTextError {
    /// Font data is invalid or corrupted
    #[error("Invalid font data")]
    InvalidFont,

    /// Character not found in font
    #[error("Glyph not found for character '{0}'")]
    GlyphNotFound(char),

    /// Tessellation failed
    #[error("Path tessellation failed")]
    TessellationFailed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    /// Load the bundled Roboto font for testing.
    fn load_test_font() -> Vec<u8> {
        include_bytes!("../../assets/fonts/Roboto-Regular.ttf").to_vec()
    }

    /// Helper to create default render params with identity transform.
    fn default_params<'a>(
        text: &'a str,
        font_size: f32,
        transform: &'a Mat4,
    ) -> TextRenderParams<'a> {
        TextRenderParams::new(
            text,
            Point::new(px(0.0), px(0.0)),
            font_size,
            Color::BLACK,
            transform,
        )
    }

    // ===== Construction Tests =====

    #[test]
    fn create_renderer_with_valid_font() {
        let renderer = VectorTextRenderer::new(load_test_font());
        assert!(
            !renderer.font_data().is_empty(),
            "font data should not be empty after construction"
        );
    }

    #[test]
    fn load_font_replaces_data() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let original_len = renderer.font_data().len();

        // Load a different (same) font to verify the method works
        let new_data = load_test_font();
        renderer.load_font(new_data.clone());
        assert_eq!(
            renderer.font_data().len(),
            original_len,
            "font data length should match after reload"
        );
    }

    // ===== Render: Basic ASCII =====

    #[test]
    fn render_basic_ascii_produces_geometry() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let identity = Mat4::IDENTITY;
        let params = default_params("Hello", 24.0, &identity);

        let (vertices, indices) = renderer
            .render(&params)
            .expect("rendering basic ASCII should succeed");

        assert!(
            !vertices.is_empty(),
            "vertices should not be empty for visible text"
        );
        assert!(
            !indices.is_empty(),
            "indices should not be empty for visible text"
        );
        // Indices must be valid triangle list (multiple of 3)
        assert_eq!(
            indices.len() % 3,
            0,
            "index count should be a multiple of 3 for triangle list"
        );
    }

    #[test]
    fn render_single_character() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let identity = Mat4::IDENTITY;
        let params = default_params("A", 16.0, &identity);

        let (vertices, indices) = renderer
            .render(&params)
            .expect("rendering single character should succeed");

        assert!(!vertices.is_empty(), "single visible glyph should produce vertices");
        assert!(!indices.is_empty(), "single visible glyph should produce indices");
    }

    // ===== Render: Empty and Whitespace =====

    #[test]
    fn render_empty_string_produces_no_geometry() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let identity = Mat4::IDENTITY;
        let params = default_params("", 24.0, &identity);

        let (vertices, indices) = renderer
            .render(&params)
            .expect("rendering empty string should succeed");

        assert!(vertices.is_empty(), "empty string should produce no vertices");
        assert!(indices.is_empty(), "empty string should produce no indices");
    }

    #[test]
    fn render_spaces_only_produces_no_geometry() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let identity = Mat4::IDENTITY;
        let params = default_params("   ", 24.0, &identity);

        let (vertices, indices) = renderer
            .render(&params)
            .expect("rendering spaces should succeed");

        assert!(
            vertices.is_empty(),
            "space-only string should produce no visible geometry"
        );
        assert!(
            indices.is_empty(),
            "space-only string should produce no visible indices"
        );
    }

    // ===== Render: Font Size Variations =====

    #[test]
    fn larger_font_size_produces_larger_extents() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let identity = Mat4::IDENTITY;

        let params_small = default_params("W", 12.0, &identity);
        let (verts_small, _) = renderer
            .render(&params_small)
            .expect("small font render should succeed");

        let params_large = default_params("W", 48.0, &identity);
        let (verts_large, _) = renderer
            .render(&params_large)
            .expect("large font render should succeed");

        // Compute bounding box width for each
        let extent = |verts: &[TextVertex]| -> f32 {
            let xs: Vec<f32> = verts.iter().map(|v| v.x).collect();
            let min = xs.iter().copied().reduce(f32::min).unwrap_or(0.0);
            let max = xs.iter().copied().reduce(f32::max).unwrap_or(0.0);
            max - min
        };

        assert!(
            extent(&verts_large) > extent(&verts_small),
            "larger font size should produce wider glyph geometry"
        );
    }

    #[test]
    fn more_characters_produce_more_vertices() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let identity = Mat4::IDENTITY;

        let params_one = default_params("A", 24.0, &identity);
        let (v1, _) = renderer
            .render(&params_one)
            .expect("single char render should succeed");

        let params_three = default_params("ABC", 24.0, &identity);
        let (v3, _) = renderer
            .render(&params_three)
            .expect("three char render should succeed");

        assert!(
            v3.len() > v1.len(),
            "more characters should produce more vertices"
        );
    }

    // ===== Render: Vertex Color =====

    #[test]
    fn vertices_carry_specified_color() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let identity = Mat4::IDENTITY;
        let color = Color::rgba(255, 0, 0, 255);
        let params = TextRenderParams::new(
            "R",
            Point::new(px(0.0), px(0.0)),
            24.0,
            color,
            &identity,
        );

        let (vertices, _) = renderer
            .render(&params)
            .expect("colored render should succeed");

        for v in &vertices {
            assert_eq!(v.color, color, "every vertex should carry the specified color");
        }
    }

    // ===== Render: Position Offset =====

    #[test]
    fn position_offset_shifts_vertices() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let identity = Mat4::IDENTITY;

        let params_origin = default_params("T", 24.0, &identity);
        let (verts_origin, _) = renderer
            .render(&params_origin)
            .expect("origin render should succeed");

        let offset_x = 100.0_f32;
        let params_offset = TextRenderParams::new(
            "T",
            Point::new(px(offset_x), px(0.0)),
            24.0,
            Color::BLACK,
            &identity,
        );
        let (verts_offset, _) = renderer
            .render(&params_offset)
            .expect("offset render should succeed");

        let avg_x = |verts: &[TextVertex]| -> f32 {
            verts.iter().map(|v| v.x).sum::<f32>() / verts.len() as f32
        };

        let diff = avg_x(&verts_offset) - avg_x(&verts_origin);
        assert!(
            (diff - offset_x).abs() < 1.0,
            "x position offset should shift vertices by approximately {offset_x}, got diff={diff}"
        );
    }

    // ===== Render: Letter and Word Spacing =====

    #[test]
    fn letter_spacing_increases_total_width() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let identity = Mat4::IDENTITY;

        let params_normal = default_params("AB", 24.0, &identity);
        let (verts_normal, _) = renderer
            .render(&params_normal)
            .expect("normal spacing render should succeed");

        let params_spaced = TextRenderParams::new(
            "AB",
            Point::new(px(0.0), px(0.0)),
            24.0,
            Color::BLACK,
            &identity,
        )
        .with_letter_spacing(20.0);
        let (verts_spaced, _) = renderer
            .render(&params_spaced)
            .expect("letter-spaced render should succeed");

        let width = |verts: &[TextVertex]| -> f32 {
            let xs: Vec<f32> = verts.iter().map(|v| v.x).collect();
            xs.iter().copied().reduce(f32::max).unwrap_or(0.0)
                - xs.iter().copied().reduce(f32::min).unwrap_or(0.0)
        };

        assert!(
            width(&verts_spaced) > width(&verts_normal),
            "letter spacing should increase total rendered width"
        );
    }

    #[test]
    fn word_spacing_increases_width_for_text_with_spaces() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let identity = Mat4::IDENTITY;

        let params_normal = default_params("A B", 24.0, &identity);
        let (verts_normal, _) = renderer
            .render(&params_normal)
            .expect("normal word spacing render should succeed");

        let params_spaced = TextRenderParams::new(
            "A B",
            Point::new(px(0.0), px(0.0)),
            24.0,
            Color::BLACK,
            &identity,
        )
        .with_word_spacing(50.0);
        let (verts_spaced, _) = renderer
            .render(&params_spaced)
            .expect("word-spaced render should succeed");

        let max_x = |verts: &[TextVertex]| -> f32 {
            verts.iter().map(|v| v.x).fold(f32::NEG_INFINITY, f32::max)
        };

        assert!(
            max_x(&verts_spaced) > max_x(&verts_normal),
            "word spacing should push later glyphs further right"
        );
    }

    // ===== Render: Transform =====

    #[test]
    fn scale_transform_affects_vertex_positions() {
        let mut renderer = VectorTextRenderer::new(load_test_font());
        let identity = Mat4::IDENTITY;
        let scale_2x = Mat4::from_scale(glam::Vec3::new(2.0, 2.0, 1.0));

        let params_identity = default_params("X", 24.0, &identity);
        let (verts_id, _) = renderer
            .render(&params_identity)
            .expect("identity render should succeed");

        let params_scaled = default_params("X", 24.0, &scale_2x);
        let (verts_sc, _) = renderer
            .render(&params_scaled)
            .expect("scaled render should succeed");

        let extent_x = |verts: &[TextVertex]| -> f32 {
            let xs: Vec<f32> = verts.iter().map(|v| v.x).collect();
            xs.iter().copied().reduce(f32::max).unwrap_or(0.0)
                - xs.iter().copied().reduce(f32::min).unwrap_or(0.0)
        };

        // With 2x scale transform, extent should roughly double
        let ratio = extent_x(&verts_sc) / extent_x(&verts_id);
        assert!(
            (ratio - 2.0).abs() < 0.5,
            "2x scale transform should roughly double glyph extent, got ratio={ratio}"
        );
    }

    // ===== Error Handling =====

    #[test]
    fn invalid_font_data_returns_error() {
        let mut renderer = VectorTextRenderer::new(vec![0, 1, 2, 3]);
        let identity = Mat4::IDENTITY;
        let params = default_params("A", 24.0, &identity);

        let result = renderer.render(&params);
        assert!(
            result.is_err(),
            "invalid font data should return an error"
        );
        assert!(
            matches!(result, Err(VectorTextError::InvalidFont)),
            "error should be InvalidFont variant"
        );
    }

    // ===== needs_vector_rendering =====

    #[test]
    fn identity_does_not_need_vector_rendering() {
        assert!(
            !VectorTextRenderer::needs_vector_rendering(&Mat4::IDENTITY),
            "identity transform should not require vector rendering"
        );
    }

    #[test]
    fn uniform_scale_does_not_need_vector_rendering() {
        let uniform = Mat4::from_scale(glam::Vec3::new(3.0, 3.0, 1.0));
        assert!(
            !VectorTextRenderer::needs_vector_rendering(&uniform),
            "uniform scale should not require vector rendering"
        );
    }

    #[test]
    fn non_uniform_scale_needs_vector_rendering() {
        let non_uniform = Mat4::from_scale(glam::Vec3::new(2.0, 1.0, 1.0));
        assert!(
            VectorTextRenderer::needs_vector_rendering(&non_uniform),
            "non-uniform scale should require vector rendering"
        );
    }

    #[test]
    fn skew_transform_needs_vector_rendering() {
        // Create a skew matrix by modifying the identity
        let mut skew = Mat4::IDENTITY;
        let cols = skew.to_cols_array_2d();
        let mut m = cols;
        m[1][0] = 0.5; // skew X by Y
        skew = Mat4::from_cols_array_2d(&m);

        assert!(
            VectorTextRenderer::needs_vector_rendering(&skew),
            "skew transform should require vector rendering"
        );
    }

    #[test]
    fn perspective_transform_needs_vector_rendering() {
        let mut perspective = Mat4::IDENTITY;
        let mut m = perspective.to_cols_array_2d();
        m[0][3] = 0.01; // perspective component
        perspective = Mat4::from_cols_array_2d(&m);

        assert!(
            VectorTextRenderer::needs_vector_rendering(&perspective),
            "perspective transform should require vector rendering"
        );
    }

    #[test]
    fn pure_translation_does_not_need_vector_rendering() {
        let translation = Mat4::from_translation(glam::Vec3::new(100.0, 200.0, 0.0));
        assert!(
            !VectorTextRenderer::needs_vector_rendering(&translation),
            "pure translation should not require vector rendering"
        );
    }

    #[test]
    fn pure_rotation_does_not_need_vector_rendering() {
        // Pure rotation is uniform scale with zero skew - raster is fine
        let rotation = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_4);
        assert!(
            !VectorTextRenderer::needs_vector_rendering(&rotation),
            "pure rotation (uniform scale, no skew) should not require vector rendering"
        );
    }

    #[test]
    fn rotation_with_non_uniform_scale_needs_vector_rendering() {
        let rotation = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_4);
        let non_uniform = Mat4::from_scale(glam::Vec3::new(2.0, 1.0, 1.0));
        let combined = rotation * non_uniform;
        assert!(
            VectorTextRenderer::needs_vector_rendering(&combined),
            "rotation combined with non-uniform scale should require vector rendering"
        );
    }

    // ===== TextRenderParams Builder =====

    #[test]
    fn text_render_params_builder_methods() {
        let identity = Mat4::IDENTITY;
        let params = TextRenderParams::new(
            "test",
            Point::new(px(10.0), px(20.0)),
            16.0,
            Color::WHITE,
            &identity,
        )
        .with_letter_spacing(2.0)
        .with_word_spacing(5.0);

        assert_eq!(params.text, "test");
        assert_eq!(params.font_size, 16.0);
        assert!((params.letter_spacing - 2.0).abs() < f32::EPSILON);
        assert!((params.word_spacing - 5.0).abs() < f32::EPSILON);
    }
}
