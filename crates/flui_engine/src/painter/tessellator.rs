//! Path tessellation using Lyon
//!
//! Converts vector paths (curves, lines, arcs) into triangle meshes
//! suitable for GPU rendering.

use crate::painter::{
    paint::{Paint, Stroke},
    vertex::Vertex,
};
use flui_types::{geometry::RRect, styling::Color, Point, Rect};
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
};
use thiserror::Error;

/// Errors that can occur during tessellation
#[derive(Debug, Error)]
pub enum TessellationError {
    #[error("Fill tessellation failed: {0}")]
    FillFailed(String),

    #[error("Stroke tessellation failed: {0}")]
    StrokeFailed(String),

    #[error("Invalid path data")]
    InvalidPath,
}

pub type Result<T> = std::result::Result<T, TessellationError>;

/// Vertex constructor for fill tessellation
struct FillVertexConstructor {
    color: Color,
}

impl lyon::tessellation::FillVertexConstructor<Vertex> for FillVertexConstructor {
    fn new_vertex(&mut self, vertex: FillVertex) -> Vertex {
        Vertex::with_color(
            Point::new(vertex.position().x, vertex.position().y),
            self.color,
        )
    }
}

/// Vertex constructor for stroke tessellation
struct StrokeVertexConstructor {
    color: Color,
}

impl lyon::tessellation::StrokeVertexConstructor<Vertex> for StrokeVertexConstructor {
    fn new_vertex(&mut self, vertex: StrokeVertex) -> Vertex {
        Vertex::with_color(
            Point::new(vertex.position().x, vertex.position().y),
            self.color,
        )
    }
}

/// Path tessellator
///
/// Converts vector paths into triangle meshes using Lyon.
/// Provides both fill and stroke tessellation.
pub struct Tessellator {
    /// Lyon fill tessellator
    fill_tessellator: FillTessellator,

    /// Lyon stroke tessellator
    stroke_tessellator: StrokeTessellator,

    /// Reusable geometry buffers
    geometry: VertexBuffers<Vertex, u32>,
}

impl Default for Tessellator {
    fn default() -> Self {
        Self::new()
    }
}

impl Tessellator {
    /// Create a new tessellator
    pub fn new() -> Self {
        Self {
            fill_tessellator: FillTessellator::new(),
            stroke_tessellator: StrokeTessellator::new(),
            geometry: VertexBuffers::new(),
        }
    }

    /// Tessellate a filled path
    ///
    /// # Arguments
    /// * `path` - Lyon path to tessellate
    /// * `paint` - Paint style (color)
    ///
    /// # Returns
    /// Tuple of (vertices, indices) ready for GPU upload
    pub fn tessellate_fill(
        &mut self,
        path: &Path,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        self.geometry.vertices.clear();
        self.geometry.indices.clear();

        let options = FillOptions::default().with_tolerance(0.1);

        self.fill_tessellator
            .tessellate_path(
                path,
                &options,
                &mut BuffersBuilder::new(
                    &mut self.geometry,
                    FillVertexConstructor {
                        color: paint.get_color(),
                    },
                ),
            )
            .map_err(|e| TessellationError::FillFailed(e.to_string()))?;

        Ok((
            self.geometry.vertices.clone(),
            self.geometry.indices.clone(),
        ))
    }

    /// Tessellate a stroked path
    ///
    /// # Arguments
    /// * `path` - Lyon path to tessellate
    /// * `paint` - Paint style (color)
    /// * `stroke` - Stroke parameters
    ///
    /// # Returns
    /// Tuple of (vertices, indices) ready for GPU upload
    pub fn tessellate_stroke(
        &mut self,
        path: &Path,
        paint: &Paint,
        stroke: &Stroke,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        self.geometry.vertices.clear();
        self.geometry.indices.clear();

        // Convert Stroke to lyon StrokeOptions
        use lyon::tessellation::{LineCap, LineJoin};
        let options = StrokeOptions::default()
            .with_line_width(stroke.width())
            .with_line_cap(match stroke.cap() {
                flui_types::painting::StrokeCap::Butt => LineCap::Butt,
                flui_types::painting::StrokeCap::Round => LineCap::Round,
                flui_types::painting::StrokeCap::Square => LineCap::Square,
            })
            .with_line_join(match stroke.join() {
                flui_types::painting::StrokeJoin::Miter => LineJoin::Miter,
                flui_types::painting::StrokeJoin::Round => LineJoin::Round,
                flui_types::painting::StrokeJoin::Bevel => LineJoin::Bevel,
            })
            .with_miter_limit(stroke.miter_limit());

        self.stroke_tessellator
            .tessellate_path(
                path,
                &options,
                &mut BuffersBuilder::new(
                    &mut self.geometry,
                    StrokeVertexConstructor {
                        color: paint.get_color(),
                    },
                ),
            )
            .map_err(|e| TessellationError::StrokeFailed(e.to_string()))?;

        Ok((
            self.geometry.vertices.clone(),
            self.geometry.indices.clone(),
        ))
    }

    /// Tessellate a rectangle (optimized path)
    pub fn tessellate_rect(
        &mut self,
        rect: Rect,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.begin(lyon::geom::point(rect.left(), rect.top()));
        path_builder.line_to(lyon::geom::point(rect.right(), rect.top()));
        path_builder.line_to(lyon::geom::point(rect.right(), rect.bottom()));
        path_builder.line_to(lyon::geom::point(rect.left(), rect.bottom()));
        path_builder.close();

        let path = path_builder.build();
        self.tessellate_fill(&path, paint)
    }

    /// Tessellate a rounded rectangle
    pub fn tessellate_rounded_rect(
        &mut self,
        rect: Rect,
        corner_radius: f32,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        let left = rect.left();
        let top = rect.top();
        let right = rect.right();
        let bottom = rect.bottom();
        let radius = corner_radius
            .min(rect.width() / 2.0)
            .min(rect.height() / 2.0);

        // Start at top-left, after the corner
        path_builder.begin(lyon::geom::point(left + radius, top));

        // Top edge
        path_builder.line_to(lyon::geom::point(right - radius, top));

        // Top-right corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(right, top),
            lyon::geom::point(right, top + radius),
        );

        // Right edge
        path_builder.line_to(lyon::geom::point(right, bottom - radius));

        // Bottom-right corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(right, bottom),
            lyon::geom::point(right - radius, bottom),
        );

        // Bottom edge
        path_builder.line_to(lyon::geom::point(left + radius, bottom));

        // Bottom-left corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(left, bottom),
            lyon::geom::point(left, bottom - radius),
        );

        // Left edge
        path_builder.line_to(lyon::geom::point(left, top + radius));

        // Top-left corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(left, top),
            lyon::geom::point(left + radius, top),
        );

        path_builder.close();

        let path = path_builder.build();
        self.tessellate_fill(&path, paint)
    }

    /// Tessellate a circle
    pub fn tessellate_circle(
        &mut self,
        center: Point,
        radius: f32,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.add_circle(
            lyon::geom::point(center.x, center.y),
            radius,
            lyon::path::Winding::Positive,
        );

        let path = path_builder.build();
        self.tessellate_fill(&path, paint)
    }

    /// Tessellate an ellipse
    pub fn tessellate_ellipse(
        &mut self,
        center: Point,
        radii: Point,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.add_ellipse(
            lyon::geom::point(center.x, center.y),
            lyon::geom::vector(radii.x, radii.y),
            lyon::geom::Angle::radians(0.0),
            lyon::path::Winding::Positive,
        );

        let path = path_builder.build();
        self.tessellate_fill(&path, paint)
    }

    /// Create a lyon path from points (polyline)
    pub fn create_polyline_path(points: &[Point], closed: bool) -> Path {
        if points.is_empty() {
            return Path::builder().build();
        }

        let mut path_builder = Path::builder();

        path_builder.begin(lyon::geom::point(points[0].x, points[0].y));

        for point in &points[1..] {
            path_builder.line_to(lyon::geom::point(point.x, point.y));
        }

        if closed {
            path_builder.close();
        } else {
            path_builder.end(false);
        }

        path_builder.build()
    }

    // ===== Additional methods for WgpuPainter =====

    /// Tessellate a rounded rectangle (RRect)
    ///
    /// Alias for tessellate_rounded_rect that accepts RRect type.
    pub fn tessellate_rrect(
        &mut self,
        rrect: RRect,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        // Use average of all corner radii for simplicity
        // TODO: Support per-corner radii
        let radius = (rrect.top_left.x
            + rrect.top_left.y
            + rrect.top_right.x
            + rrect.top_right.y
            + rrect.bottom_left.x
            + rrect.bottom_left.y
            + rrect.bottom_right.x
            + rrect.bottom_right.y)
            / 8.0;

        self.tessellate_rounded_rect(rrect.rect, radius, paint)
    }

    /// Tessellate a stroked rectangle
    pub fn tessellate_rect_stroke(
        &mut self,
        rect: Rect,
        paint: &Paint,
        stroke: &Stroke,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.begin(lyon::geom::point(rect.left(), rect.top()));
        path_builder.line_to(lyon::geom::point(rect.right(), rect.top()));
        path_builder.line_to(lyon::geom::point(rect.right(), rect.bottom()));
        path_builder.line_to(lyon::geom::point(rect.left(), rect.bottom()));
        path_builder.close();

        let path = path_builder.build();
        self.tessellate_stroke(&path, paint, stroke)
    }

    /// Tessellate a line
    pub fn tessellate_line(
        &mut self,
        p1: Point,
        p2: Point,
        paint: &Paint,
        stroke: &Stroke,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let points = vec![p1, p2];
        let path = Self::create_polyline_path(&points, false);
        self.tessellate_stroke(&path, paint, stroke)
    }
}

/// Helper trait for creating lyon paths from FLUI types
pub trait IntoLyonPath {
    /// Convert to lyon path
    fn into_lyon_path(&self) -> Path;
}

impl IntoLyonPath for Rect {
    fn into_lyon_path(&self) -> Path {
        let mut builder = Path::builder();

        builder.begin(lyon::geom::point(self.left(), self.top()));
        builder.line_to(lyon::geom::point(self.right(), self.top()));
        builder.line_to(lyon::geom::point(self.right(), self.bottom()));
        builder.line_to(lyon::geom::point(self.left(), self.bottom()));
        builder.close();

        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tessellate_rect() {
        let mut tessellator = Tessellator::new();
        let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let paint = Paint::solid(Color::RED);

        let result = tessellator.tessellate_rect(rect, &paint);
        assert!(result.is_ok());

        let (vertices, indices) = result.unwrap();
        assert!(!vertices.is_empty());
        assert!(!indices.is_empty());
        assert_eq!(indices.len() % 3, 0); // Should be triangles
    }

    #[test]
    fn test_tessellate_circle() {
        let mut tessellator = Tessellator::new();
        let center = Point::new(50.0, 50.0);
        let paint = Paint::solid(Color::BLUE);

        let result = tessellator.tessellate_circle(center, 25.0, &paint);
        assert!(result.is_ok());

        let (vertices, indices) = result.unwrap();
        assert!(!vertices.is_empty());
        assert!(!indices.is_empty());
    }

    #[test]
    fn test_tessellate_rounded_rect() {
        let mut tessellator = Tessellator::new();
        let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let paint = Paint::solid(Color::GREEN);

        let result = tessellator.tessellate_rounded_rect(rect, 10.0, &paint);
        assert!(result.is_ok());

        let (vertices, indices) = result.unwrap();
        assert!(!vertices.is_empty());
        assert!(!indices.is_empty());
    }

    #[test]
    fn test_create_polyline_path() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(20.0, 0.0),
        ];

        let path = Tessellator::create_polyline_path(&points, false);
        // Path should be created successfully
        // We can't easily test the internal structure, but we can verify it doesn't panic
    }
}
