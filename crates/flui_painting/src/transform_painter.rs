//! TransformPainter - Painter wrapper that applies coordinate transformations
//!
//! This module provides a painter wrapper that intercepts all drawing calls
//! and transforms coordinates using glam::Affine2 before passing to the underlying painter.
//!
//! # Implementation Strategy
//!
//! Instead of trying to collect shapes (which requires Shape::Transform from newer egui),
//! we intercept drawing calls and transform coordinates directly using glam.
//!
//! This works with egui 0.33 and doesn't require architectural changes!

use egui::{Painter, Pos2, Rect, Shape};
use glam::{Affine2, Vec2};

/// A painter wrapper that transforms all coordinates
///
/// # How it works
///
/// TransformPainter wraps an egui::Painter and intercepts every drawing call.
/// All coordinates (positions, rectangles, etc.) are transformed using glam::Affine2
/// before being passed to the underlying painter.
///
/// # Example
///
/// ```rust,ignore
/// use glam::Affine2;
/// use flui_painting::TransformPainter;
///
/// // Create transformation: rotate 15 degrees around a point
/// let center = Vec2::new(200.0, 100.0);
/// let transform = Affine2::from_translation(-center)
///     * Affine2::from_angle(0.26)
///     * Affine2::from_translation(center);
///
/// let transforming = TransformPainter::new(painter, transform);
///
/// // All drawing is automatically transformed
/// ctx.paint_child(child_id, &transforming, offset);
/// ```
pub struct TransformPainter<'a> {
    /// The underlying egui painter
    painter: &'a Painter,

    /// The transformation to apply to all coordinates
    transform: Affine2,
}

impl<'a> TransformPainter<'a> {
    /// Create a new TransformPainter
    ///
    /// # Parameters
    ///
    /// - `painter`: The underlying egui painter
    /// - `transform`: The glam::Affine2 transformation matrix
    pub fn new(painter: &'a Painter, transform: Affine2) -> Self {
        Self { painter, transform }
    }

    /// Transform a single position
    #[inline]
    fn transform_pos(&self, pos: Pos2) -> Pos2 {
        let v = self.transform.transform_point2(Vec2::new(pos.x, pos.y));
        Pos2::new(v.x, v.y)
    }

    /// Transform a rectangle
    ///
    /// Note: For rotated rectangles, this transforms min/max corners.
    /// The result may not be axis-aligned.
    #[inline]
    fn transform_rect(&self, rect: Rect) -> Rect {
        let min = self.transform_pos(rect.min);
        let max = self.transform_pos(rect.max);
        Rect::from_min_max(min, max)
    }

    /// Add a shape with transformed coordinates
    pub fn add(&self, shape: Shape) {
        // Transform the shape coordinates
        let transformed = self.transform_shape(shape);
        self.painter.add(transformed);
    }

    /// Transform a shape's coordinates
    fn transform_shape(&self, shape: Shape) -> Shape {
        match shape {
            Shape::Noop => Shape::Noop,
            Shape::Vec(shapes) => {
                Shape::Vec(shapes.into_iter().map(|s| self.transform_shape(s)).collect())
            }
            Shape::Circle(mut circle) => {
                circle.center = self.transform_pos(circle.center);
                // Note: For non-uniform scaling, radius should be adjusted
                Shape::Circle(circle)
            }
            Shape::Ellipse(mut ellipse) => {
                ellipse.center = self.transform_pos(ellipse.center);
                // Note: For rotations, ellipse axes would need to be rotated
                Shape::Ellipse(ellipse)
            }
            Shape::LineSegment { points, stroke } => {
                Shape::LineSegment {
                    points: [self.transform_pos(points[0]), self.transform_pos(points[1])],
                    stroke,
                }
            }
            Shape::Path(mut path) => {
                path.points = path.points.iter().map(|&p| self.transform_pos(p)).collect();
                Shape::Path(path)
            }
            Shape::Rect(mut rect_shape) => {
                rect_shape.rect = self.transform_rect(rect_shape.rect);
                Shape::Rect(rect_shape)
            }
            Shape::Text(mut text) => {
                text.pos = self.transform_pos(text.pos);
                Shape::Text(text)
            }
            Shape::Mesh(mesh) => {
                // Mesh is Arc, need to clone to modify
                let mut mesh_clone = (*mesh).clone();
                // Transform all vertex positions
                for vertex in &mut mesh_clone.vertices {
                    vertex.pos = self.transform_pos(vertex.pos);
                }
                Shape::Mesh(mesh_clone.into())
            }
            Shape::QuadraticBezier(mut bezier) => {
                // Transform array manually (can't collect iterator into fixed-size array)
                for i in 0..bezier.points.len() {
                    bezier.points[i] = self.transform_pos(bezier.points[i]);
                }
                Shape::QuadraticBezier(bezier)
            }
            Shape::CubicBezier(mut bezier) => {
                // Transform array manually (can't collect iterator into fixed-size array)
                for i in 0..bezier.points.len() {
                    bezier.points[i] = self.transform_pos(bezier.points[i]);
                }
                Shape::CubicBezier(bezier)
            }
            Shape::Callback(callback) => {
                // Can't transform callbacks - pass through
                tracing::warn!("TransformPainter: Cannot transform Shape::Callback");
                Shape::Callback(callback)
            }
        }
    }

    /// Get the underlying painter (for accessing clip_rect, fonts, etc)
    pub fn inner(&self) -> &Painter {
        self.painter
    }

    /// Get clip rectangle
    pub fn clip_rect(&self) -> Rect {
        self.painter.clip_rect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_pos() {
        // Identity transform
        let transform = Affine2::IDENTITY;
        let painter = TransformPainter {
            painter: unsafe { &*(std::ptr::null() as *const Painter) }, // Dummy painter for test
            transform,
        };

        let pos = Pos2::new(10.0, 20.0);
        let transformed = painter.transform_pos(pos);
        assert!((transformed.x - 10.0).abs() < 0.001);
        assert!((transformed.y - 20.0).abs() < 0.001);
    }

    #[test]
    fn test_rotation_transform() {
        // 90 degree rotation
        let transform = Affine2::from_angle(std::f32::consts::PI / 2.0);
        let painter = TransformPainter {
            painter: unsafe { &*(std::ptr::null() as *const Painter) },
            transform,
        };

        let pos = Pos2::new(1.0, 0.0);
        let transformed = painter.transform_pos(pos);
        // After 90° rotation, (1,0) becomes (0,1)
        assert!((transformed.x - 0.0).abs() < 0.001);
        assert!((transformed.y - 1.0).abs() < 0.001);
    }
}
