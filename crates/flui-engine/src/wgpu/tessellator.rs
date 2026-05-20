//! Path tessellation using Lyon
//!
//! Converts vector paths (curves, lines, arcs) into triangle meshes
//! suitable for GPU rendering.

use flui_painting::{Paint, StrokeCap, StrokeJoin};
use flui_types::{
    Point, Rect,
    geometry::{Pixels, RRect},
    styling::Color,
};
use lyon::{
    path::Path,
    tessellation::{
        BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
        StrokeVertex, VertexBuffers,
    },
};
use thiserror::Error;

use super::vertex::Vertex;

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
    fn new_vertex(&mut self, vertex: FillVertex<'_>) -> Vertex {
        Vertex::new(
            [vertex.position().x, vertex.position().y],
            self.color.to_rgba_f32_array(),
            [0.0, 0.0],
        )
    }
}

/// Vertex constructor for stroke tessellation
struct StrokeVertexConstructor {
    color: Color,
}

impl lyon::tessellation::StrokeVertexConstructor<Vertex> for StrokeVertexConstructor {
    fn new_vertex(&mut self, vertex: StrokeVertex<'_, '_>) -> Vertex {
        Vertex::new(
            [vertex.position().x, vertex.position().y],
            self.color.to_rgba_f32_array(),
            [0.0, 0.0],
        )
    }
}

/// Path tessellator
///
/// Converts vector paths into triangle meshes using Lyon.
/// Provides both fill and stroke tessellation.
#[derive(Default)]
#[allow(missing_debug_implementations)]
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
                    FillVertexConstructor { color: paint.color },
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
        use lyon::tessellation::{LineCap, LineJoin};

        self.geometry.vertices.clear();
        self.geometry.indices.clear();

        // Extract stroke info from Paint
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
                    StrokeVertexConstructor { color: paint.color },
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
        rect: Rect<Pixels>,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.begin(lyon::geom::point(rect.left().0, rect.top().0));
        path_builder.line_to(lyon::geom::point(rect.right().0, rect.top().0));
        path_builder.line_to(lyon::geom::point(rect.right().0, rect.bottom().0));
        path_builder.line_to(lyon::geom::point(rect.left().0, rect.bottom().0));
        path_builder.close();

        let path = path_builder.build();
        self.tessellate_fill(&path, paint)
    }

    /// Tessellate a rounded rectangle
    pub fn tessellate_rounded_rect(
        &mut self,
        rect: Rect<Pixels>,
        corner_radius: f32,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        let left = rect.left().0;
        let top = rect.top().0;
        let right = rect.right().0;
        let bottom = rect.bottom().0;
        let radius = corner_radius
            .min(rect.width().0 / 2.0)
            .min(rect.height().0 / 2.0);

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
        center: Point<Pixels>,
        radius: f32,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.add_circle(
            lyon::geom::point(center.x.0, center.y.0),
            radius,
            lyon::path::Winding::Positive,
        );

        let path = path_builder.build();
        self.tessellate_fill(&path, paint)
    }

    /// Tessellate an ellipse
    pub fn tessellate_ellipse(
        &mut self,
        center: Point<Pixels>,
        radii: Point<Pixels>,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.add_ellipse(
            lyon::geom::point(center.x.0, center.y.0),
            lyon::geom::vector(radii.x.0, radii.y.0),
            lyon::geom::Angle::radians(0.0),
            lyon::path::Winding::Positive,
        );

        let path = path_builder.build();
        self.tessellate_fill(&path, paint)
    }

    /// Tessellate an arc (pie slice or arc stroke)
    ///
    /// Uses lyon's `Arc` geometry primitive to generate accurate cubic Bezier
    /// curves instead of a manual line-segment approximation.
    ///
    /// # Arguments
    /// * `rect` - Bounding rectangle of the ellipse
    /// * `start_angle` - Start angle in radians
    /// * `sweep_angle` - Sweep angle in radians
    /// * `use_center` - If true, draws a pie slice (connected to center)
    /// * `paint` - Paint style (fill or stroke)
    ///
    /// # Returns
    /// Tuple of (vertices, indices) ready for GPU upload
    pub fn tessellate_arc(
        &mut self,
        rect: Rect<Pixels>,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        let center = rect.center();
        let rx = (rect.width() / 2.0).0;
        let ry = (rect.height() / 2.0).0;

        // Handle near-zero sweep: emit a degenerate path (just the start point)
        if sweep_angle.abs() < 1e-6 {
            let start_x = center.x.0 + rx * start_angle.cos();
            let start_y = center.y.0 + ry * start_angle.sin();
            path_builder.begin(lyon::geom::point(start_x, start_y));
            path_builder.end(false);
            let path = path_builder.build();
            return if paint.style == flui_painting::PaintStyle::Fill {
                self.tessellate_fill(&path, paint)
            } else {
                self.tessellate_stroke(&path, paint)
            };
        }

        // Build a lyon Arc and convert to cubic Bezier curves
        let arc = lyon::geom::Arc {
            center: lyon::geom::point(center.x.0, center.y.0),
            radii: lyon::geom::vector(rx, ry),
            start_angle: lyon::geom::Angle::radians(start_angle),
            sweep_angle: lyon::geom::Angle::radians(sweep_angle),
            x_rotation: lyon::geom::Angle::radians(0.0),
        };

        let arc_start = arc.from();

        if use_center {
            // Pie slice: start from center, line to arc start
            path_builder.begin(lyon::geom::point(center.x.0, center.y.0));
            path_builder.line_to(arc_start);
        } else {
            path_builder.begin(arc_start);
        }

        // Emit the arc as a series of cubic Bezier curves
        arc.for_each_cubic_bezier(&mut |cubic| {
            path_builder.cubic_bezier_to(cubic.ctrl1, cubic.ctrl2, cubic.to);
        });

        if use_center {
            // Pie slice: close back to center
            path_builder.line_to(lyon::geom::point(center.x.0, center.y.0));
            path_builder.close();
        } else {
            path_builder.end(false);
        }

        let path = path_builder.build();

        // Use fill or stroke based on paint style
        if paint.style == flui_painting::PaintStyle::Fill {
            self.tessellate_fill(&path, paint)
        } else {
            self.tessellate_stroke(&path, paint)
        }
    }

    /// Tessellate a double rounded rectangle (ring/border with inner cutout)
    ///
    /// Creates a path with two contours: outer (positive winding) and inner
    /// (negative winding). The result is a ring or border where the inner
    /// RRect is cut out from the outer RRect.
    ///
    /// # Arguments
    /// * `outer` - Outer rounded rectangle
    /// * `inner` - Inner rounded rectangle (cutout)
    /// * `paint` - Paint style (color)
    ///
    /// # Returns
    /// Tuple of (vertices, indices) ready for GPU upload
    #[allow(clippy::similar_names)] // tl_x/tl_y, tr_x/tr_y, etc. are intentional corner names
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
                    builder.begin(lyon::geom::point((left + tl_x).0, top.0));

                    // Top edge to top-right corner
                    builder.line_to(lyon::geom::point((right - tr_x).0, top.0));
                    // Top-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right.0, top.0),
                        lyon::geom::point(right.0, (top + tr_y).0),
                    );

                    // Right edge to bottom-right corner
                    builder.line_to(lyon::geom::point(right.0, (bottom - br_y).0));
                    // Bottom-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right.0, bottom.0),
                        lyon::geom::point((right - br_x).0, bottom.0),
                    );

                    // Bottom edge to bottom-left corner
                    builder.line_to(lyon::geom::point((left + bl_x).0, bottom.0));
                    // Bottom-left corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left.0, bottom.0),
                        lyon::geom::point(left.0, (bottom - bl_y).0),
                    );

                    // Left edge to top-left corner
                    builder.line_to(lyon::geom::point(left.0, (top + tl_y).0));
                    // Top-left corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left.0, top.0),
                        lyon::geom::point((left + tl_x).0, top.0),
                    );

                    builder.close();
                }
                lyon::path::Winding::Negative => {
                    // Counter-clockwise: top-left -> bottom-left -> bottom-right -> top-right
                    builder.begin(lyon::geom::point((left + tl_x).0, top.0));

                    // Top-left corner (reverse)
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left.0, top.0),
                        lyon::geom::point(left.0, (top + tl_y).0),
                    );

                    // Left edge to bottom-left corner
                    builder.line_to(lyon::geom::point(left.0, (bottom - bl_y).0));
                    // Bottom-left corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left.0, bottom.0),
                        lyon::geom::point((left + bl_x).0, bottom.0),
                    );

                    // Bottom edge to bottom-right corner
                    builder.line_to(lyon::geom::point((right - br_x).0, bottom.0));
                    // Bottom-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right.0, bottom.0),
                        lyon::geom::point(right.0, (bottom - br_y).0),
                    );

                    // Right edge to top-right corner
                    builder.line_to(lyon::geom::point(right.0, (top + tr_y).0));
                    // Top-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right.0, top.0),
                        lyon::geom::point((right - tr_x).0, top.0),
                    );

                    // Top edge back to start
                    builder.line_to(lyon::geom::point((left + tl_x).0, top.0));

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
    pub fn create_polyline_path(points: &[Point<Pixels>], closed: bool) -> Path {
        if points.is_empty() {
            return Path::builder().build();
        }

        let mut path_builder = Path::builder();

        path_builder.begin(lyon::geom::point(points[0].x.0, points[0].y.0));

        for point in &points[1..] {
            path_builder.line_to(lyon::geom::point(point.x.0, point.y.0));
        }

        if closed {
            path_builder.close();
        } else {
            path_builder.end(false);
        }

        path_builder.build()
    }

    // ===== Additional methods for WgpuPainter =====

    /// Tessellate a rounded rectangle (RRect) with per-corner radii
    ///
    /// Builds a lyon path with independent corner arcs, supporting
    /// different radii for each corner of the rectangle.
    #[allow(clippy::similar_names)]
    pub fn tessellate_rrect(
        &mut self,
        rrect: RRect,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        let rect = rrect.rect;
        let left = rect.left();
        let top = rect.top();
        let right = rect.right();
        let bottom = rect.bottom();

        // Clamp corner radii to half the smallest dimension
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

        // Start at top-left, after the corner arc
        path_builder.begin(lyon::geom::point((left + tl_x).0, top.0));

        // Top edge to top-right corner
        path_builder.line_to(lyon::geom::point((right - tr_x).0, top.0));
        // Top-right corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(right.0, top.0),
            lyon::geom::point(right.0, (top + tr_y).0),
        );

        // Right edge to bottom-right corner
        path_builder.line_to(lyon::geom::point(right.0, (bottom - br_y).0));
        // Bottom-right corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(right.0, bottom.0),
            lyon::geom::point((right - br_x).0, bottom.0),
        );

        // Bottom edge to bottom-left corner
        path_builder.line_to(lyon::geom::point((left + bl_x).0, bottom.0));
        // Bottom-left corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(left.0, bottom.0),
            lyon::geom::point(left.0, (bottom - bl_y).0),
        );

        // Left edge to top-left corner
        path_builder.line_to(lyon::geom::point(left.0, (top + tl_y).0));
        // Top-left corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(left.0, top.0),
            lyon::geom::point((left + tl_x).0, top.0),
        );

        path_builder.close();

        let path = path_builder.build();
        self.tessellate_fill(&path, paint)
    }

    /// Tessellate a stroked rectangle
    pub fn tessellate_rect_stroke(
        &mut self,
        rect: Rect<Pixels>,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.begin(lyon::geom::point(rect.left().0, rect.top().0));
        path_builder.line_to(lyon::geom::point(rect.right().0, rect.top().0));
        path_builder.line_to(lyon::geom::point(rect.right().0, rect.bottom().0));
        path_builder.line_to(lyon::geom::point(rect.left().0, rect.bottom().0));
        path_builder.close();

        let path = path_builder.build();
        self.tessellate_stroke(&path, paint)
    }

    /// Tessellate a line
    pub fn tessellate_line(
        &mut self,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "Tessellator::tessellate_line: p1={:?}, p2={:?}, stroke_width={}",
            p1,
            p2,
            paint.stroke_width
        );

        let points = vec![p1, p2];
        let path = Self::create_polyline_path(&points, false);
        let result = if let Some(ref dash) = paint.dash_pattern {
            self.tessellate_dashed_stroke(&path, paint, dash)
        } else {
            self.tessellate_stroke(&path, paint)
        };

        #[cfg(debug_assertions)]
        match &result {
            Ok((verts, inds)) => tracing::trace!(
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

    /// Tessellate a stroked lyon path with dash pattern.
    ///
    /// Splits the path into dash segments based on the pattern, then tessellates
    /// each dash as a separate stroke sub-path.
    ///
    /// # Arguments
    /// * `path` - Lyon path to tessellate
    /// * `paint` - Paint style (must have stroke style and dash_pattern set)
    /// * `dash_pattern` - The dash pattern (intervals and phase)
    ///
    /// # Returns
    /// Tuple of (vertices, indices) ready for GPU upload
    pub fn tessellate_dashed_stroke(
        &mut self,
        path: &Path,
        paint: &Paint,
        dash_pattern: &flui_types::painting::DashPattern,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        use lyon::path::PathEvent;
        use lyon::path::iterator::PathIterator;
        use lyon::tessellation::{LineCap, LineJoin};

        if !dash_pattern.is_valid() {
            // Fallback to solid stroke if pattern is invalid
            return self.tessellate_stroke(path, paint);
        }

        let intervals = &dash_pattern.intervals;
        // Normalize: if odd number of intervals, conceptually double the array
        let effective_intervals: Vec<f32> = if intervals.len() % 2 != 0 {
            intervals.iter().chain(intervals.iter()).copied().collect()
        } else {
            intervals.clone()
        };

        let cycle_length: f32 = effective_intervals.iter().sum();
        if cycle_length <= 0.0 {
            return self.tessellate_stroke(path, paint);
        }

        // Collect all line segments from the path by flattening curves
        let mut segments: Vec<(lyon::geom::Point<f32>, lyon::geom::Point<f32>)> = Vec::new();
        let mut current_pos = lyon::geom::point(0.0f32, 0.0);

        // Flatten the path to line segments (tolerance controls curve approximation quality)
        for event in path.iter().flattened(0.5) {
            match event {
                PathEvent::Begin { at } => {
                    current_pos = at;
                }
                PathEvent::Line { from: _, to } => {
                    segments.push((current_pos, to));
                    current_pos = to;
                }
                PathEvent::End { .. } => {}
                PathEvent::Quadratic { .. } | PathEvent::Cubic { .. } => {
                    // flattened() should have converted these to lines
                }
            }
        }

        if segments.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        // Walk the segments and generate dash sub-paths
        let mut dash_paths: Vec<Path> = Vec::new();
        let mut phase = dash_pattern.phase % cycle_length;
        if phase < 0.0 {
            phase += cycle_length;
        }

        // Find starting interval index and remaining distance in that interval
        let mut interval_idx = 0usize;
        let mut remaining_in_interval = effective_intervals[0];
        let mut consumed = 0.0f32;
        while consumed + remaining_in_interval <= phase && interval_idx < effective_intervals.len()
        {
            consumed += remaining_in_interval;
            interval_idx = (interval_idx + 1) % effective_intervals.len();
            remaining_in_interval = effective_intervals[interval_idx];
        }
        remaining_in_interval -= phase - consumed;
        let is_drawing = interval_idx % 2 == 0; // Even indices are dashes, odd are gaps

        let mut drawing = is_drawing;
        let mut remaining = remaining_in_interval;

        let mut current_builder: Option<lyon::path::BuilderWithAttributes> = None;
        if drawing {
            current_builder = Some(Path::builder_with_attributes(0));
        }
        let mut started_subpath = false;

        for (from, to) in &segments {
            let dx = to.x - from.x;
            let dy = to.y - from.y;
            let seg_length = (dx * dx + dy * dy).sqrt();
            if seg_length < f32::EPSILON {
                continue;
            }
            let dir_x = dx / seg_length;
            let dir_y = dy / seg_length;

            let mut offset = 0.0f32;

            while offset < seg_length {
                let available = seg_length - offset;
                let consume = remaining.min(available);

                let start_x = from.x + dir_x * offset;
                let start_y = from.y + dir_y * offset;
                let end_x = from.x + dir_x * (offset + consume);
                let end_y = from.y + dir_y * (offset + consume);

                if drawing {
                    if let Some(ref mut builder) = current_builder {
                        if !started_subpath {
                            builder.begin(lyon::geom::point(start_x, start_y), &[]);
                            started_subpath = true;
                        }
                        builder.line_to(lyon::geom::point(end_x, end_y), &[]);
                    }
                }

                remaining -= consume;
                offset += consume;

                if remaining <= f32::EPSILON {
                    // Finished current interval, move to next
                    if drawing && started_subpath {
                        if let Some(mut builder) = current_builder.take() {
                            builder.end(false);
                            dash_paths.push(builder.build());
                        }
                        started_subpath = false;
                    }
                    interval_idx = (interval_idx + 1) % effective_intervals.len();
                    drawing = interval_idx % 2 == 0;
                    remaining = effective_intervals[interval_idx];
                    if drawing {
                        current_builder = Some(Path::builder_with_attributes(0));
                    } else {
                        current_builder = None;
                    }
                }
            }
        }

        // Finish any in-progress dash
        if drawing && started_subpath {
            if let Some(mut builder) = current_builder.take() {
                builder.end(false);
                dash_paths.push(builder.build());
            }
        }

        // Now tessellate all dash sub-paths and combine the geometry
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

        let mut all_vertices: Vec<Vertex> = Vec::new();
        let mut all_indices: Vec<u32> = Vec::new();

        for dash_path in &dash_paths {
            self.geometry.vertices.clear();
            self.geometry.indices.clear();

            self.stroke_tessellator
                .tessellate_path(
                    dash_path,
                    &options,
                    &mut BuffersBuilder::new(
                        &mut self.geometry,
                        StrokeVertexConstructor { color: paint.color },
                    ),
                )
                .map_err(|e| TessellationError::StrokeFailed(e.to_string()))?;

            // Offset indices for combined buffer
            #[allow(clippy::cast_possible_truncation)]
            let base_vertex = all_vertices.len() as u32;
            all_vertices.extend_from_slice(&self.geometry.vertices);
            all_indices.extend(self.geometry.indices.iter().map(|i| i + base_vertex));
        }

        Ok((all_vertices, all_indices))
    }

    /// Tessellate a stroked FLUI path with dash pattern.
    ///
    /// Converts the FLUI path to a lyon path, then delegates to
    /// `tessellate_dashed_stroke`.
    pub fn tessellate_flui_path_dashed_stroke(
        &mut self,
        flui_path: &flui_types::painting::path::Path,
        paint: &Paint,
        dash_pattern: &flui_types::painting::DashPattern,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let lyon_path = flui_path.to_lyon_path();
        self.tessellate_dashed_stroke(&lyon_path, paint, dash_pattern)
    }
}

/// Helper trait for creating lyon paths from FLUI types
pub trait IntoLyonPath {
    /// Convert to lyon path
    fn to_lyon_path(&self) -> Path;
}

impl IntoLyonPath for Rect<Pixels> {
    fn to_lyon_path(&self) -> Path {
        let mut builder = Path::builder();

        builder.begin(lyon::geom::point(self.left().0, self.top().0));
        builder.line_to(lyon::geom::point(self.right().0, self.top().0));
        builder.line_to(lyon::geom::point(self.right().0, self.bottom().0));
        builder.line_to(lyon::geom::point(self.left().0, self.bottom().0));
        builder.close();

        builder.build()
    }
}

impl IntoLyonPath for flui_types::painting::path::Path {
    fn to_lyon_path(&self) -> Path {
        use flui_types::painting::path::PathCommand;

        let mut builder = Path::builder();
        let mut _current_pos: Option<Point<Pixels>> = None;
        let mut has_begun = false;

        for command in self.commands() {
            match command {
                PathCommand::MoveTo(point) => {
                    // End previous subpath if exists
                    if has_begun {
                        builder.end(false);
                    }
                    builder.begin(lyon::geom::point(point.x.0, point.y.0));
                    _current_pos = Some(*point);
                    has_begun = true;
                }

                PathCommand::LineTo(point) => {
                    // Auto-begin if no move_to was called
                    if has_begun {
                        builder.line_to(lyon::geom::point(point.x.0, point.y.0));
                    } else {
                        builder.begin(lyon::geom::point(point.x.0, point.y.0));
                        has_begun = true;
                    }
                    _current_pos = Some(*point);
                }

                PathCommand::QuadraticTo(control, end) => {
                    if !has_begun {
                        builder.begin(lyon::geom::point(control.x.0, control.y.0));
                        has_begun = true;
                    }
                    builder.quadratic_bezier_to(
                        lyon::geom::point(control.x.0, control.y.0),
                        lyon::geom::point(end.x.0, end.y.0),
                    );
                    _current_pos = Some(*end);
                }

                PathCommand::CubicTo(control1, control2, end) => {
                    if !has_begun {
                        builder.begin(lyon::geom::point(control1.x.0, control1.y.0));
                        has_begun = true;
                    }
                    builder.cubic_bezier_to(
                        lyon::geom::point(control1.x.0, control1.y.0),
                        lyon::geom::point(control2.x.0, control2.y.0),
                        lyon::geom::point(end.x.0, end.y.0),
                    );
                    _current_pos = Some(*end);
                }

                PathCommand::Close => {
                    if has_begun {
                        builder.close();
                        has_begun = false;
                        _current_pos = None;
                    }
                }

                PathCommand::AddRect(rect) => {
                    // Start new subpath for rectangle
                    if has_begun {
                        builder.end(false);
                    }
                    builder.begin(lyon::geom::point(rect.left().0, rect.top().0));
                    builder.line_to(lyon::geom::point(rect.right().0, rect.top().0));
                    builder.line_to(lyon::geom::point(rect.right().0, rect.bottom().0));
                    builder.line_to(lyon::geom::point(rect.left().0, rect.bottom().0));
                    builder.close();
                    _current_pos = None;
                    has_begun = false;
                }

                PathCommand::AddCircle(center, radius) => {
                    // Start new subpath for circle
                    if has_begun {
                        builder.end(false);
                    }
                    builder.add_circle(
                        lyon::geom::point(center.x.0, center.y.0),
                        *radius,
                        lyon::path::Winding::Positive,
                    );
                    _current_pos = None;
                    has_begun = false;
                }

                PathCommand::AddOval(rect) => {
                    // Start new subpath for oval/ellipse
                    if has_begun {
                        builder.end(false);
                    }
                    let center = rect.center();
                    let radii = lyon::geom::vector((rect.width() / 2.0).0, (rect.height() / 2.0).0);
                    builder.add_ellipse(
                        lyon::geom::point(center.x.0, center.y.0),
                        radii,
                        lyon::geom::Angle::radians(0.0),
                        lyon::path::Winding::Positive,
                    );
                    _current_pos = None;
                    has_begun = false;
                }

                PathCommand::AddArc(rect, start_angle, sweep_angle) => {
                    // Start new subpath for arc using lyon Arc primitive
                    if has_begun {
                        builder.end(false);
                    }
                    let center = rect.center();
                    let rx = (rect.width() / 2.0).0;
                    let ry = (rect.height() / 2.0).0;

                    let arc = lyon::geom::Arc {
                        center: lyon::geom::point(center.x.0, center.y.0),
                        radii: lyon::geom::vector(rx, ry),
                        start_angle: lyon::geom::Angle::radians(*start_angle),
                        sweep_angle: lyon::geom::Angle::radians(*sweep_angle),
                        x_rotation: lyon::geom::Angle::radians(0.0),
                    };

                    let arc_start = arc.from();
                    builder.begin(arc_start);
                    has_begun = true;

                    arc.for_each_cubic_bezier(&mut |cubic| {
                        builder.cubic_bezier_to(cubic.ctrl1, cubic.ctrl2, cubic.to);
                    });

                    let arc_end = arc.to();
                    _current_pos = Some(Point::new(Pixels(arc_end.x), Pixels(arc_end.y)));
                }
            }
        }

        // End the final subpath if not closed
        if has_begun {
            builder.end(false);
        }

        builder.build()
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use super::*;
    use flui_types::geometry::{Radius, rrect::RRect};

    fn px(v: f32) -> Pixels {
        Pixels(v)
    }

    #[test]
    fn test_tessellate_rect() {
        let mut tessellator = Tessellator::new();
        let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
        let paint = Paint::fill(Color::RED);

        let result = tessellator.tessellate_rect(rect, &paint);
        assert!(result.is_ok());

        let (vertices, indices) = result.expect("rect tessellation should succeed");
        assert!(!vertices.is_empty());
        assert!(!indices.is_empty());
        assert_eq!(indices.len() % 3, 0); // Should be triangles
    }

    #[test]
    fn test_tessellate_circle() {
        let mut tessellator = Tessellator::new();
        let center = Point::new(px(50.0), px(50.0));
        let paint = Paint::fill(Color::BLUE);

        let result = tessellator.tessellate_circle(center, 25.0, &paint);
        assert!(result.is_ok());

        let (vertices, indices) = result.expect("circle tessellation should succeed");
        assert!(!vertices.is_empty());
        assert!(!indices.is_empty());
    }

    #[test]
    fn test_tessellate_rounded_rect() {
        let mut tessellator = Tessellator::new();
        let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
        let paint = Paint::fill(Color::GREEN);

        let result = tessellator.tessellate_rounded_rect(rect, 10.0, &paint);
        assert!(result.is_ok());

        let (vertices, indices) = result.expect("rounded rect tessellation should succeed");
        assert!(!vertices.is_empty());
        assert!(!indices.is_empty());
    }

    #[test]
    fn test_rrect_per_corner_radii() {
        let mut tessellator = Tessellator::new();
        let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
        let paint = Paint::fill(Color::RED);

        // Create an RRect with different radii per corner
        let rrect = RRect::from_rect_and_corners(
            rect,
            Radius::circular(px(5.0)),  // top-left: small
            Radius::circular(px(15.0)), // top-right: medium
            Radius::circular(px(25.0)), // bottom-right: large
            Radius::circular(px(10.0)), // bottom-left: moderate
        );

        let result = tessellator.tessellate_rrect(rrect, &paint);
        assert!(result.is_ok());

        let (vertices, indices) = result.expect("tessellation should succeed");
        assert!(!vertices.is_empty());
        assert!(!indices.is_empty());
        assert_eq!(indices.len() % 3, 0, "indices should form triangles");

        // Also test with elliptical (non-circular) radii
        let rrect_elliptical = RRect::from_rect_and_corners(
            rect,
            Radius::elliptical(px(5.0), px(10.0)),
            Radius::elliptical(px(15.0), px(8.0)),
            Radius::elliptical(px(20.0), px(12.0)),
            Radius::elliptical(px(3.0), px(18.0)),
        );

        let result_elliptical = tessellator.tessellate_rrect(rrect_elliptical, &paint);
        assert!(result_elliptical.is_ok());

        let (verts, inds) = result_elliptical.expect("elliptical tessellation should succeed");
        assert!(!verts.is_empty());
        assert!(!inds.is_empty());
        assert_eq!(inds.len() % 3, 0, "indices should form triangles");
    }

    #[test]
    fn test_create_polyline_path() {
        let points = vec![
            Point::new(px(0.0), px(0.0)),
            Point::new(px(10.0), px(10.0)),
            Point::new(px(20.0), px(0.0)),
        ];

        let _path = Tessellator::create_polyline_path(&points, false);
        // Path should be created successfully
        // We can't easily test the internal structure, but we can verify it
        // doesn't panic
    }

    // ===== Arc tessellation tests =====

    /// Helper: creates a square bounding rect centered at (50, 50) with radius 25
    fn arc_rect() -> Rect<Pixels> {
        Rect::from_ltrb(px(25.0), px(25.0), px(75.0), px(75.0))
    }

    #[test]
    fn test_tessellate_arc_full_circle() {
        let mut tessellator = Tessellator::new();
        let rect = arc_rect();
        let paint = Paint::fill(Color::RED);

        // Full circle: sweep_angle = 2*PI
        let result = tessellator.tessellate_arc(rect, 0.0, std::f32::consts::TAU, false, &paint);
        assert!(result.is_ok(), "full circle arc should tessellate");

        let (vertices, indices) = result.expect("full circle arc tessellation should succeed");
        assert!(!vertices.is_empty(), "full circle should produce vertices");
        assert!(!indices.is_empty(), "full circle should produce indices");
        assert_eq!(indices.len() % 3, 0, "indices should form triangles");
    }

    #[test]
    fn test_tessellate_arc_semicircle() {
        let mut tessellator = Tessellator::new();
        let rect = arc_rect();
        let paint = Paint::fill(Color::GREEN);

        // Semicircle: sweep_angle = PI
        let result = tessellator.tessellate_arc(
            rect,
            0.0,
            std::f32::consts::PI,
            true, // pie slice
            &paint,
        );
        assert!(result.is_ok(), "semicircle arc should tessellate");

        let (vertices, indices) = result.expect("semicircle tessellation should succeed");
        assert!(!vertices.is_empty(), "semicircle should produce vertices");
        assert!(!indices.is_empty(), "semicircle should produce indices");
        assert_eq!(indices.len() % 3, 0, "indices should form triangles");
    }

    #[test]
    fn test_tessellate_arc_quarter_circle() {
        let mut tessellator = Tessellator::new();
        let rect = arc_rect();
        let paint = Paint::fill(Color::BLUE);

        // Quarter circle: sweep_angle = PI/2
        let result = tessellator.tessellate_arc(
            rect,
            0.0,
            std::f32::consts::FRAC_PI_2,
            true, // pie slice
            &paint,
        );
        assert!(result.is_ok(), "quarter circle arc should tessellate");

        let (vertices, indices) = result.expect("quarter circle tessellation should succeed");
        assert!(
            !vertices.is_empty(),
            "quarter circle should produce vertices"
        );
        assert!(!indices.is_empty(), "quarter circle should produce indices");
        assert_eq!(indices.len() % 3, 0, "indices should form triangles");
    }

    #[test]
    fn test_tessellate_arc_negative_sweep() {
        let mut tessellator = Tessellator::new();
        let rect = arc_rect();
        let paint = Paint::fill(Color::RED);

        // Negative sweep (clockwise arc)
        let result = tessellator.tessellate_arc(
            rect,
            std::f32::consts::PI,
            -std::f32::consts::FRAC_PI_2,
            false,
            &paint,
        );
        assert!(result.is_ok(), "negative sweep arc should tessellate");

        let (vertices, indices) = result.expect("negative sweep tessellation should succeed");
        assert!(
            !vertices.is_empty(),
            "negative sweep should produce vertices"
        );
        assert!(!indices.is_empty(), "negative sweep should produce indices");
        assert_eq!(indices.len() % 3, 0, "indices should form triangles");
    }

    #[test]
    fn test_tessellate_arc_near_zero_sweep() {
        let mut tessellator = Tessellator::new();
        let rect = arc_rect();
        let paint = Paint::fill(Color::GREEN);

        // Very small sweep (near zero) — should not panic
        let result = tessellator.tessellate_arc(rect, 0.0, 1e-8, false, &paint);
        // Near-zero sweep produces a degenerate path; tessellation may produce
        // empty geometry but must not error or panic.
        assert!(result.is_ok(), "near-zero sweep arc should not error");
    }

    #[test]
    fn test_tessellate_arc_stroke_mode() {
        let mut tessellator = Tessellator::new();
        let rect = arc_rect();
        let paint = Paint::stroke(Color::RED, 2.0);

        // Stroke-mode arc (quarter circle)
        let result =
            tessellator.tessellate_arc(rect, 0.0, std::f32::consts::FRAC_PI_2, false, &paint);
        assert!(result.is_ok(), "stroke-mode arc should tessellate");

        let (vertices, indices) = result.expect("stroke arc tessellation should succeed");
        assert!(!vertices.is_empty(), "stroke arc should produce vertices");
        assert!(!indices.is_empty(), "stroke arc should produce indices");
    }

    #[test]
    fn test_tessellate_arc_elliptical() {
        let mut tessellator = Tessellator::new();
        // Non-square bounding rect for an elliptical arc
        let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(50.0));
        let paint = Paint::fill(Color::BLUE);

        let result = tessellator.tessellate_arc(rect, 0.0, std::f32::consts::PI, true, &paint);
        assert!(result.is_ok(), "elliptical arc should tessellate");

        let (vertices, indices) = result.expect("elliptical arc tessellation should succeed");
        assert!(
            !vertices.is_empty(),
            "elliptical arc should produce vertices"
        );
        assert!(!indices.is_empty(), "elliptical arc should produce indices");
        assert_eq!(indices.len() % 3, 0, "indices should form triangles");
    }
}
