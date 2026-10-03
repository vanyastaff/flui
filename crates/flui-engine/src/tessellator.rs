//! Path tessellation using Lyon
//!
//! Converts vector paths (curves, lines, arcs) into triangle meshes
//! suitable for GPU rendering.

use flui_foundation::geometry::{Point, RRect, Rect};
use flui_painting::styling::Color;
use flui_painting::{Paint, StrokeCap, StrokeJoin};
use lyon::{
    path::{FillRule, Path},
    tessellation::{
        BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
        StrokeVertex, VertexBuffers,
    },
};
use thiserror::Error;

use crate::vertex::Vertex;

/// Finalize a contour without turning its closed on-dash seam into two caps.
fn finish_dashed_contour(
    paths: &mut Vec<Path>,
    current: &mut Option<lyon::path::BuilderWithAttributes>,
    started: &mut bool,
    contour_start: usize,
    join_seam: bool,
) {
    if *started && let Some(mut builder) = current.take() {
        builder.end(false);
        paths.push(builder.build());
    }
    *started = false;
    if !join_seam || paths.len() == contour_start {
        return;
    }
    let count = paths.len() - contour_start;
    let last = paths.pop().expect("BUG: a covered seam has a dash");
    let first = (count > 1).then(|| &paths[contour_start]);
    let mut builder = Path::builder_with_attributes(0);
    let mut begun = false;
    // Walk the final fragment through the seam into the first fragment. With
    // only one fragment the whole contour is covered and must be closed.
    for part in std::iter::once(&last).chain(first) {
        for event in part {
            match event {
                lyon::path::PathEvent::Begin { at } if !begun => {
                    builder.begin(at, &[]);
                    begun = true;
                }
                lyon::path::PathEvent::Line { to, .. } => {
                    builder.line_to(to, &[]);
                }
                _ => {}
            }
        }
    }
    builder.end(count == 1);
    let merged = builder.build();
    if count == 1 {
        paths.push(merged);
    } else {
        paths[contour_start] = merged;
    }
}

/// Device-space chord-error budget for curve flattening, in device pixels.
///
/// Mirrors Impeller's `kCircleTolerance = 0.1f` (`impeller/tessellator/
/// tessellator.h`): a curve is subdivided until its chord deviates from the
/// true arc by at most this many *device* pixels. FLUI bakes the world
/// transform into vertices after tessellation (`shape.wgsl` has no model
/// matrix), so the local-space tolerance handed to lyon must be pre-divided by
/// the transform's scale to keep the device-space error constant — see
/// [`Tessellator::set_max_scale`].
const DEVICE_FILL_TOLERANCE: f32 = 0.1;

/// Device-space chord-error budget for the dashed-stroke walker's flattening
/// pass, in device pixels. Coarser than [`DEVICE_FILL_TOLERANCE`] because dash
/// placement only needs segment endpoints, not render-quality curvature.
const DEVICE_DASH_TOLERANCE: f32 = 0.5;

/// Map a FLUI [`PathFillType`](flui_painting::paint::PathFillType) to lyon's
/// [`FillRule`]. FLUI defaults to non-zero winding; lyon's
/// `FillOptions::default()` defaults to even-odd, so this mapping must be
/// applied explicitly for every filled FLUI path.
fn fill_rule_for(fill_type: flui_painting::paint::PathFillType) -> FillRule {
    match fill_type {
        flui_painting::paint::PathFillType::NonZero => FillRule::NonZero,
        flui_painting::paint::PathFillType::EvenOdd => FillRule::EvenOdd,
    }
}

/// Errors that can occur during tessellation
///
/// `#[non_exhaustive]` future-compat marker.
#[derive(Debug, Error)]
#[non_exhaustive]
pub(crate) enum TessellationError {
    #[error("Fill tessellation failed: {0}")]
    FillFailed(String),

    #[error("Stroke tessellation failed: {0}")]
    StrokeFailed(String),
    // No `InvalidPath` variant: a workspace-wide search found no code that
    // would need to construct one. The tessellator's surface builders
    // (`Path::builder().begin(...).line_to(...).build()`) cannot produce an
    // invalid lyon `Path` through their live entry points, and the
    // similarly-named `TessellationError::InvalidPath` in `flui-painting` is
    // a separate type (different message body) used only by the
    // painting-side path builder.
}

pub(crate) type Result<T> = std::result::Result<T, TessellationError>;

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
pub(crate) struct Tessellator {
    /// Lyon fill tessellator
    fill_tessellator: FillTessellator,

    /// Lyon stroke tessellator
    stroke_tessellator: StrokeTessellator,

    /// Reusable geometry buffers
    geometry: VertexBuffers<Vertex, u32>,

    /// Maximum basis length of the world transform's 2D linear part.
    ///
    /// The painter bakes the world transform into vertices *after*
    /// tessellation, so flattening tolerances are pre-divided by this scale to
    /// keep the device-space chord error constant regardless of the on-screen
    /// magnification (HiDPI root scale, user `Transform.scale`). Set via
    /// [`Self::set_max_scale`] immediately before each tessellation call;
    /// defaults to `1.0` (identity transform).
    max_scale: f32,
}

impl Default for Tessellator {
    fn default() -> Self {
        Self {
            fill_tessellator: FillTessellator::default(),
            stroke_tessellator: StrokeTessellator::default(),
            geometry: VertexBuffers::default(),
            max_scale: 1.0,
        }
    }
}

impl Tessellator {
    /// Create a new tessellator
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Set the world-transform scale used to derive scale-aware flattening
    /// tolerances. `max_scale` is the maximum basis length of the transform's
    /// upper-left 2x2 (mirroring Impeller's `GetMaxBasisLengthXY`). The painter
    /// must call this immediately before tessellating so curves are subdivided
    /// finely enough at the magnification they will be drawn at.
    pub(crate) fn set_max_scale(&mut self, max_scale: f32) {
        // Guard against zero/NaN/negative collapsing the tolerance to infinity.
        self.max_scale = if max_scale.is_finite() && max_scale > f32::EPSILON {
            max_scale
        } else {
            1.0
        };
    }

    /// Local-space tolerance for fill/stroke flattening at the current scale.
    ///
    /// Equals the device-space budget divided by the world scale, so that after
    /// the painter bakes the transform the on-screen chord error stays at
    /// [`DEVICE_FILL_TOLERANCE`] device pixels.
    fn fill_tolerance(&self) -> f32 {
        DEVICE_FILL_TOLERANCE / self.max_scale
    }

    /// Local-space tolerance for the dashed-stroke walker at the current scale.
    fn dash_tolerance(&self) -> f32 {
        DEVICE_DASH_TOLERANCE / self.max_scale
    }

    /// Tessellate a filled path with the given fill rule.
    ///
    /// # Arguments
    /// * `path` - Lyon path to tessellate
    /// * `paint` - Paint style (color)
    /// * `fill_rule` - Winding rule. FLUI defaults to
    ///   [`FillRule::NonZero`]; only paths carrying an explicit
    ///   [`PathFillType::EvenOdd`](flui_painting::paint::PathFillType) use
    ///   even-odd. Convex shapes (circle/ellipse/arc/rrect/drrect) are unaffected
    ///   by the rule, so their callers pass the FLUI default.
    ///
    /// # Returns
    /// Tuple of (vertices, indices) ready for GPU upload
    pub(crate) fn tessellate_fill(
        &mut self,
        path: &Path,
        paint: &Paint,
        fill_rule: FillRule,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        self.geometry.vertices.clear();
        self.geometry.indices.clear();

        let options = FillOptions::default()
            .with_fill_rule(fill_rule)
            .with_tolerance(self.fill_tolerance());

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
    pub(crate) fn tessellate_stroke(
        &mut self,
        path: &Path,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        use lyon::tessellation::{LineCap, LineJoin};

        self.geometry.vertices.clear();
        self.geometry.indices.clear();

        // Extract stroke info from Paint
        let options = StrokeOptions::default()
            .with_tolerance(self.fill_tolerance())
            .with_line_width(paint.stroke_width as f32)
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

    // `tessellate_rect` and `tessellate_rounded_rect` were removed. Both
    // were forward-looking convenience wrappers that built a tiny
    // `lyon::Path` and forwarded it to `tessellate_fill`, but nothing in the
    // workspace called them: the painter draws rects through the
    // instancing path, not lyon tessellation. The two unit tests that
    // exercised them were deleted alongside the methods.

    /// Tessellates `path` as a fill or as an outline according to
    /// `paint.style`.
    ///
    /// Every shape helper that accepts a whole [`Paint`] routes through
    /// here. Filling unconditionally is not a lesser approximation of a
    /// stroke, it is a different shape: a stroked circle asked for a ring
    /// and a fill hands back a disc in the stroke colour, which covers
    /// whatever it was supposed to outline. The batch layer decides
    /// *whether* a shape is tessellated at all (instanced SDF paths take
    /// filled `SrcOver` shapes); once it has decided to tessellate, the
    /// style must survive the trip.
    fn dispatch_on_style(&mut self, path: &Path, paint: &Paint) -> Result<(Vec<Vertex>, Vec<u32>)> {
        if paint.style == flui_painting::PaintStyle::Fill {
            self.tessellate_fill(path, paint, FillRule::NonZero)
        } else {
            self.tessellate_stroke(path, paint)
        }
    }

    /// Tessellate a circle
    pub(crate) fn tessellate_circle(
        &mut self,
        center: Point<f64>,
        radius: f32,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.add_circle(
            lyon::geom::point(center.x as f32, center.y as f32),
            radius,
            lyon::path::Winding::Positive,
        );

        let path = path_builder.build();
        // Convex shape: fill rule is moot; pass the FLUI default.
        self.dispatch_on_style(&path, paint)
    }

    /// Tessellate an ellipse
    pub(crate) fn tessellate_ellipse(
        &mut self,
        center: Point<f64>,
        radii: Point<f64>,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.add_ellipse(
            lyon::geom::point(center.x as f32, center.y as f32),
            lyon::geom::vector(radii.x as f32, radii.y as f32),
            lyon::geom::Angle::radians(0.0),
            lyon::path::Winding::Positive,
        );

        let path = path_builder.build();
        // Convex shape: fill rule is moot; pass the FLUI default.
        self.dispatch_on_style(&path, paint)
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
    pub(crate) fn tessellate_arc(
        &mut self,
        rect: Rect<f64>,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        let center = rect.center();
        let rx = rect.width() / 2.0;
        let ry = rect.height() / 2.0;

        // Handle near-zero sweep: emit a degenerate path (just the start point)
        if sweep_angle.abs() < 1e-6 {
            let start_x = center.x + rx * f64::from(start_angle.cos());
            let start_y = center.y + ry * f64::from(start_angle.sin());
            path_builder.begin(lyon::geom::point(start_x as f32, start_y as f32));
            path_builder.end(false);
            let path = path_builder.build();
            return if paint.style == flui_painting::PaintStyle::Fill {
                self.tessellate_fill(&path, paint, FillRule::NonZero)
            } else {
                self.tessellate_stroke(&path, paint)
            };
        }

        // Build a lyon Arc and convert to cubic Bezier curves
        let arc = lyon::geom::Arc {
            center: lyon::geom::point(center.x as f32, center.y as f32),
            radii: lyon::geom::vector(rx as f32, ry as f32),
            start_angle: lyon::geom::Angle::radians(start_angle),
            sweep_angle: lyon::geom::Angle::radians(sweep_angle),
            x_rotation: lyon::geom::Angle::radians(0.0),
        };

        let arc_start = arc.from();

        if use_center {
            // Pie slice: start from center, line to arc start
            path_builder.begin(lyon::geom::point(center.x as f32, center.y as f32));
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
            path_builder.line_to(lyon::geom::point(center.x as f32, center.y as f32));
            path_builder.close();
        } else {
            path_builder.end(false);
        }

        let path = path_builder.build();

        // Use fill or stroke based on paint style
        if paint.style == flui_painting::PaintStyle::Fill {
            // Convex pie/arc segment: fill rule is moot; pass the FLUI default.
            self.tessellate_fill(&path, paint, FillRule::NonZero)
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
    #[expect(clippy::similar_names)] // tl_x/tl_y, tr_x/tr_y, etc. are intentional corner names
    pub(crate) fn tessellate_drrect(
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
                    builder.begin(lyon::geom::point((left + tl_x) as f32, top as f32));

                    // Top edge to top-right corner
                    builder.line_to(lyon::geom::point((right - tr_x) as f32, top as f32));
                    // Top-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right as f32, top as f32),
                        lyon::geom::point(right as f32, (top + tr_y) as f32),
                    );

                    // Right edge to bottom-right corner
                    builder.line_to(lyon::geom::point(right as f32, (bottom - br_y) as f32));
                    // Bottom-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right as f32, bottom as f32),
                        lyon::geom::point((right - br_x) as f32, bottom as f32),
                    );

                    // Bottom edge to bottom-left corner
                    builder.line_to(lyon::geom::point((left + bl_x) as f32, bottom as f32));
                    // Bottom-left corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left as f32, bottom as f32),
                        lyon::geom::point(left as f32, (bottom - bl_y) as f32),
                    );

                    // Left edge to top-left corner
                    builder.line_to(lyon::geom::point(left as f32, (top + tl_y) as f32));
                    // Top-left corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left as f32, top as f32),
                        lyon::geom::point((left + tl_x) as f32, top as f32),
                    );

                    builder.close();
                }
                lyon::path::Winding::Negative => {
                    // Counter-clockwise: top-left -> bottom-left -> bottom-right -> top-right
                    builder.begin(lyon::geom::point((left + tl_x) as f32, top as f32));

                    // Top-left corner (reverse)
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left as f32, top as f32),
                        lyon::geom::point(left as f32, (top + tl_y) as f32),
                    );

                    // Left edge to bottom-left corner
                    builder.line_to(lyon::geom::point(left as f32, (bottom - bl_y) as f32));
                    // Bottom-left corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(left as f32, bottom as f32),
                        lyon::geom::point((left + bl_x) as f32, bottom as f32),
                    );

                    // Bottom edge to bottom-right corner
                    builder.line_to(lyon::geom::point((right - br_x) as f32, bottom as f32));
                    // Bottom-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right as f32, bottom as f32),
                        lyon::geom::point(right as f32, (bottom - br_y) as f32),
                    );

                    // Right edge to top-right corner
                    builder.line_to(lyon::geom::point(right as f32, (top + tr_y) as f32));
                    // Top-right corner
                    builder.quadratic_bezier_to(
                        lyon::geom::point(right as f32, top as f32),
                        lyon::geom::point((right - tr_x) as f32, top as f32),
                    );

                    // Top edge back to start
                    builder.line_to(lyon::geom::point((left + tl_x) as f32, top as f32));

                    builder.close();
                }
            }
        };

        // Add outer RRect with positive winding (filled)
        add_rrect(&mut path_builder, outer, lyon::path::Winding::Positive);

        // Add inner RRect with negative winding (cutout)
        add_rrect(&mut path_builder, inner, lyon::path::Winding::Negative);

        let path = path_builder.build();
        // The cutout is built from opposite windings, so either fill rule rings
        // the inner region correctly; use NonZero to match the FLUI
        // default.
        self.tessellate_fill(&path, paint, FillRule::NonZero)
    }

    /// Create a lyon path from points (polyline)
    pub(crate) fn create_polyline_path(points: &[Point<f64>], closed: bool) -> Path {
        if points.is_empty() {
            return Path::builder().build();
        }

        let mut path_builder = Path::builder();

        path_builder.begin(lyon::geom::point(points[0].x as f32, points[0].y as f32));

        for point in &points[1..] {
            path_builder.line_to(lyon::geom::point(point.x as f32, point.y as f32));
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
    pub(crate) fn tessellate_rrect(
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
        path_builder.begin(lyon::geom::point((left + tl_x) as f32, top as f32));

        // Top edge to top-right corner
        path_builder.line_to(lyon::geom::point((right - tr_x) as f32, top as f32));
        // Top-right corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(right as f32, top as f32),
            lyon::geom::point(right as f32, (top + tr_y) as f32),
        );

        // Right edge to bottom-right corner
        path_builder.line_to(lyon::geom::point(right as f32, (bottom - br_y) as f32));
        // Bottom-right corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(right as f32, bottom as f32),
            lyon::geom::point((right - br_x) as f32, bottom as f32),
        );

        // Bottom edge to bottom-left corner
        path_builder.line_to(lyon::geom::point((left + bl_x) as f32, bottom as f32));
        // Bottom-left corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(left as f32, bottom as f32),
            lyon::geom::point(left as f32, (bottom - bl_y) as f32),
        );

        // Left edge to top-left corner
        path_builder.line_to(lyon::geom::point(left as f32, (top + tl_y) as f32));
        // Top-left corner
        path_builder.quadratic_bezier_to(
            lyon::geom::point(left as f32, top as f32),
            lyon::geom::point((left + tl_x) as f32, top as f32),
        );

        path_builder.close();

        let path = path_builder.build();
        // Convex rounded rect: fill rule is moot; pass the FLUI default.
        self.dispatch_on_style(&path, paint)
    }

    /// Tessellate a stroked rectangle
    pub(crate) fn tessellate_rect_stroke(
        &mut self,
        rect: Rect<f64>,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let mut path_builder = Path::builder();

        path_builder.begin(lyon::geom::point(rect.left() as f32, rect.top() as f32));
        path_builder.line_to(lyon::geom::point(rect.right() as f32, rect.top() as f32));
        path_builder.line_to(lyon::geom::point(rect.right() as f32, rect.bottom() as f32));
        path_builder.line_to(lyon::geom::point(rect.left() as f32, rect.bottom() as f32));
        path_builder.close();

        let path = path_builder.build();
        self.tessellate_stroke(&path, paint)
    }

    /// Tessellate a line
    pub(crate) fn tessellate_line(
        &mut self,
        p1: Point<f64>,
        p2: Point<f64>,
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

    /// Tessellate a FLUI Path (filled), honoring the path's own fill rule.
    ///
    /// Unlike the convex-shape helpers, an arbitrary FLUI path can
    /// self-intersect or overlap same-winding subpaths, so the winding rule is
    /// observable: `PathFillType::NonZero` (the FLUI default) fills overlaps
    /// solid, `EvenOdd` punches holes. This is the only fill entry point that
    /// reads [`flui_painting::paint::path::Path::fill_type`].
    pub(crate) fn tessellate_flui_path_fill(
        &mut self,
        flui_path: &flui_painting::paint::path::Path,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let lyon_path = flui_path.to_lyon_path();
        self.tessellate_fill(&lyon_path, paint, fill_rule_for(flui_path.fill_type()))
    }

    /// Tessellate a FLUI Path (stroked)
    pub(crate) fn tessellate_flui_path_stroke(
        &mut self,
        flui_path: &flui_painting::paint::path::Path,
        paint: &Paint,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let lyon_path = flui_path.to_lyon_path();
        self.tessellate_stroke(&lyon_path, paint)
    }

    /// Tessellate a FLUI Path with a dash pattern (stroked).
    ///
    /// Converts the FLUI path to a Lyon path then delegates to
    /// [`Self::tessellate_dashed_stroke`].  The caller is responsible for
    /// verifying that `dash_pattern` is valid before calling this method;
    /// an invalid pattern falls back to a solid stroke.
    pub(crate) fn tessellate_flui_path_dashed_stroke(
        &mut self,
        flui_path: &flui_painting::paint::path::Path,
        paint: &Paint,
        dash_pattern: &flui_painting::paint::DashPattern,
    ) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let lyon_path = flui_path.to_lyon_path();
        self.tessellate_dashed_stroke(&lyon_path, paint, dash_pattern)
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
    pub(crate) fn tessellate_dashed_stroke(
        &mut self,
        path: &Path,
        paint: &Paint,
        dash_pattern: &flui_painting::paint::DashPattern,
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
        // Dash intervals are logical f64; the dasher walks the f32 lyon path.
        let effective_intervals: Vec<f32> = if intervals.len().is_multiple_of(2) {
            intervals.iter().map(|&v| v as f32).collect()
        } else {
            intervals
                .iter()
                .chain(intervals.iter())
                .map(|&v| v as f32)
                .collect()
        };

        let cycle_length: f32 = effective_intervals.iter().sum();
        if cycle_length <= 0.0 {
            return self.tessellate_stroke(path, paint);
        }

        // Walk the segments and generate dash sub-paths
        let mut dash_paths: Vec<Path> = Vec::new();
        let mut phase = dash_pattern.phase as f32 % cycle_length;
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
        let is_drawing = interval_idx.is_multiple_of(2); // Even indices are dashes, odd are gaps

        let mut drawing = is_drawing;
        let mut remaining = remaining_in_interval;

        let mut current_builder: Option<lyon::path::BuilderWithAttributes> = None;
        if drawing {
            current_builder = Some(Path::builder_with_attributes(0));
        }
        let mut started_subpath = false;
        let mut contour_start = 0;
        let mut starts_on_dash = false;
        let mut ends_on_dash = false;

        // Lyon lazily flattens curves while preserving contour events. Keep
        // those events: End carries the implicit closing edge, and Begin must
        // terminate a dash before a disconnected contour starts. Dash phase
        // continues across contours, counting only their travelled lengths.
        for event in path.iter().flattened(self.dash_tolerance()) {
            let (from, to, closed) = match event {
                PathEvent::Begin { .. } => {
                    contour_start = dash_paths.len();
                    starts_on_dash = drawing;
                    ends_on_dash = false;
                    current_builder = drawing.then(|| Path::builder_with_attributes(0));
                    continue;
                }
                PathEvent::Line { from, to } => (from, to, false),
                PathEvent::End {
                    last,
                    first,
                    close: true,
                } => (last, first, true),
                PathEvent::End { close: false, .. } => {
                    finish_dashed_contour(
                        &mut dash_paths,
                        &mut current_builder,
                        &mut started_subpath,
                        contour_start,
                        false,
                    );
                    continue;
                }
                PathEvent::Quadratic { .. } | PathEvent::Cubic { .. } => continue,
            };
            let dx = to.x - from.x;
            let dy = to.y - from.y;
            let seg_length = dx.hypot(dy);
            if !seg_length.is_finite() {
                return Err(TessellationError::StrokeFailed(
                    "dashed contour has non-finite segment length".to_owned(),
                ));
            }
            if seg_length >= f32::EPSILON {
                let dir_x = dx / seg_length;
                let dir_y = dy / seg_length;

                let mut offset = 0.0f32;

                while offset < seg_length {
                    let available = seg_length - offset;
                    let consume = remaining.min(available);
                    let next_offset = offset + consume;
                    // A positive interval can still round back to the old offset.
                    // Refuse before emitting geometry so the whole stroke is absent.
                    if !next_offset.is_finite() || next_offset <= offset {
                        return Err(TessellationError::StrokeFailed(
                            "dashed interval does not advance at raster precision".to_owned(),
                        ));
                    }

                    let start_x = from.x + dir_x * offset;
                    let start_y = from.y + dir_y * offset;
                    let end_x = from.x + dir_x * next_offset;
                    let end_y = from.y + dir_y * next_offset;

                    ends_on_dash = drawing;
                    if drawing && let Some(ref mut builder) = current_builder {
                        if !started_subpath {
                            builder.begin(lyon::geom::point(start_x, start_y), &[]);
                            started_subpath = true;
                        }
                        builder.line_to(lyon::geom::point(end_x, end_y), &[]);
                    }

                    remaining -= consume;
                    offset = next_offset;

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
                        drawing = interval_idx.is_multiple_of(2);
                        remaining = effective_intervals[interval_idx];
                        if drawing {
                            current_builder = Some(Path::builder_with_attributes(0));
                        } else {
                            current_builder = None;
                        }
                    }
                }
            }
            if closed {
                finish_dashed_contour(
                    &mut dash_paths,
                    &mut current_builder,
                    &mut started_subpath,
                    contour_start,
                    starts_on_dash && ends_on_dash,
                );
            }
        }

        // Now tessellate all dash sub-paths and combine the geometry
        let options = StrokeOptions::default()
            .with_tolerance(self.fill_tolerance())
            .with_line_width(paint.stroke_width as f32)
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
            let base_vertex = all_vertices.len() as u32;
            all_vertices.extend_from_slice(&self.geometry.vertices);
            all_indices.extend(self.geometry.indices.iter().map(|i| i + base_vertex));
        }

        Ok((all_vertices, all_indices))
    }
}

/// Helper trait for creating lyon paths from FLUI types
pub(crate) trait IntoLyonPath {
    /// Convert to lyon path
    fn to_lyon_path(&self) -> Path;
}

impl IntoLyonPath for Rect<f64> {
    fn to_lyon_path(&self) -> Path {
        let mut builder = Path::builder();

        builder.begin(lyon::geom::point(self.left() as f32, self.top() as f32));
        builder.line_to(lyon::geom::point(self.right() as f32, self.top() as f32));
        builder.line_to(lyon::geom::point(self.right() as f32, self.bottom() as f32));
        builder.line_to(lyon::geom::point(self.left() as f32, self.bottom() as f32));
        builder.close();

        builder.build()
    }
}

impl IntoLyonPath for flui_painting::paint::path::Path {
    /// The one kurbo-to-lyon adapter (ADR-0098 §7): a FLUI path's elements, whose curves
    /// kurbo built, become a lyon path. It is also the one narrowing point from logical `f64`
    /// path geometry to lyon's `f32` (ADR-0098 §2).
    fn to_lyon_path(&self) -> Path {
        use flui_painting::paint::path::PathCommand;

        let lyon_point = |p: Point<f64>| lyon::geom::point(p.x as f32, p.y as f32);

        let mut builder = Path::builder();
        let mut has_begun = false;
        // `flui_painting::paint::Path` starts every contour with a `MoveTo`; the pen only
        // matters if a segment ever arrives without one, and then it starts where Skia's pen
        // would: the origin, or the start of the contour just closed.
        let mut pen = lyon::geom::point(0.0_f32, 0.0);
        let mut contour_start = pen;
        let begin_at_pen = |builder: &mut lyon::path::path::Builder,
                            has_begun: &mut bool,
                            pen: lyon::math::Point,
                            contour_start: &mut lyon::math::Point| {
            if !*has_begun {
                *contour_start = pen;
                builder.begin(pen);
                *has_begun = true;
            }
        };

        for command in self.commands() {
            match command {
                PathCommand::MoveTo(point) => {
                    if has_begun {
                        builder.end(false);
                    }
                    pen = lyon_point(point);
                    contour_start = pen;
                    builder.begin(pen);
                    has_begun = true;
                }
                PathCommand::LineTo(point) => {
                    begin_at_pen(&mut builder, &mut has_begun, pen, &mut contour_start);
                    pen = lyon_point(point);
                    builder.line_to(pen);
                }
                PathCommand::QuadraticTo(control, end) => {
                    begin_at_pen(&mut builder, &mut has_begun, pen, &mut contour_start);
                    pen = lyon_point(end);
                    builder.quadratic_bezier_to(lyon_point(control), pen);
                }
                PathCommand::CubicTo(control1, control2, end) => {
                    begin_at_pen(&mut builder, &mut has_begun, pen, &mut contour_start);
                    pen = lyon_point(end);
                    builder.cubic_bezier_to(lyon_point(control1), lyon_point(control2), pen);
                }
                PathCommand::Close => {
                    if has_begun {
                        builder.close();
                        has_begun = false;
                    }
                    pen = contour_start;
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
