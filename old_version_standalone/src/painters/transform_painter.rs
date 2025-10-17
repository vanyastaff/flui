//! Transform painter - renders widgets with TRS transformations.
//!
//! This module provides utilities for rendering UI elements with transformations
//! applied using egui's Mesh API.

use crate::types::core::{Transform, Color};
use crate::types::styling::BoxDecoration;
use egui::epaint::{Mesh, Color32, Vertex, Pos2};
use egui::emath::Rot2;
use egui::{Painter, Rect, TextureId};

/// Utility for painting transformed decorations.
///
/// Applies TRS (Translate-Rotate-Scale) transformations to decoration meshes,
/// similar to Flutter's Matrix4 transform system.
pub struct TransformPainter;

impl TransformPainter {
    /// Paint a decoration with full TRS transformation applied.
    ///
    /// # Arguments
    ///
    /// * `painter` - The egui painter to draw with
    /// * `rect` - The rectangle to paint (before transformation)
    /// * `origin` - The pivot point for transformation
    /// * `transform` - The TRS transformation to apply
    /// * `decoration` - The box decoration to paint
    ///
    /// # Transformation Order
    ///
    /// Transformations are applied in TRS order (like Flutter's Matrix4):
    /// 1. Scale - around the origin point
    /// 2. Rotate - around the origin point
    /// 3. Translate - move to final position
    ///
    /// # Limitations
    ///
    /// Currently only renders the background color. Borders, shadows, and
    /// rounded corners are not yet transformed.
    pub fn paint_transformed_decoration(
        painter: &Painter,
        rect: Rect,
        origin: Pos2,
        transform: &Transform,
        decoration: &BoxDecoration,
    ) {
        // Extract decoration color
        let color = decoration.color
            .map(|c| Self::convert_color(c))
            .unwrap_or(Color32::TRANSPARENT);

        // Create and transform the mesh
        let mesh = Self::create_transformed_quad(rect, origin, transform, color);

        // Paint to screen
        painter.add(mesh);

        // TODO: Add support for:
        // - Borders (transform border mesh)
        // - Box shadows (transform shadow mesh)
        // - Rounded corners (transform rounded mesh)
        // - Gradients (transform gradient mesh)
    }

    /// Create a quad mesh with TRS transformation applied.
    ///
    /// # Arguments
    ///
    /// * `rect` - The rectangle bounds (before transformation)
    /// * `origin` - The pivot point for transformation
    /// * `transform` - The TRS transformation to apply
    /// * `color` - The fill color for the quad
    ///
    /// # Returns
    ///
    /// A transformed mesh ready to be added to the painter.
    pub fn create_transformed_quad(
        rect: Rect,
        origin: Pos2,
        transform: &Transform,
        color: Color32,
    ) -> Mesh {
        let mut mesh = Mesh::default();
        mesh.texture_id = TextureId::default();

        // Define the four corners RELATIVE to origin
        // This allows transformations to be applied around the origin point
        let tl = Pos2::new(rect.left() - origin.x, rect.top() - origin.y);
        let tr = Pos2::new(rect.right() - origin.x, rect.top() - origin.y);
        let bl = Pos2::new(rect.left() - origin.x, rect.bottom() - origin.y);
        let br = Pos2::new(rect.right() - origin.x, rect.bottom() - origin.y);

        // Create vertices for the quad
        mesh.vertices = vec![
            Vertex { pos: tl, uv: Pos2::ZERO, color },
            Vertex { pos: tr, uv: Pos2::ZERO, color },
            Vertex { pos: br, uv: Pos2::ZERO, color },
            Vertex { pos: bl, uv: Pos2::ZERO, color },
        ];

        // Create triangle indices (two triangles make a quad)
        mesh.indices = vec![0, 1, 2, 0, 2, 3];

        // Apply TRS transformations in order

        // 1. Scale (around origin at 0,0 in relative coordinates)
        if transform.scale.x != 1.0 || transform.scale.y != 1.0 {
            Self::apply_scale(&mut mesh, transform.scale.x, transform.scale.y);
        }

        // 2. Rotate (around origin at 0,0)
        if transform.rotation != 0.0 {
            Self::apply_rotation(&mut mesh, transform.rotation);
        }

        // 3. Translate (move to final position)
        let final_translation = egui::vec2(
            origin.x + transform.translation.dx,
            origin.y + transform.translation.dy,
        );
        mesh.translate(final_translation);

        mesh
    }

    /// Apply scale transformation to mesh vertices.
    ///
    /// Scales each vertex position by the given factors.
    #[inline]
    fn apply_scale(mesh: &mut Mesh, scale_x: f32, scale_y: f32) {
        for vertex in &mut mesh.vertices {
            vertex.pos.x *= scale_x;
            vertex.pos.y *= scale_y;
        }
    }

    /// Apply rotation transformation to mesh.
    ///
    /// Rotates the mesh around the origin (0, 0) by the given angle.
    #[inline]
    fn apply_rotation(mesh: &mut Mesh, radians: f32) {
        let rot = Rot2::from_angle(radians);
        mesh.rotate(rot, Pos2::ZERO);
    }

    /// Convert nebula Color to egui Color32.
    #[inline]
    fn convert_color(color: Color) -> Color32 {
        Color32::from_rgba_premultiplied(
            color.red(),
            color.green(),
            color.blue(),
            color.alpha(),
        )
    }

    /// Paint a simple colored quad with transformation.
    ///
    /// Convenience method for painting a solid color rectangle with transform.
    pub fn paint_transformed_quad(
        painter: &Painter,
        rect: Rect,
        origin: Pos2,
        transform: &Transform,
        color: Color,
    ) {
        let egui_color = Self::convert_color(color);
        let mesh = Self::create_transformed_quad(rect, origin, transform, egui_color);
        painter.add(mesh);
    }

    /// Check if a transform requires visual rendering.
    ///
    /// Returns true if the transform is NOT identity (i.e., it actually transforms something).
    #[inline]
    pub fn should_apply_transform(transform: &Transform) -> bool {
        !transform.is_identity()
    }

    /// Transform a shape (mesh) in-place.
    ///
    /// Applies TRS transformation to all vertices in the shape.
    /// This is used to transform child widget shapes.
    pub fn transform_shape(
        shape: &mut egui::Shape,
        origin: Pos2,
        transform: &Transform,
    ) {
        use egui::Shape;

        match shape {
            Shape::Noop => {},
            Shape::Vec(shapes) => {
                // Recursively transform all shapes in the vector
                for s in shapes {
                    Self::transform_shape(s, origin, transform);
                }
            },
            Shape::Circle(circle_shape) => {
                // Transform circle center
                let center = Self::transform_point(circle_shape.center, origin, transform);
                circle_shape.center = center;
                // Note: radius could be scaled, but that gets complex with non-uniform scaling
            },
            Shape::Ellipse(ellipse_shape) => {
                // Transform ellipse center
                let center = Self::transform_point(ellipse_shape.center, origin, transform);
                ellipse_shape.center = center;
                // Note: radius could be scaled
            },
            Shape::LineSegment { points, stroke } => {
                // Transform line endpoints
                points[0] = Self::transform_point(points[0], origin, transform);
                points[1] = Self::transform_point(points[1], origin, transform);
                // Note: stroke width could be scaled
                let _ = stroke; // Keep stroke as-is for now
            },
            Shape::Path(path_shape) => {
                // Transform all points in the path
                for point in &mut path_shape.points {
                    *point = Self::transform_point(*point, origin, transform);
                }
            },
            Shape::Rect(rect_shape) => {
                // Transform rectangle - this gets complex with rotation
                // For now, we'll transform the corners and approximate
                let rect = rect_shape.rect;
                let tl = Self::transform_point(rect.left_top(), origin, transform);
                let tr = Self::transform_point(rect.right_top(), origin, transform);
                let bl = Self::transform_point(rect.left_bottom(), origin, transform);
                let br = Self::transform_point(rect.right_bottom(), origin, transform);

                // After rotation, rect might not be axis-aligned anymore
                // We need to find the bounding box
                let min_x = tl.x.min(tr.x).min(bl.x).min(br.x);
                let max_x = tl.x.max(tr.x).max(bl.x).max(br.x);
                let min_y = tl.y.min(tr.y).min(bl.y).min(br.y);
                let max_y = tl.y.max(tr.y).max(bl.y).max(br.y);

                rect_shape.rect = egui::Rect::from_min_max(
                    Pos2::new(min_x, min_y),
                    Pos2::new(max_x, max_y),
                );
            },
            Shape::Text(text_shape) => {
                // Transform text position
                text_shape.pos = Self::transform_point(text_shape.pos, origin, transform);

                // Add rotation angle to text
                if transform.rotation != 0.0 {
                    text_shape.angle += transform.rotation;
                }
            },
            Shape::Mesh(mesh_arc) => {
                // Transform all mesh vertices
                // Need to clone since Mesh is in an Arc
                let mesh = std::sync::Arc::make_mut(mesh_arc);
                Self::transform_mesh_vertices(mesh, origin, transform);
            },
            Shape::QuadraticBezier(bezier) => {
                // Transform bezier points
                for point in &mut bezier.points {
                    *point = Self::transform_point(*point, origin, transform);
                }
            },
            Shape::CubicBezier(bezier) => {
                // Transform bezier points
                for point in &mut bezier.points {
                    *point = Self::transform_point(*point, origin, transform);
                }
            },
            Shape::Callback(_) => {
                // Callbacks can't be transformed
            },
        }
    }

    /// Transform a single point using TRS transformation.
    fn transform_point(point: Pos2, origin: Pos2, transform: &Transform) -> Pos2 {
        // Convert to relative coordinates
        let mut p = Pos2::new(point.x - origin.x, point.y - origin.y);

        // 1. Scale
        p.x *= transform.scale.x;
        p.y *= transform.scale.y;

        // 2. Rotate
        if transform.rotation != 0.0 {
            let cos = transform.rotation.cos();
            let sin = transform.rotation.sin();
            let rotated_x = p.x * cos - p.y * sin;
            let rotated_y = p.x * sin + p.y * cos;
            p.x = rotated_x;
            p.y = rotated_y;
        }

        // 3. Translate
        Pos2::new(
            p.x + origin.x + transform.translation.dx,
            p.y + origin.y + transform.translation.dy,
        )
    }

    /// Transform all vertices in a mesh.
    fn transform_mesh_vertices(
        mesh: &mut egui::epaint::Mesh,
        origin: Pos2,
        transform: &Transform,
    ) {
        for vertex in &mut mesh.vertices {
            vertex.pos = Self::transform_point(vertex.pos, origin, transform);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::core::{Scale, Offset};

    #[test]
    fn test_create_transformed_quad_identity() {
        let rect = Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(50.0, 50.0));
        let origin = rect.center();
        let transform = Transform::IDENTITY;
        let color = Color32::RED;

        let mesh = TransformPainter::create_transformed_quad(rect, origin, &transform, color);

        // Identity transform should result in 4 vertices
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.indices.len(), 6);

        // All vertices should have the specified color
        for vertex in &mesh.vertices {
            assert_eq!(vertex.color, color);
        }
    }

    #[test]
    fn test_create_transformed_quad_with_scale() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(100.0, 100.0));
        let origin = rect.center();
        let transform = Transform::scale_uniform(2.0);
        let color = Color32::BLUE;

        let mesh = TransformPainter::create_transformed_quad(rect, origin, &transform, color);

        assert_eq!(mesh.vertices.len(), 4);
        // Mesh should be scaled, but we can't easily test exact positions
        // since they're in relative coordinates then translated
    }

    #[test]
    fn test_create_transformed_quad_with_rotation() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(100.0, 50.0));
        let origin = rect.center();
        let transform = Transform::rotate_degrees(90.0);
        let color = Color32::GREEN;

        let mesh = TransformPainter::create_transformed_quad(rect, origin, &transform, color);

        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.indices.len(), 6);
    }

    #[test]
    fn test_should_apply_transform() {
        assert!(!TransformPainter::should_apply_transform(&Transform::IDENTITY));
        assert!(TransformPainter::should_apply_transform(&Transform::rotate_degrees(45.0)));
        assert!(TransformPainter::should_apply_transform(&Transform::scale_uniform(2.0)));
        assert!(TransformPainter::should_apply_transform(&Transform::translate(10.0, 20.0)));
    }

    #[test]
    fn test_apply_scale() {
        let mut mesh = Mesh::default();
        mesh.vertices = vec![
            Vertex { pos: Pos2::new(10.0, 20.0), uv: Pos2::ZERO, color: Color32::WHITE },
            Vertex { pos: Pos2::new(30.0, 40.0), uv: Pos2::ZERO, color: Color32::WHITE },
        ];

        TransformPainter::apply_scale(&mut mesh, 2.0, 3.0);

        assert_eq!(mesh.vertices[0].pos.x, 20.0);
        assert_eq!(mesh.vertices[0].pos.y, 60.0);
        assert_eq!(mesh.vertices[1].pos.x, 60.0);
        assert_eq!(mesh.vertices[1].pos.y, 120.0);
    }

    #[test]
    fn test_convert_color() {
        let nebula_color = Color::from_rgba(255, 128, 64, 200);
        let egui_color = TransformPainter::convert_color(nebula_color);

        assert_eq!(egui_color.r(), 255);
        assert_eq!(egui_color.g(), 128);
        assert_eq!(egui_color.b(), 64);
        assert_eq!(egui_color.a(), 200);
    }
}
