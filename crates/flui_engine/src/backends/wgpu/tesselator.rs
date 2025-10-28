//! Geometry tesselation - converts shapes to GPU triangles

use super::vertex::SolidVertex;
use crate::painter::RRect;
use flui_types::{Point, Rect};
use glam::{Mat4, Vec3};

/// Geometry tesselator
///
/// Converts high-level shapes (rect, circle, rrect, line) into GPU triangles.
/// Applies transform matrix during tesselation for efficient batching.
pub struct Tesselator {
    /// Vertex buffer (accumulated triangles)
    pub vertices: Vec<SolidVertex>,

    /// Index buffer (triangle indices)
    pub indices: Vec<u32>,

    /// Quality setting for circles (segments per circle)
    circle_segments: u32,
}

impl Tesselator {
    /// Create a new tesselator
    ///
    /// # Arguments
    /// * `circle_segments` - Number of segments for circle tesselation (16-64 recommended)
    pub fn new(circle_segments: u32) -> Self {
        Self {
            vertices: Vec::with_capacity(1024),
            indices: Vec::with_capacity(2048),
            circle_segments,
        }
    }

    /// Tesselate a filled rectangle
    ///
    /// # Arguments
    /// * `rect` - Rectangle bounds
    /// * `color` - RGBA color (premultiplied alpha)
    /// * `transform` - Transform matrix to apply
    pub fn tesselate_rect(&mut self, rect: Rect, color: [f32; 4], transform: Mat4) {
        let base_index = self.vertices.len() as u32;

        // Four corners
        let corners = [
            [rect.min.x, rect.min.y], // Top-left
            [rect.max.x, rect.min.y], // Top-right
            [rect.max.x, rect.max.y], // Bottom-right
            [rect.min.x, rect.max.y], // Bottom-left
        ];

        // Transform and add vertices
        for &[x, y] in &corners {
            let pos = transform.transform_point3(Vec3::new(x, y, 0.0));
            self.vertices.push(SolidVertex::new(pos.x, pos.y, color));
        }

        // Two triangles (CCW winding)
        self.indices.extend_from_slice(&[
            base_index,
            base_index + 1,
            base_index + 2, // Triangle 1
            base_index,
            base_index + 2,
            base_index + 3, // Triangle 2
        ]);
    }

    /// Tesselate a filled circle
    ///
    /// # Arguments
    /// * `center` - Circle center
    /// * `radius` - Circle radius
    /// * `color` - RGBA color (premultiplied alpha)
    /// * `transform` - Transform matrix to apply
    pub fn tesselate_circle(
        &mut self,
        center: Point,
        radius: f32,
        color: [f32; 4],
        transform: Mat4,
    ) {
        let base_index = self.vertices.len() as u32;
        let segments = self.circle_segments;

        // Center vertex
        let center_pos = transform.transform_point3(Vec3::new(center.x, center.y, 0.0));
        self.vertices
            .push(SolidVertex::new(center_pos.x, center_pos.y, color));

        // Circle perimeter vertices
        for i in 0..=segments {
            let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let x = center.x + radius * angle.cos();
            let y = center.y + radius * angle.sin();

            let pos = transform.transform_point3(Vec3::new(x, y, 0.0));
            self.vertices.push(SolidVertex::new(pos.x, pos.y, color));
        }

        // Triangle fan from center
        for i in 0..segments {
            self.indices.extend_from_slice(&[
                base_index,         // Center
                base_index + i + 1, // Current perimeter vertex
                base_index + i + 2, // Next perimeter vertex
            ]);
        }
    }

    /// Tesselate a rounded rectangle
    ///
    /// # Arguments
    /// * `rrect` - Rounded rectangle
    /// * `color` - RGBA color (premultiplied alpha)
    /// * `transform` - Transform matrix to apply
    pub fn tesselate_rrect(&mut self, rrect: RRect, color: [f32; 4], transform: Mat4) {
        let rect = rrect.rect;
        let radius = rrect.corner_radius.min(rect.width() * 0.5).min(rect.height() * 0.5);

        if radius <= 0.0 {
            // Degenerate to regular rect
            self.tesselate_rect(rect, color, transform);
            return;
        }

        let segments_per_corner = 8; // 8 segments per corner = 32 total for smooth corners
        let base_index = self.vertices.len() as u32;

        // Center point (for triangle fan)
        let center = rect.center();
        let center_pos = transform.transform_point3(Vec3::new(center.x, center.y, 0.0));
        self.vertices
            .push(SolidVertex::new(center_pos.x, center_pos.y, color));

        // Helper to add corner arc
        let mut add_corner_arc = |corner_center: Point, start_angle: f32| {
            for i in 0..=segments_per_corner {
                let t = i as f32 / segments_per_corner as f32;
                let angle = start_angle + t * std::f32::consts::FRAC_PI_2;
                let x = corner_center.x + radius * angle.cos();
                let y = corner_center.y + radius * angle.sin();

                let pos = transform.transform_point3(Vec3::new(x, y, 0.0));
                self.vertices.push(SolidVertex::new(pos.x, pos.y, color));
            }
        };

        // Four corner arcs (clockwise from top-left)
        add_corner_arc(
            Point::new(rect.min.x + radius, rect.min.y + radius),
            std::f32::consts::PI,
        ); // Top-left
        add_corner_arc(
            Point::new(rect.max.x - radius, rect.min.y + radius),
            -std::f32::consts::FRAC_PI_2,
        ); // Top-right
        add_corner_arc(
            Point::new(rect.max.x - radius, rect.max.y - radius),
            0.0,
        ); // Bottom-right
        add_corner_arc(
            Point::new(rect.min.x + radius, rect.max.y - radius),
            std::f32::consts::FRAC_PI_2,
        ); // Bottom-left

        // Triangle fan from center to perimeter
        let perimeter_vertices = (segments_per_corner + 1) * 4;
        for i in 0..perimeter_vertices {
            let next = (i + 1) % perimeter_vertices;
            self.indices.extend_from_slice(&[
                base_index,         // Center
                base_index + i + 1, // Current perimeter vertex
                base_index + next + 1, // Next perimeter vertex
            ]);
        }
    }

    /// Tesselate a line (expanded to quad)
    ///
    /// # Arguments
    /// * `p1` - Start point
    /// * `p2` - End point
    /// * `width` - Line width in pixels
    /// * `color` - RGBA color (premultiplied alpha)
    /// * `transform` - Transform matrix to apply
    pub fn tesselate_line(
        &mut self,
        p1: Point,
        p2: Point,
        width: f32,
        color: [f32; 4],
        transform: Mat4,
    ) {
        let base_index = self.vertices.len() as u32;

        // Calculate perpendicular vector for line width
        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;
        let len = (dx * dx + dy * dy).sqrt();

        if len < 0.001 {
            // Degenerate line - draw a point as small rect
            self.tesselate_rect(
                Rect::from_center_size(
                    p1,
                    flui_types::Size::new(width, width),
                ),
                color,
                transform,
            );
            return;
        }

        let nx = -dy / len * width * 0.5; // Perpendicular X
        let ny = dx / len * width * 0.5; // Perpendicular Y

        // Four corners of line quad
        let corners = [
            [p1.x + nx, p1.y + ny], // Start top
            [p1.x - nx, p1.y - ny], // Start bottom
            [p2.x - nx, p2.y - ny], // End bottom
            [p2.x + nx, p2.y + ny], // End top
        ];

        // Transform and add vertices
        for &[x, y] in &corners {
            let pos = transform.transform_point3(Vec3::new(x, y, 0.0));
            self.vertices.push(SolidVertex::new(pos.x, pos.y, color));
        }

        // Two triangles
        self.indices.extend_from_slice(&[
            base_index,
            base_index + 1,
            base_index + 2,
            base_index,
            base_index + 2,
            base_index + 3,
        ]);
    }

    /// Clear all geometry
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }

    /// Get current vertex count
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Get current index count
    pub fn index_count(&self) -> usize {
        self.indices.len()
    }
}
