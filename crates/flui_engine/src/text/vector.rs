//! Backend-agnostic vector text rendering using ttf-parser and lyon
//!
//! This module provides vector-based text rendering that supports full matrix
//! transformations including skew and perspective. It converts font glyphs to
//! vector paths and tessellates them using Lyon.
//!
//! This is slower than raster text but supports arbitrary transformations.

use flui_types::{Color, Point};
use glam::Mat4;
use std::sync::Arc;

/// Simple 2D vertex with position and color (backend-agnostic)
#[derive(Debug, Clone, Copy)]
pub struct TextVertex {
    pub x: f32,
    pub y: f32,
    pub color: Color,
}

/// Parameters for text rendering
#[derive(Debug, Clone)]
pub struct TextRenderParams<'a> {
    /// Text to render
    pub text: &'a str,
    /// Starting position
    pub position: Point,
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
    pub fn new(
        text: &'a str,
        position: Point,
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
pub struct VectorTextRenderer {
    /// Cached font face data
    font_data: Arc<Vec<u8>>,
    /// Lyon tessellator for converting paths to triangles
    fill_tessellator: lyon::tessellation::FillTessellator,
}

impl Default for VectorTextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorTextRenderer {
    /// Create a new vector text renderer with embedded Arial font
    pub fn new() -> Self {
        // Load embedded Arial font
        let font_bytes = include_bytes!("../../assets/fonts/Arial.ttf");
        let font_data = Arc::new(font_bytes.to_vec());

        Self {
            font_data,
            fill_tessellator: lyon::tessellation::FillTessellator::new(),
        }
    }

    /// Load font from bytes
    pub fn load_font(&mut self, font_data: Vec<u8>) {
        self.font_data = Arc::new(font_data);
    }

    /// Render text as vector paths and return backend-agnostic vertices and indices
    ///
    /// # Parameters
    /// - `params`: Text rendering parameters
    ///
    /// # Returns
    /// Tuple of (vertices, indices) that can be converted to any backend format
    pub fn render(
        &mut self,
        params: &TextRenderParams,
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
            let scale = params.font_size / face.units_per_em() as f32;

            // Create a new path builder for this glyph
            let mut path_builder = lyon::path::Builder::new();

            // Extract glyph outline in LOCAL space (no offsets yet)
            let mut outline_builder = LyonOutlineBuilder {
                builder: &mut path_builder,
                scale,
                offset_x: 0.0, // Build in local space
                offset_y: 0.0, // Build in local space
            };

            // Try to get outline - it's OK if whitespace characters don't have one
            let has_outline = face.outline_glyph(glyph_id, &mut outline_builder).is_some();

            if !has_outline {
                // No outline (e.g., space character) - skip rendering but still advance cursor
                if let Some(advance) = face.glyph_hor_advance(glyph_id) {
                    x_offset += advance as f32 * scale;
                }
                x_offset += params.letter_spacing;
                if ch.is_whitespace() {
                    x_offset += params.word_spacing;
                }
                continue;
            }

            // Build path
            let path = path_builder.build();

            // Tessellate path into triangles using Lyon's VertexBuffers
            let mut buffers: lyon::tessellation::VertexBuffers<lyon::math::Point, u16> =
                lyon::tessellation::VertexBuffers::new();

            self.fill_tessellator
                .tessellate_path(
                    &path,
                    &lyon::tessellation::FillOptions::default(),
                    &mut lyon::tessellation::BuffersBuilder::new(
                        &mut buffers,
                        |vertex: lyon::tessellation::FillVertex| vertex.position(),
                    ),
                )
                .map_err(|_| VectorTextError::TessellationFailed)?;

            // Convert Lyon vertices to backend-agnostic vertices with transform applied
            let vertex_offset = vertices.len() as u32;

            for point in buffers.vertices.iter() {
                // Place character at its offset position (relative to start of string)
                // Then the transform matrix will be applied to the whole positioned text
                let local_x = point.x + x_offset;
                let local_y = point.y + params.position.y;

                // Apply full 4x4 transformation with perspective division
                let m = params.transform.to_cols_array_2d();

                // Full 4x4 matrix multiplication (treating 2D point as 3D with z=0)
                let x = m[0][0] * local_x + m[1][0] * local_y + m[2][0] * 0.0 + m[3][0];
                let y = m[0][1] * local_x + m[1][1] * local_y + m[2][1] * 0.0 + m[3][1];
                let w = m[0][3] * local_x + m[1][3] * local_y + m[2][3] * 0.0 + m[3][3];

                // Perspective division - if w is close to 1.0, skip division for performance
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

            // Add indices with offset for this glyph
            for index in buffers.indices.iter() {
                indices.push(vertex_offset + (*index as u32));
            }

            // Advance cursor
            if let Some(advance) = face.glyph_hor_advance(glyph_id) {
                x_offset += advance as f32 * scale;
            }

            // Apply letter spacing after each character
            x_offset += params.letter_spacing;

            // Apply word spacing after space characters
            if ch.is_whitespace() {
                x_offset += params.word_spacing;
            }
        }

        Ok((vertices, indices))
    }

    /// Check if vector rendering is needed based on transform
    pub fn needs_vector_rendering(transform: &Mat4) -> bool {
        // Extract matrix components
        let m = transform.to_cols_array_2d();

        // Check for skew (non-orthogonal)
        let m00 = m[0][0];
        let m01 = m[0][1];
        let m10 = m[1][0];
        let m11 = m[1][1];

        let dot = m00 * m10 + m01 * m11;

        // Use vector rendering if:
        // 1. Has skew (dot product > threshold)
        // 2. Has perspective (w-component affected: m[0][3] or m[1][3] != 0)
        // 3. Has non-uniform scale (scale ratio > threshold)
        let has_skew = dot.abs() > 0.01;
        let has_perspective = m[0][3].abs() > 0.001 || m[1][3].abs() > 0.001;

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
        let py = -y * self.scale + self.offset_y; // Flip Y (font coords are inverted)
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
#[derive(Debug)]
pub enum VectorTextError {
    /// Font data is invalid or corrupted
    InvalidFont,
    /// Character not found in font
    GlyphNotFound(char),
    /// Glyph has no outline (e.g., space character)
    NoOutline(char),
    /// Tessellation failed
    TessellationFailed,
}

impl std::fmt::Display for VectorTextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VectorTextError::InvalidFont => write!(f, "Invalid font data"),
            VectorTextError::GlyphNotFound(ch) => {
                write!(f, "Glyph not found for character '{}'", ch)
            }
            VectorTextError::NoOutline(ch) => write!(f, "No outline for character '{}'", ch),
            VectorTextError::TessellationFailed => write!(f, "Path tessellation failed"),
        }
    }
}

impl std::error::Error for VectorTextError {}
