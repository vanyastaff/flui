//! Path tessellation using Lyon
//!
//! Converts vector paths (curves, lines, arcs) into triangle meshes
//! suitable for GPU rendering.

use crate::painter::vertex::Vertex;
use flui_painting::{Paint, StrokeCap, StrokeJoin};
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
#[derive(Default)]
pub struct Tessellator {
    /// Lyon fill tessellator
    fill_tessellator: FillTessellator,

    /// Lyon stroke tessellator
    stroke_tessellator: StrokeTessellator,

    /// Reusable geometry buffers
    geometry: VertexBuffers<Vertex, u32>,
}

impl Tessellator {
    /// Create a new tessellator
    pub fn new() -> Self {
        Self::default()
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
                        color: paint.color,
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
    /// * `paint` - Paint style (contains stroke information)
    ///
    /// # Returns
    /// Tuple of (vertices, indices) ready for GPU upload
    pub fn tessellate_stroke(
        &mut self,
        path: &Path,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        self.geometry.vertices.clear();
        self.geometry.indices.clear();

        // Extract stroke info from Paint
        use lyon::tessellation::{LineCap, LineJoin};
        let options = StrokeOptions::default()
            .with_line_width(paint.stroke_width)
            .with_line_cap(match paint.stroke_cap {
                StrokeCap::Butt => LineCap::Butt,
                StrokeCap::Round => LineCap::Round,
                StrokeCap::Square => LineCap::Square,
            })
            .with_line_join(match paint.stroke_join {
                StrokeJoin::Miter => LineJoin::Miter,
                StrokeJoin::Round => LineJoin::Round,
                StrokeJoin::Bevel => LineJoin::Bevel,
            })
            .with_miter_limit(4.0);

        self.stroke_tessellator
            .tessellate_path(
                path,
                &options,
                &mut BuffersBuilder::new(
                    &mut self.geometry,
                    StrokeVertexConstructor {
                        color: paint.color,
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

    /// Tessellate a double rounded rectangle (ring/border with inner cutout)
    ///
    /// Creates a path with two contours: outer (positive winding) and inner (negative winding).
    /// The result is a ring or border where the inner RRect is cut out from the outer RRect.
    ///
    /// # Arguments
    /// * `outer` - Outer rounded rectangle
    /// * `inner` - Inner rounded rectangle (cutout)
    /// * `paint` - Paint style (color)
    ///
    /// # Returns
    /// Tuple of (vertices, indices) ready for GPU upload
    pub fn tessellate_drrect(
        &mut self,
        outer: &RRect,
        inner: &RRect,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        // Helper to add an RRect to the path builder with specified winding
        let add_rrect = |builder: &mut lyon::path::path::Builder,
                         rrect: &RRect,
                         winding: lyon::path::Winding| {
            let rect = rrect.rect;
            let left = rect.left();
            let top = rect.top();
            let right = rect.right();
            let bottom = rect.bottom();

            // Get corner radii (clamp to half the smallest dimension)
            let max_radius_x = rect.width() / 2.0;
            let max_radius_y = rect.height() / 2.0;

            let tl_x = rrect.top_left.x.min(max_radius_x);
            let tl_y = rrect.top_left.y.min(max_radius_y);
            let tr_x = rrect.top_right.x.min(max_radius_x);
            let tr_y = rrect.top_right.y.min(max_radius_y);
            let br_x = rrect.bottom_right.x.min(max_radius_x);
            let br_y = rrect.bottom_right.y.min(max_radius_y);
            let bl_x = rrect.bottom_left.x.min(max_radius_x);
            let bl_y = rrect.bottom_left.y.min(max_radius_y);

            // Build the path based on winding direction
            match winding {
                lyon::path::Winding::Positive => {
                    // Clockwise: top-left -> top-right -> bottom-right -> bottom-left
                    builder.begin(lyon::geom::point(left + tl_x, top));

                    // Top edge to top-right corner
                    builder.line_to(lyon::geom::point(right - tr_x, top));
                    // Top-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right, top),
                        lyon::geom::point(right, top + tr_y),
                    );

                    // Right edge to bottom-right corner
                    builder.line_to(lyon::geom::point(right, bottom - br_y));
                    // Bottom-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right, bottom),
                        lyon::geom::point(right - br_x, bottom),
                    );

                    // Bottom edge to bottom-left corner
                    builder.line_to(lyon::geom::point(left + bl_x, bottom));
                    // Bottom-left corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left, bottom),
                        lyon::geom::point(left, bottom - bl_y),
                    );

                    // Left edge to top-left corner
                    builder.line_to(lyon::geom::point(left, top + tl_y));
                    // Top-left corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left, top),
                        lyon::geom::point(left + tl_x, top),
                    );

                    builder.close();
                }
                lyon::path::Winding::Negative => {
                    // Counter-clockwise: top-left -> bottom-left -> bottom-right -> top-right
                    builder.begin(lyon::geom::point(left + tl_x, top));

                    // Top-left corner (reverse)
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left, top),
                        lyon::geom::point(left, top + tl_y),
                    );

                    // Left edge to bottom-left corner
                    builder.line_to(lyon::geom::point(left, bottom - bl_y));
                    // Bottom-left corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left, bottom),
                        lyon::geom::point(left + bl_x, bottom),
                    );

                    // Bottom edge to bottom-right corner
                    builder.line_to(lyon::geom::point(right - br_x, bottom));
                    // Bottom-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right, bottom),
                        lyon::geom::point(right, bottom - br_y),
                    );

                    // Right edge to top-right corner
                    builder.line_to(lyon::geom::point(right, top + tr_y));
                    // Top-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right, top),
                        lyon::geom::point(right - tr_x, top),
                    );

                    // Top edge back to start
                    builder.line_to(lyon::geom::point(left + tl_x, top));

                    builder.close();
                }
            }
        };

        // Add outer RRect with positive winding (filled)
        add_rrect(&mut path_builder, outer, lyon::path::Winding::Positive);

        // Add inner RRect with negative winding (cutout)
        add_rrect(&mut path_builder, inner, lyon::path::Winding::Negative);

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
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.begin(lyon::geom::point(rect.left(), rect.top()));
        path_builder.line_to(lyon::geom::point(rect.right(), rect.top()));
        path_builder.line_to(lyon::geom::point(rect.right(), rect.bottom()));
        path_builder.line_to(lyon::geom::point(rect.left(), rect.bottom()));
        path_builder.close();

        let path = path_builder.build();
        self.tessellate_stroke(&path, paint)
    }

    /// Tessellate a line
    pub fn tessellate_line(
        &mut self,
        p1: Point,
        p2: Point,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "Tessellator::tessellate_line: p1={:?}, p2={:?}, stroke_width={}",
            p1,
            p2,
            paint.stroke_width
        );

        let points = vec![p1, p2];
        let path = Self::create_polyline_path(&points, false);
        let result = self.tessellate_stroke(&path, paint);

        #[cfg(debug_assertions)]
        match &result {
            Ok((verts, inds)) => tracing::debug!(
                "Tessellator::tessellate_line: SUCCESS - {} vertices, {} indices",
                verts.len(),
                inds.len()
            ),
            Err(e) => tracing::error!("Tessellator::tessellate_line: FAILED - {}", e),
        }

        result
    }

    /// Tessellate a FLUI Path (filled)
    pub fn tessellate_flui_path_fill(
        &mut self,
        flui_path: &flui_types::painting::path::Path,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let lyon_path = flui_path.to_lyon_path();
        self.tessellate_fill(&lyon_path, paint)
    }

    /// Tessellate a FLUI Path (stroked)
    pub fn tessellate_flui_path_stroke(
        &mut self,
        flui_path: &flui_types::painting::path::Path,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let lyon_path = flui_path.to_lyon_path();
        self.tessellate_stroke(&lyon_path, paint)
    }
}

/// Helper trait for creating lyon paths from FLUI types
pub trait IntoLyonPath {
    /// Convert to lyon path
    fn to_lyon_path(&self) -> Path;
}

impl IntoLyonPath for Rect {
    fn to_lyon_path(&self) -> Path {
        let mut builder = Path::builder();

        builder.begin(lyon::geom::point(self.left(), self.top()));
        builder.line_to(lyon::geom::point(self.right(), self.top()));
        builder.line_to(lyon::geom::point(self.right(), self.bottom()));
        builder.line_to(lyon::geom::point(self.left(), self.bottom()));
        builder.close();

        builder.build()
    }
}

impl IntoLyonPath for flui_types::painting::path::Path {
    fn to_lyon_path(&self) -> Path {
        use flui_types::painting::path::PathCommand;

        let mut builder = Path::builder();
        let mut current_pos: Option<Point> = None;

        for command in self.commands() {
            match command {
                PathCommand::MoveTo(point) => {
                    // End previous subpath if exists
                    if current_pos.is_some() {
                        builder.end(false);
                    }
                    builder.begin(lyon::geom::point(point.x, point.y));
                    current_pos = Some(*point);
                }

                PathCommand::LineTo(point) => {
                    builder.line_to(lyon::geom::point(point.x, point.y));
                    current_pos = Some(*point);
                }

                PathCommand::QuadraticTo(control, end) => {
                    builder.quadratic_bezier_to(
                        lyon::geom::point(control.x, control.y),
                        lyon::geom::point(end.x, end.y),
                    );
                    current_pos = Some(*end);
                }

                PathCommand::CubicTo(control1, control2, end) => {
                    builder.cubic_bezier_to(
                        lyon::geom::point(control1.x, control1.y),
                        lyon::geom::point(control2.x, control2.y),
                        lyon::geom::point(end.x, end.y),
                    );
                    current_pos = Some(*end);
                }

                PathCommand::Close => {
                    builder.close();
                }

                PathCommand::AddRect(rect) => {
                    // Start new subpath for rectangle
                    if current_pos.is_some() {
                        builder.end(false);
                    }
                    builder.begin(lyon::geom::point(rect.left(), rect.top()));
                    builder.line_to(lyon::geom::point(rect.right(), rect.top()));
                    builder.line_to(lyon::geom::point(rect.right(), rect.bottom()));
                    builder.line_to(lyon::geom::point(rect.left(), rect.bottom()));
                    builder.close();
                    current_pos = None;
                }

                PathCommand::AddCircle(center, radius) => {
                    // Start new subpath for circle
                    if current_pos.is_some() {
                        builder.end(false);
                    }
                    builder.add_circle(
                        lyon::geom::point(center.x, center.y),
                        *radius,
                        lyon::path::Winding::Positive,
                    );
                    current_pos = None;
                }

                PathCommand::AddOval(rect) => {
                    // Start new subpath for oval/ellipse
                    if current_pos.is_some() {
                        builder.end(false);
                    }
                    let center = rect.center();
                    let radii = lyon::geom::vector(rect.width() / 2.0, rect.height() / 2.0);
                    builder.add_ellipse(
                        lyon::geom::point(center.x, center.y),
                        radii,
                        lyon::geom::Angle::radians(0.0),
                        lyon::path::Winding::Positive,
                    );
                    current_pos = None;
                }

                PathCommand::AddArc(rect, start_angle, sweep_angle) => {
                    // Start new subpath for arc
                    if current_pos.is_some() {
                        builder.end(false);
                    }
                    let center = rect.center();
                    let radii = lyon::geom::vector(rect.width() / 2.0, rect.height() / 2.0);

                    // Approximate arc with line segments
                    // Use more segments for larger sweep angles
                    let num_segments =
                        ((sweep_angle.abs() / (std::f32::consts::PI / 6.0)).ceil() as i32).max(4);
                    let angle_step = sweep_angle / num_segments as f32;

                    let start_x = center.x + radii.x * start_angle.cos();
                    let start_y = center.y + radii.y * start_angle.sin();

                    builder.begin(lyon::geom::point(start_x, start_y));

                    for i in 1..=num_segments {
                        let angle = start_angle + angle_step * i as f32;
                        let x = center.x + radii.x * angle.cos();
                        let y = center.y + radii.y * angle.sin();
                        builder.line_to(lyon::geom::point(x, y));
                    }

                    let end_angle = start_angle + sweep_angle;
                    let end_x = center.x + radii.x * end_angle.cos();
                    let end_y = center.y + radii.y * end_angle.sin();
                    current_pos = Some(Point::new(end_x, end_y));
                }
            }
        }

        // End the final subpath if not closed
        if current_pos.is_some() {
            builder.end(false);
        }

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
        let paint = Paint::fill(Color::RED);

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
        let paint = Paint::fill(Color::BLUE);

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
        let paint = Paint::fill(Color::GREEN);

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

        let _path = Tessellator::create_polyline_path(&points, false);
        // Path should be created successfully
        // We can't easily test the internal structure, but we can verify it doesn't panic
    }
}
