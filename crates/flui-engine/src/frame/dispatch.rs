//! Draw command dispatch from render commands to GPU pipelines.
//!
//! This module routes [`DrawCommand`] variants to the appropriate batcher
//! and traverses the [`Scene`]'s layer tree, dispatching all commands
//! encountered during traversal.
//!
//! This is CPU-side logic only -- it operates on references to batchers
//! and state stacks, with no GPU code.

use flui_foundation::LayerId;
use flui_layer::tree::LayerTree;
use flui_layer::{Layer, Scene};
use flui_painting::display_list::{DisplayListCore, DrawCommand};
use flui_types::geometry::{Matrix4, Pixels, RRect, Rect};
use flui_types::painting::{Paint, Path, PathCommand, PointMode, Shader};
use flui_types::styling::Color;
use flui_types::typography::FontWeight;

use crate::batchers::compositing::{CompositingBatcher, FilterType};
use crate::batchers::effects::{
    EffectBatcher, GradientStop, LinearGradientInstance, RadialGradientInstance, ShadowInstance,
    SweepGradientInstance,
};
use crate::batchers::images::ImageBatcher;
use crate::batchers::paths::PathBatcher;
use crate::batchers::shapes::ShapeBatcher;
use crate::batchers::text::TextBatcher;
use crate::frame::state_stack::{ClipRect, StateStack};
use crate::frame::submission::{BatcherSnapshot, DrawOp, ScissorRect};
use crate::text::TextCacheKey;

// ===========================================================================
// Batchers collection
// ===========================================================================

/// Collection of all batchers used during frame dispatch.
///
/// Each batcher accumulates instances for a specific primitive type.
/// After traversal, the batchers are consumed by the GPU submission step.
pub struct Batchers {
    /// Rectangle, circle, oval, arc, and line batching.
    pub shapes: ShapeBatcher,
    /// Tessellated vector path batching.
    pub paths: PathBatcher,
    /// Glyph/text run batching.
    pub text: TextBatcher,
    /// Image quad batching grouped by texture ID.
    pub images: ImageBatcher,
    /// Gradient, shadow, and blur effect batching.
    pub effects: EffectBatcher,
    /// Offscreen target and filter operation batching.
    pub compositing: CompositingBatcher,
}

impl Batchers {
    /// Create a new collection with pre-allocated capacity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            shapes: ShapeBatcher::new(),
            paths: PathBatcher::new(),
            text: TextBatcher::new(),
            images: ImageBatcher::new(),
            effects: EffectBatcher::new(),
            compositing: CompositingBatcher::new(),
        }
    }

    /// Clear all batchers, keeping allocated memory for reuse.
    pub fn clear_all(&mut self) {
        self.shapes.clear();
        self.paths.clear();
        self.text.clear();
        self.images.clear();
        self.effects.clear();
        self.compositing.clear();
    }

    /// Take a snapshot of all batcher counts at this point in time.
    ///
    /// Used together with a second snapshot after dispatching a leaf layer's
    /// commands to compute the per-layer instance ranges.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn snapshot(&self) -> BatcherSnapshot {
        BatcherSnapshot {
            rects: self.shapes.rect_count() as u32,
            circles: self.shapes.circle_count() as u32,
            arcs: self.shapes.arc_count() as u32,
            shadows: self.effects.shadow_count() as u32,
            path_draw_ranges: self.paths.draw_range_count() as u32,
            linear_gradients: self.effects.linear_gradient_count() as u32,
            radial_gradients: self.effects.radial_gradient_count() as u32,
            sweep_gradients: self.effects.sweep_gradient_count() as u32,
            text_runs: self.text.run_count() as u32,
            images: self.images.total_instance_count() as u32,
        }
    }

    /// Returns `true` if every batcher is empty.
    #[must_use]
    pub fn is_all_empty(&self) -> bool {
        self.shapes.is_empty()
            && self.paths.is_empty()
            && self.text.is_empty()
            && self.images.is_empty()
            && self.effects.is_empty()
            && self.compositing.is_empty()
    }
}

impl Default for Batchers {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Helper functions
// ===========================================================================

/// Convert a [`Paint`]'s color to an `[f32; 4]` RGBA array.
fn paint_to_color(paint: &Paint) -> [f32; 4] {
    color_to_array(&paint.color)
}

/// Convert a [`Color`] (u8 channels) to an `[f32; 4]` RGBA array.
fn color_to_array(color: &Color) -> [f32; 4] {
    [
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        color.a as f32 / 255.0,
    ]
}

/// Extract a 2D affine transform from a [`Matrix4`] as `[scale_x, skew_x, skew_y, scale_y]`.
///
/// Currently returns identity -- full extraction will be added when
/// the rendering pipeline consumes non-identity transforms.
fn extract_transform_2d(_m: &Matrix4) -> [f32; 4] {
    // Matrix4 is column-major [col0..col3] = 16 floats.
    // 2D affine: scale_x = m[0], skew_x = m[4], skew_y = m[1], scale_y = m[5]
    // Placeholder identity for now.
    [1.0, 0.0, 0.0, 1.0]
}

/// Extract per-corner radii from an [`RRect`] as `[tl, tr, br, bl]`.
///
/// Uses the *x* component of each corner radius (assumes circular corners
/// for the GPU shader).
fn rrect_corner_radii(rrect: &RRect) -> [f32; 4] {
    [
        rrect.top_left.x.get(),
        rrect.top_right.x.get(),
        rrect.bottom_right.x.get(),
        rrect.bottom_left.x.get(),
    ]
}

/// Helper to extract `[x, y, w, h]` from a `Rect<Pixels>`.
fn rect_to_xywh(r: &Rect<Pixels>) -> [f32; 4] {
    [
        r.left().get(),
        r.top().get(),
        r.width().get(),
        r.height().get(),
    ]
}

// ===========================================================================
// Gradient dispatch helper
// ===========================================================================

/// Convert a slice of [`Color`] values and optional stop positions into GPU
/// [`GradientStop`] instances with evenly distributed positions when no
/// explicit stops are provided.
fn build_gradient_stops(colors: &[Color], stops: Option<&Vec<f32>>) -> Vec<GradientStop> {
    let count = colors.len();
    colors
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let position = stops.and_then(|s| s.get(i).copied()).unwrap_or_else(|| {
                if count <= 1 {
                    0.0
                } else {
                    i as f32 / (count - 1) as f32
                }
            });
            GradientStop {
                color: color_to_array(c),
                position,
                _padding: [0.0; 3],
            }
        })
        .collect()
}

/// Dispatch a [`Shader`] variant to the appropriate gradient batcher.
///
/// Handles `LinearGradient`, `RadialGradient`, and `SweepGradient` (mapped to
/// radial as a fallback). `Image` shaders are ignored with a debug log.
fn dispatch_gradient(
    batchers: &mut Batchers,
    shader: &Shader,
    bounds: [f32; 4],
    corner_radii: [f32; 4],
    transform: [f32; 4],
) {
    match shader {
        Shader::LinearGradient {
            from,
            to,
            colors,
            stops,
            tile_mode: _,
        } => {
            let gradient_stops = build_gradient_stops(colors, stops.as_ref());
            batchers
                .effects
                .add_linear_gradient(LinearGradientInstance {
                    bounds,
                    start: [from.dx.get(), from.dy.get()],
                    end: [to.dx.get(), to.dy.get()],
                    stops: gradient_stops,
                    corner_radii,
                    transform,
                });
        }
        Shader::RadialGradient {
            center,
            radius,
            colors,
            stops,
            tile_mode: _,
            focal: _,
            focal_radius: _,
        } => {
            let gradient_stops = build_gradient_stops(colors, stops.as_ref());
            batchers
                .effects
                .add_radial_gradient(RadialGradientInstance {
                    bounds,
                    center: [center.dx.get(), center.dy.get()],
                    radius: *radius,
                    stops: gradient_stops,
                    corner_radii,
                    transform,
                });
        }
        Shader::SweepGradient {
            center,
            colors,
            stops,
            start_angle,
            end_angle,
            ..
        } => {
            let gradient_stops = build_gradient_stops(colors, stops.as_ref());
            batchers.effects.add_sweep_gradient(SweepGradientInstance {
                bounds,
                center: [center.dx.get(), center.dy.get()],
                start_angle: *start_angle,
                end_angle: *end_angle,
                stops: gradient_stops,
                corner_radii,
                transform,
            });
        }
        Shader::Image(_) | _ => {
            tracing::debug!("DrawGradient: unsupported shader variant, skipping");
        }
    }
}

// ===========================================================================
// Path conversion helpers
// ===========================================================================

/// Convert a `flui_types::painting::Path` to a `lyon::path::Path`.
///
/// Handles all [`PathCommand`] variants by mapping to the lyon path builder API.
/// Returns `None` if the path is empty.
#[allow(unused_assignments)] // has_open_subpath tracking is intentional across branches
fn convert_path_to_lyon(path: &Path) -> Option<lyon::path::Path> {
    let commands = path.commands();
    if commands.is_empty() {
        return None;
    }

    let mut builder = lyon::path::Path::builder();
    let mut has_open_subpath = false;

    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo(p) => {
                if has_open_subpath {
                    builder.end(false);
                }
                builder.begin(lyon::math::point(p.x.get(), p.y.get()));
                has_open_subpath = true;
            }
            PathCommand::LineTo(p) => {
                if !has_open_subpath {
                    builder.begin(lyon::math::point(0.0, 0.0));
                    has_open_subpath = true;
                }
                builder.line_to(lyon::math::point(p.x.get(), p.y.get()));
            }
            PathCommand::QuadraticTo(cp, ep) => {
                if !has_open_subpath {
                    builder.begin(lyon::math::point(0.0, 0.0));
                    has_open_subpath = true;
                }
                builder.quadratic_bezier_to(
                    lyon::math::point(cp.x.get(), cp.y.get()),
                    lyon::math::point(ep.x.get(), ep.y.get()),
                );
            }
            PathCommand::CubicTo(c1, c2, ep) => {
                if !has_open_subpath {
                    builder.begin(lyon::math::point(0.0, 0.0));
                    has_open_subpath = true;
                }
                builder.cubic_bezier_to(
                    lyon::math::point(c1.x.get(), c1.y.get()),
                    lyon::math::point(c2.x.get(), c2.y.get()),
                    lyon::math::point(ep.x.get(), ep.y.get()),
                );
            }
            PathCommand::Close => {
                if has_open_subpath {
                    builder.close();
                    has_open_subpath = false;
                }
            }
            PathCommand::AddRect(rect) => {
                if has_open_subpath {
                    builder.end(false);
                    has_open_subpath = false;
                }
                let l = rect.left().get();
                let t = rect.top().get();
                let r = rect.right().get();
                let b = rect.bottom().get();
                builder.begin(lyon::math::point(l, t));
                has_open_subpath = true;
                builder.line_to(lyon::math::point(r, t));
                builder.line_to(lyon::math::point(r, b));
                builder.line_to(lyon::math::point(l, b));
                builder.close();
                has_open_subpath = false;
            }
            PathCommand::AddCircle(center, radius) => {
                if has_open_subpath {
                    builder.end(false);
                    has_open_subpath = false;
                }
                // Approximate circle with 4 cubic bezier segments
                let cx = center.x.get();
                let cy = center.y.get();
                // Control point offset for circular arc approximation
                let k = radius * 0.552_284_749_8; // (4/3)(sqrt(2)-1)
                builder.begin(lyon::math::point(cx + radius, cy));
                has_open_subpath = true;
                builder.cubic_bezier_to(
                    lyon::math::point(cx + radius, cy + k),
                    lyon::math::point(cx + k, cy + radius),
                    lyon::math::point(cx, cy + radius),
                );
                builder.cubic_bezier_to(
                    lyon::math::point(cx - k, cy + radius),
                    lyon::math::point(cx - radius, cy + k),
                    lyon::math::point(cx - radius, cy),
                );
                builder.cubic_bezier_to(
                    lyon::math::point(cx - radius, cy - k),
                    lyon::math::point(cx - k, cy - radius),
                    lyon::math::point(cx, cy - radius),
                );
                builder.cubic_bezier_to(
                    lyon::math::point(cx + k, cy - radius),
                    lyon::math::point(cx + radius, cy - k),
                    lyon::math::point(cx + radius, cy),
                );
                builder.close();
                has_open_subpath = false;
            }
            PathCommand::AddOval(rect) => {
                if has_open_subpath {
                    builder.end(false);
                    has_open_subpath = false;
                }
                let cx = (rect.left().get() + rect.right().get()) * 0.5;
                let cy = (rect.top().get() + rect.bottom().get()) * 0.5;
                let rx = rect.width().get() * 0.5;
                let ry = rect.height().get() * 0.5;
                let kx = rx * 0.552_284_749_8;
                let ky = ry * 0.552_284_749_8;
                builder.begin(lyon::math::point(cx + rx, cy));
                has_open_subpath = true;
                builder.cubic_bezier_to(
                    lyon::math::point(cx + rx, cy + ky),
                    lyon::math::point(cx + kx, cy + ry),
                    lyon::math::point(cx, cy + ry),
                );
                builder.cubic_bezier_to(
                    lyon::math::point(cx - kx, cy + ry),
                    lyon::math::point(cx - rx, cy + ky),
                    lyon::math::point(cx - rx, cy),
                );
                builder.cubic_bezier_to(
                    lyon::math::point(cx - rx, cy - ky),
                    lyon::math::point(cx - kx, cy - ry),
                    lyon::math::point(cx, cy - ry),
                );
                builder.cubic_bezier_to(
                    lyon::math::point(cx + kx, cy - ry),
                    lyon::math::point(cx + rx, cy - ky),
                    lyon::math::point(cx + rx, cy),
                );
                builder.close();
                has_open_subpath = false;
            }
            PathCommand::AddArc(rect, start_angle, sweep_angle) => {
                if has_open_subpath {
                    builder.end(false);
                    has_open_subpath = false;
                }
                // Approximate arc with line segments
                let cx = (rect.left().get() + rect.right().get()) * 0.5;
                let cy = (rect.top().get() + rect.bottom().get()) * 0.5;
                let rx = rect.width().get() * 0.5;
                let ry = rect.height().get() * 0.5;
                let segments = 32u32;
                for i in 0..=segments {
                    let t = start_angle + sweep_angle * (i as f32 / segments as f32);
                    let px = cx + rx * t.cos();
                    let py = cy + ry * t.sin();
                    if i == 0 {
                        builder.begin(lyon::math::point(px, py));
                        has_open_subpath = true;
                    } else {
                        builder.line_to(lyon::math::point(px, py));
                    }
                }
                if has_open_subpath {
                    builder.end(false);
                    has_open_subpath = false;
                }
            }
        }
    }

    if has_open_subpath {
        builder.end(false);
    }

    Some(builder.build())
}

/// Convert an [`RRect`] to a `lyon::path::Path` for tessellation.
fn rrect_to_lyon_path(rrect: &RRect) -> lyon::path::Path {
    let flui_path = Path::from_rrect(*rrect);
    // from_rrect always produces a non-empty path
    convert_path_to_lyon(&flui_path).unwrap_or_else(|| {
        // Fallback: simple rectangle
        let mut builder = lyon::path::Path::builder();
        let l = rrect.rect.left().get();
        let t = rrect.rect.top().get();
        let r = rrect.rect.right().get();
        let b = rrect.rect.bottom().get();
        builder.begin(lyon::math::point(l, t));
        builder.line_to(lyon::math::point(r, t));
        builder.line_to(lyon::math::point(r, b));
        builder.line_to(lyon::math::point(l, b));
        builder.close();
        builder.build()
    })
}

// ===========================================================================
// Command dispatch
// ===========================================================================

/// Route a single [`DrawCommand`] to the appropriate batcher.
///
/// This is an exhaustive match -- every variant is handled. Variants that
/// require tessellation or shader decomposition log a debug/warn message
/// and are skipped for now.
#[allow(unused_variables)] // transform / path params not yet consumed
pub fn dispatch_command(
    cmd: &DrawCommand,
    batchers: &mut Batchers,
    state: &mut StateStack,
    _scale_factor: f32,
) {
    match cmd {
        // =================================================================
        // Primitives -> ShapeBatcher
        // =================================================================
        DrawCommand::DrawRect {
            rect,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let t = extract_transform_2d(transform);
            batchers.shapes.add_rect(
                rect.left().get(),
                rect.top().get(),
                rect.width().get(),
                rect.height().get(),
                color,
                [0.0; 4],
                t,
            );
        }

        DrawCommand::DrawRRect {
            rrect,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let t = extract_transform_2d(transform);
            let radii = rrect_corner_radii(rrect);
            batchers.shapes.add_rect(
                rrect.rect.left().get(),
                rrect.rect.top().get(),
                rrect.rect.width().get(),
                rrect.rect.height().get(),
                color,
                radii,
                t,
            );
        }

        DrawCommand::DrawCircle {
            center,
            radius,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let t = extract_transform_2d(transform);
            batchers
                .shapes
                .add_circle(center.x.get(), center.y.get(), radius.get(), color, t);
        }

        DrawCommand::DrawOval {
            rect,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let t = extract_transform_2d(transform);
            batchers.shapes.add_oval(
                rect.left().get(),
                rect.top().get(),
                rect.width().get(),
                rect.height().get(),
                color,
                t,
            );
        }

        DrawCommand::DrawLine {
            p1,
            p2,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let width = paint.stroke_width.max(1.0);
            batchers
                .shapes
                .add_line(p1.x.get(), p1.y.get(), p2.x.get(), p2.y.get(), color, width);
        }

        DrawCommand::DrawArc {
            rect,
            start_angle,
            sweep_angle,
            use_center: _,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let cx = rect.left().get() + rect.width().get() / 2.0;
            let cy = rect.top().get() + rect.height().get() / 2.0;
            let r = rect.width().get().min(rect.height().get()) / 2.0;
            batchers
                .shapes
                .add_arc(cx, cy, r, *start_angle, *sweep_angle, color);
        }

        // =================================================================
        // Path primitives -> PathBatcher (pending tessellation)
        // =================================================================
        DrawCommand::DrawPath {
            path,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            if let Some(lyon_path) = convert_path_to_lyon(path) {
                if paint.is_fill() {
                    batchers.paths.add_fill(&lyon_path, color);
                } else {
                    let width = paint.stroke_width.max(1.0);
                    batchers.paths.add_stroke(&lyon_path, color, width);
                }
            }
        }

        DrawCommand::DrawDRRect {
            outer,
            inner,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            // Tessellate outer rounded rect as a fill, then subtract inner.
            // For correct subtraction we'd need boolean path ops (not in lyon core),
            // so we tessellate both: outer as fill, inner as fill with background
            // color (punch-through). For now, just tessellate the outer shape.
            let outer_path = rrect_to_lyon_path(outer);
            if paint.is_fill() {
                batchers.paths.add_fill(&outer_path, color);
                // Tessellate inner as an "erase" with zero-alpha to approximate subtraction.
                let inner_path = rrect_to_lyon_path(inner);
                batchers.paths.add_fill(&inner_path, [0.0, 0.0, 0.0, 0.0]);
            } else {
                let width = paint.stroke_width.max(1.0);
                batchers.paths.add_stroke(&outer_path, color, width);
                let inner_path = rrect_to_lyon_path(inner);
                batchers.paths.add_stroke(&inner_path, color, width);
            }
        }

        DrawCommand::DrawPoints {
            mode,
            points,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let width = paint.stroke_width.max(1.0);
            match mode {
                PointMode::Points => {
                    // Draw each point as a small filled circle
                    let radius = width * 0.5;
                    for pt in points {
                        let cx = pt.x.get();
                        let cy = pt.y.get();
                        // Approximate point as a diamond (4 triangles)
                        let verts: &[[f32; 2]] = &[
                            [cx, cy - radius],
                            [cx + radius, cy],
                            [cx, cy + radius],
                            [cx - radius, cy],
                        ];
                        let idxs: &[u32] = &[0, 1, 2, 0, 2, 3];
                        batchers.paths.add_vertices(verts, None, idxs, color);
                    }
                }
                PointMode::Lines => {
                    // Draw lines between consecutive point pairs
                    if points.len() >= 2 {
                        let mut builder = lyon::path::Path::builder();
                        let mut i = 0;
                        while i + 1 < points.len() {
                            builder.begin(lyon::math::point(points[i].x.get(), points[i].y.get()));
                            builder.line_to(lyon::math::point(
                                points[i + 1].x.get(),
                                points[i + 1].y.get(),
                            ));
                            builder.end(false);
                            i += 2;
                        }
                        let lyon_path = builder.build();
                        batchers.paths.add_stroke(&lyon_path, color, width);
                    }
                }
                PointMode::Polygon => {
                    // Draw a closed polygon connecting all points
                    if points.len() >= 2 {
                        let mut builder = lyon::path::Path::builder();
                        builder.begin(lyon::math::point(points[0].x.get(), points[0].y.get()));
                        for pt in &points[1..] {
                            builder.line_to(lyon::math::point(pt.x.get(), pt.y.get()));
                        }
                        builder.close();
                        let lyon_path = builder.build();
                        if paint.is_fill() {
                            batchers.paths.add_fill(&lyon_path, color);
                        } else {
                            batchers.paths.add_stroke(&lyon_path, color, width);
                        }
                    }
                }
            }
        }

        DrawCommand::DrawVertices {
            vertices,
            colors,
            tex_coords,
            indices,
            paint,
            transform,
        } => {
            tracing::debug!("DrawVertices: direct upload pending");
        }

        // =================================================================
        // Text -> TextBatcher
        // =================================================================
        DrawCommand::DrawText {
            text,
            offset,
            size: _,
            style,
            paint,
            transform: _,
        } => {
            let font_size = style.font_size.unwrap_or(14.0) as f32;
            let font_weight = style
                .font_weight
                .as_ref()
                .map(FontWeight::value)
                .unwrap_or(400);
            let font_family = style.font_family.as_deref().unwrap_or("sans-serif");
            let key = TextCacheKey::new(text, font_size, font_family, font_weight);
            let color = paint_to_color(paint);
            let position = [offset.dx.get(), offset.dy.get()];
            let clip = state
                .clip
                .current_clip()
                .map(|c| [c.x, c.y, c.width, c.height]);
            batchers.text.add_run(
                key,
                text.clone(),
                font_family.to_string(),
                position,
                color,
                clip,
            );
        }

        DrawCommand::DrawTextSpan {
            span,
            offset,
            text_scale_factor: _,
            transform: _,
        } => {
            // Extract plain text from the rich span and render as a single-style run.
            let plain_text = span.to_plain_text();
            if plain_text.is_empty() {
                tracing::debug!("DrawTextSpan: empty span, skipping");
            } else {
                // Use the root span's style if available, otherwise defaults.
                let (font_size, font_weight, font_family) = if let Some(style) = span.style() {
                    (
                        style.font_size.unwrap_or(14.0) as f32,
                        style
                            .font_weight
                            .as_ref()
                            .map(FontWeight::value)
                            .unwrap_or(400),
                        style
                            .font_family
                            .as_deref()
                            .unwrap_or("sans-serif")
                            .to_string(),
                    )
                } else {
                    (14.0_f32, 400_u16, "sans-serif".to_string())
                };
                let key = TextCacheKey::new(&plain_text, font_size, &font_family, font_weight);
                let color = [0.0, 0.0, 0.0, 1.0]; // default black; spans don't carry a Paint
                let position = [offset.dx.get(), offset.dy.get()];
                let clip = state
                    .clip
                    .current_clip()
                    .map(|c| [c.x, c.y, c.width, c.height]);
                batchers
                    .text
                    .add_run(key, plain_text, font_family, position, color, clip);
            }
        }

        // =================================================================
        // Images -> ImageBatcher
        // =================================================================
        DrawCommand::DrawImage {
            image,
            dst,
            paint,
            transform,
        } => {
            let color = paint
                .as_ref()
                .map(paint_to_color)
                .unwrap_or([1.0, 1.0, 1.0, 1.0]);
            batchers.images.add_image(
                0, // texture_id placeholder
                rect_to_xywh(dst),
                [0.0, 0.0, 1.0, 1.0],
                color,
                extract_transform_2d(transform),
            );
        }

        DrawCommand::DrawTexture {
            texture_id,
            dst,
            src,
            filter_quality: _,
            opacity,
            transform,
        } => {
            let src_uv = src
                .as_ref()
                .map(|s| {
                    [
                        s.left().get(),
                        s.top().get(),
                        s.right().get(),
                        s.bottom().get(),
                    ]
                })
                .unwrap_or([0.0, 0.0, 1.0, 1.0]);
            batchers.images.add_image(
                texture_id.get(),
                rect_to_xywh(dst),
                src_uv,
                [1.0, 1.0, 1.0, *opacity],
                extract_transform_2d(transform),
            );
        }

        DrawCommand::DrawAtlas {
            image,
            sprites,
            transforms: _,
            colors,
            blend_mode,
            paint,
            transform,
        } => {
            tracing::debug!("DrawAtlas: sprite batch pending");
        }

        DrawCommand::DrawImageRepeat {
            image,
            dst,
            repeat,
            paint,
            transform,
        } => {
            tracing::debug!("DrawImageRepeat: tiled texture pending");
        }

        DrawCommand::DrawImageNineSlice {
            image,
            center_slice,
            dst,
            paint,
            transform,
        } => {
            tracing::debug!("DrawImageNineSlice: 9-slice pending");
        }

        DrawCommand::DrawImageFiltered {
            image,
            dst,
            filter,
            paint,
            transform,
        } => {
            tracing::debug!("DrawImageFiltered: filtered image pending");
        }

        // =================================================================
        // Effects -> EffectBatcher
        // =================================================================
        DrawCommand::DrawShadow {
            path,
            color,
            elevation,
            transform,
        } => {
            batchers.effects.add_shadow(ShadowInstance {
                bounds: [0.0, 0.0, 100.0, 100.0], // placeholder bounds from path
                color: color_to_array(color),
                offset: [0.0, *elevation],
                blur_radius: *elevation * 2.0,
                spread: 0.0,
            });
        }

        DrawCommand::DrawGradient {
            rect,
            shader,
            transform,
        } => {
            let bounds = rect_to_xywh(rect);
            let corner_radii = [0.0; 4];
            let xform = extract_transform_2d(transform);
            dispatch_gradient(batchers, shader, bounds, corner_radii, xform);
        }

        DrawCommand::DrawGradientRRect {
            rrect,
            shader,
            transform,
        } => {
            let rect = rrect.bounding_rect();
            let bounds = rect_to_xywh(&rect);
            let corner_radii = rrect_corner_radii(rrect);
            let xform = extract_transform_2d(transform);
            dispatch_gradient(batchers, shader, bounds, corner_radii, xform);
        }

        DrawCommand::DrawColor {
            color,
            blend_mode: _,
            transform,
        } => {
            let c = color_to_array(color);
            // Full-viewport rect with color.
            batchers.shapes.add_rect(
                0.0,
                0.0,
                f32::MAX,
                f32::MAX,
                c,
                [0.0; 4],
                [1.0, 0.0, 0.0, 1.0],
            );
        }

        // =================================================================
        // Compositing -> CompositingBatcher / StateStack
        // =================================================================
        DrawCommand::ShaderMask {
            child: _,
            shader: _,
            bounds,
            blend_mode: _,
            transform,
        } => {
            batchers
                .compositing
                .add_shader_mask(rect_to_xywh(bounds), 0);
        }

        DrawCommand::BackdropFilter {
            child: _,
            filter: _,
            bounds,
            blend_mode: _,
            transform,
        } => {
            batchers.compositing.add_backdrop_filter(
                rect_to_xywh(bounds),
                FilterType::Blur {
                    sigma_x: 10.0,
                    sigma_y: 10.0,
                }, // placeholder
            );
        }

        DrawCommand::SaveLayer {
            bounds,
            paint: _,
            transform,
        } => {
            let b = bounds
                .as_ref()
                .map(rect_to_xywh)
                .unwrap_or([0.0, 0.0, f32::MAX, f32::MAX]);
            batchers.compositing.push_target(b, 1.0, 0);
        }

        DrawCommand::RestoreLayer { transform } => {
            batchers.compositing.pop_target();
        }

        // =================================================================
        // Clipping -> StateStack
        // =================================================================
        DrawCommand::ClipRect { rect, transform } => {
            state.clip.push_rect(ClipRect {
                x: rect.left().get(),
                y: rect.top().get(),
                width: rect.width().get(),
                height: rect.height().get(),
            });
        }

        DrawCommand::ClipRRect { rrect, transform } => {
            state.clip.push_rect(ClipRect {
                x: rrect.rect.left().get(),
                y: rrect.rect.top().get(),
                width: rrect.rect.width().get(),
                height: rrect.rect.height().get(),
            });
        }

        DrawCommand::ClipPath { path, transform } => {
            tracing::debug!("ClipPath: stencil-based clipping pending");
        }
    }
}

// ===========================================================================
// Layer traversal
// ===========================================================================

/// Traverse a [`Scene`]'s layer tree and dispatch all draw commands.
///
/// Returns a `Vec<DrawOp>` that records draw operations in painter's order.
/// Each leaf layer (Canvas, Picture) produces a `DrawGroup` that references
/// ranges into the batcher data. Clip state changes produce `SetScissor`
/// and `ClearScissor` ops.
pub fn traverse_scene(
    scene: &Scene,
    batchers: &mut Batchers,
    state: &mut StateStack,
    scale_factor: f32,
) -> Vec<DrawOp> {
    let mut draw_ops = Vec::new();
    if let Some(root_id) = scene.root() {
        traverse_layer(
            scene.layer_tree(),
            root_id,
            batchers,
            state,
            scale_factor,
            &mut draw_ops,
        );
    }
    draw_ops
}

/// Recursively traverse a single layer and its children.
fn traverse_layer(
    tree: &LayerTree,
    layer_id: LayerId,
    batchers: &mut Batchers,
    state: &mut StateStack,
    scale_factor: f32,
    draw_ops: &mut Vec<DrawOp>,
) {
    let Some(node) = tree.get(layer_id) else {
        return;
    };
    let layer = node.layer();

    match layer {
        // =====================================================================
        // Leaf layers -- dispatch display list commands and emit DrawGroup
        // =====================================================================
        Layer::Canvas(canvas) => {
            let before = batchers.snapshot();
            for cmd in canvas.display_list().commands() {
                dispatch_command(cmd, batchers, state, scale_factor);
            }
            let after = batchers.snapshot();
            if before != after {
                draw_ops.push(DrawOp::DrawGroup { before, after });
            }
        }

        Layer::Picture(picture) => {
            let before = batchers.snapshot();
            for cmd in picture.picture().commands() {
                dispatch_command(cmd, batchers, state, scale_factor);
            }
            let after = batchers.snapshot();
            if before != after {
                draw_ops.push(DrawOp::DrawGroup { before, after });
            }
        }

        Layer::Texture(_) => {
            tracing::debug!("TextureLayer rendering pending");
        }

        Layer::PlatformView(_) => {
            // Platform views are rendered by the platform; engine skips.
        }

        Layer::PerformanceOverlay(_) => {
            tracing::debug!("PerformanceOverlay rendering pending");
        }

        // =====================================================================
        // Clip layers -- emit scissor ops around children
        // =====================================================================
        Layer::ClipRect(clip) => {
            let r = clip.clip_rect();
            let clip_rect = ClipRect {
                x: r.left().get(),
                y: r.top().get(),
                width: r.width().get(),
                height: r.height().get(),
            };
            state.clip.push_rect(clip_rect);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            draw_ops.push(DrawOp::SetScissor(ScissorRect {
                x: clip_rect.x as u32,
                y: clip_rect.y as u32,
                width: clip_rect.width as u32,
                height: clip_rect.height as u32,
            }));
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
            state.clip.pop();
            draw_ops.push(DrawOp::ClearScissor);
        }

        Layer::ClipRRect(clip) => {
            let r = &clip.clip_rrect().rect;
            let clip_rect = ClipRect {
                x: r.left().get(),
                y: r.top().get(),
                width: r.width().get(),
                height: r.height().get(),
            };
            state.clip.push_rect(clip_rect);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            draw_ops.push(DrawOp::SetScissor(ScissorRect {
                x: clip_rect.x as u32,
                y: clip_rect.y as u32,
                width: clip_rect.width as u32,
                height: clip_rect.height as u32,
            }));
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
            state.clip.pop();
            draw_ops.push(DrawOp::ClearScissor);
        }

        Layer::ClipPath(_) => {
            tracing::debug!("ClipPath layer: stencil clipping pending");
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
        }

        Layer::ClipSuperellipse(_) => {
            tracing::debug!("ClipSuperellipse layer: falling back to no clip");
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
        }

        // =====================================================================
        // Transform layers
        // =====================================================================
        Layer::Offset(offset_layer) => {
            let dx = offset_layer.offset().dx.get();
            let dy = offset_layer.offset().dy.get();
            state
                .transform
                .push(glam::Mat4::from_translation(glam::Vec3::new(dx, dy, 0.0)));
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
            state.transform.pop();
        }

        Layer::Transform(_transform_layer) => {
            // Full Matrix4 -> glam::Mat4 conversion to be added.
            state.transform.push(glam::Mat4::IDENTITY);
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
            state.transform.pop();
        }

        // =====================================================================
        // Effect layers
        // =====================================================================
        Layer::Opacity(opacity_layer) => {
            state.opacity.push(opacity_layer.alpha());
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
            state.opacity.pop();
        }

        Layer::ColorFilter(_) => {
            batchers.compositing.add_color_filter([0.0; 4]);
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
        }

        Layer::ImageFilter(_) => {
            batchers.compositing.add_image_filter(
                [0.0; 4],
                FilterType::Blur {
                    sigma_x: 0.0,
                    sigma_y: 0.0,
                },
            );
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
        }

        Layer::ShaderMask(_) => {
            batchers.compositing.add_shader_mask([0.0; 4], 0);
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
        }

        Layer::BackdropFilter(_) => {
            batchers.compositing.add_backdrop_filter(
                [0.0; 4],
                FilterType::Blur {
                    sigma_x: 10.0,
                    sigma_y: 10.0,
                },
            );
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
        }

        // =====================================================================
        // Linking layers
        // =====================================================================
        Layer::Leader(_) => {
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
        }

        Layer::Follower(_) => {
            traverse_children(tree, layer_id, batchers, state, scale_factor, draw_ops);
        }

        // =====================================================================
        // Annotation layers (no visual effect)
        // =====================================================================
        Layer::AnnotatedRegion(_) => {
            // Skip -- metadata only.
        }
    }
}

/// Traverse all children of `parent_id` in order.
fn traverse_children(
    tree: &LayerTree,
    parent_id: LayerId,
    batchers: &mut Batchers,
    state: &mut StateStack,
    scale_factor: f32,
    draw_ops: &mut Vec<DrawOp>,
) {
    if let Some(children) = tree.children(parent_id) {
        // Clone the slice to avoid holding a borrow on `tree` while recursing.
        let child_ids: Vec<_> = children.to_vec();
        for child_id in child_ids {
            traverse_layer(tree, child_id, batchers, state, scale_factor, draw_ops);
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_layer::layer::{CanvasLayer, ClipRectLayer, OffsetLayer, OpacityLayer};
    use flui_types::geometry::{px, Point};

    // -- Batchers tests -------------------------------------------------------

    #[test]
    fn batchers_new_is_empty() {
        let b = Batchers::new();
        assert!(b.is_all_empty());
    }

    #[test]
    fn batchers_clear_all() {
        let mut b = Batchers::new();
        b.shapes.add_rect(
            0.0,
            0.0,
            10.0,
            10.0,
            [1.0; 4],
            [0.0; 4],
            [1.0, 0.0, 0.0, 1.0],
        );
        assert!(!b.is_all_empty());
        b.clear_all();
        assert!(b.is_all_empty());
    }

    #[test]
    fn batchers_default_is_empty() {
        let b = Batchers::default();
        assert!(b.is_all_empty());
    }

    // -- Helper tests ---------------------------------------------------------

    #[test]
    fn color_to_array_converts_correctly() {
        let c = Color::rgba(255, 128, 0, 255);
        let arr = color_to_array(&c);
        assert!((arr[0] - 1.0).abs() < f32::EPSILON);
        assert!((arr[1] - 128.0 / 255.0).abs() < 0.001);
        assert!((arr[2] - 0.0).abs() < f32::EPSILON);
        assert!((arr[3] - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn paint_to_color_uses_paint_color() {
        let paint = Paint::fill(Color::rgb(0, 255, 0));
        let arr = paint_to_color(&paint);
        assert!((arr[0] - 0.0).abs() < f32::EPSILON);
        assert!((arr[1] - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rrect_corner_radii_extracts_x() {
        let rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            px(8.0),
        );
        let radii = rrect_corner_radii(&rrect);
        assert!((radii[0] - 8.0).abs() < f32::EPSILON);
        assert!((radii[1] - 8.0).abs() < f32::EPSILON);
        assert!((radii[2] - 8.0).abs() < f32::EPSILON);
        assert!((radii[3] - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rect_to_xywh_extracts_fields() {
        let r = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let xywh = rect_to_xywh(&r);
        assert!((xywh[0] - 10.0).abs() < f32::EPSILON);
        assert!((xywh[1] - 20.0).abs() < f32::EPSILON);
        assert!((xywh[2] - 100.0).abs() < f32::EPSILON);
        assert!((xywh[3] - 50.0).abs() < f32::EPSILON);
    }

    // -- Dispatch tests -------------------------------------------------------

    #[test]
    fn dispatch_draw_rect_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawRect {
            rect: Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(30.0)),
            paint: Paint::fill(Color::rgb(255, 0, 0)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.rect_count(), 1);
    }

    #[test]
    fn dispatch_draw_rrect_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawRRect {
            rrect: RRect::from_rect_circular(
                Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
                px(10.0),
            ),
            paint: Paint::fill(Color::rgb(0, 0, 255)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.rect_count(), 1);
    }

    #[test]
    fn dispatch_draw_circle_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawCircle {
            center: Point::new(px(50.0), px(50.0)),
            radius: px(25.0),
            paint: Paint::fill(Color::rgb(0, 255, 0)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.circle_count(), 1);
    }

    #[test]
    fn dispatch_draw_oval_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawOval {
            rect: Rect::from_xywh(px(0.0), px(0.0), px(80.0), px(40.0)),
            paint: Paint::fill(Color::rgb(128, 128, 128)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        // Ovals are dispatched as circle instances.
        assert_eq!(batchers.shapes.circle_count(), 1);
    }

    #[test]
    fn dispatch_draw_line_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawLine {
            p1: Point::new(px(0.0), px(0.0)),
            p2: Point::new(px(100.0), px(100.0)),
            paint: Paint::stroke(Color::rgb(255, 255, 255), 2.0),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.line_count(), 1);
    }

    #[test]
    fn dispatch_draw_arc_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawArc {
            rect: Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            start_angle: 0.0,
            sweep_angle: std::f32::consts::PI,
            use_center: false,
            paint: Paint::fill(Color::rgb(255, 255, 0)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.arc_count(), 1);
    }

    #[test]
    fn dispatch_draw_text_routes_to_text() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawText {
            text: "Hello".to_string(),
            offset: flui_types::Offset::ZERO,
            size: flui_types::Size::new(px(50.0), px(14.0)),
            style: flui_types::typography::TextStyle::default(),
            paint: Paint::fill(Color::BLACK),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.text.run_count(), 1);
    }

    #[test]
    fn dispatch_draw_shadow_routes_to_effects() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawShadow {
            path: flui_types::painting::Path::new(),
            color: Color::rgba(0, 0, 0, 128),
            elevation: 4.0,
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.effects.shadow_count(), 1);
    }

    #[test]
    fn dispatch_draw_color_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawColor {
            color: Color::rgb(255, 0, 0),
            blend_mode: flui_types::painting::BlendMode::SrcOver,
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.rect_count(), 1);
    }

    #[test]
    fn dispatch_clip_rect_pushes_state() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::ClipRect {
            rect: Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        let clip = state.clip.current_clip().expect("clip should be active");
        assert!((clip.x - 10.0).abs() < f32::EPSILON);
        assert!((clip.y - 20.0).abs() < f32::EPSILON);
        assert!((clip.width - 100.0).abs() < f32::EPSILON);
        assert!((clip.height - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dispatch_clip_rrect_pushes_state() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::ClipRRect {
            rrect: RRect::from_rect_circular(
                Rect::from_xywh(px(5.0), px(5.0), px(80.0), px(60.0)),
                px(8.0),
            ),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        let clip = state.clip.current_clip().expect("clip should be active");
        assert!((clip.x - 5.0).abs() < f32::EPSILON);
        assert!((clip.width - 80.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dispatch_save_restore_layer() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();

        let save = DrawCommand::SaveLayer {
            bounds: Some(Rect::from_xywh(px(0.0), px(0.0), px(200.0), px(200.0))),
            paint: Paint::fill(Color::BLACK),
            transform: Matrix4::identity(),
        };
        dispatch_command(&save, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.compositing.target_depth(), 1);

        let restore = DrawCommand::RestoreLayer {
            transform: Matrix4::identity(),
        };
        dispatch_command(&restore, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.compositing.target_depth(), 0);
    }

    // -- Traversal tests ------------------------------------------------------

    #[test]
    fn traverse_empty_scene_is_noop() {
        let scene = Scene::empty(flui_types::Size::new(px(800.0), px(600.0)));
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        traverse_scene(&scene, &mut batchers, &mut state, 1.0);
        assert!(batchers.is_all_empty());
    }

    #[test]
    fn traverse_scene_with_canvas_layer() {
        use flui_painting::Canvas;

        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(50.0)),
            &Paint::fill(Color::rgb(255, 0, 0)),
        );
        let canvas_layer = CanvasLayer::from_canvas(canvas);

        let scene = Scene::from_layer(
            flui_types::Size::new(px(800.0), px(600.0)),
            Layer::Canvas(canvas_layer),
            0,
        );

        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        traverse_scene(&scene, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.rect_count(), 1);
    }

    #[test]
    fn traverse_clip_rect_pushes_and_pops() {
        use flui_types::painting::Clip;

        let mut tree = LayerTree::new();
        let clip_layer = Layer::ClipRect(ClipRectLayer::new(
            Rect::from_xywh(px(10.0), px(10.0), px(200.0), px(200.0)),
            Clip::HardEdge,
        ));
        let clip_id = tree.insert(clip_layer);

        let canvas_layer = Layer::Canvas(CanvasLayer::new());
        let canvas_id = tree.insert(canvas_layer);
        tree.add_child(clip_id, canvas_id);

        let scene = Scene::new(
            flui_types::Size::new(px(800.0), px(600.0)),
            tree,
            Some(clip_id),
            0,
        );

        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        traverse_scene(&scene, &mut batchers, &mut state, 1.0);

        // Clip should have been popped after traversal.
        assert!(state.clip.current_clip().is_none());
    }

    #[test]
    fn traverse_offset_layer_pushes_and_pops_transform() {
        let mut tree = LayerTree::new();
        let offset_layer = Layer::Offset(OffsetLayer::new(flui_types::Offset::new(
            px(10.0),
            px(20.0),
        )));
        let offset_id = tree.insert(offset_layer);

        let canvas_layer = Layer::Canvas(CanvasLayer::new());
        let canvas_id = tree.insert(canvas_layer);
        tree.add_child(offset_id, canvas_id);

        let scene = Scene::new(
            flui_types::Size::new(px(800.0), px(600.0)),
            tree,
            Some(offset_id),
            0,
        );

        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        traverse_scene(&scene, &mut batchers, &mut state, 1.0);

        // Transform should have been popped after traversal.
        assert_eq!(state.transform.depth(), 0);
    }

    #[test]
    fn traverse_opacity_layer_pushes_and_pops() {
        let mut tree = LayerTree::new();
        let opacity_layer = Layer::Opacity(OpacityLayer::new(0.5));
        let opacity_id = tree.insert(opacity_layer);

        let canvas_layer = Layer::Canvas(CanvasLayer::new());
        let canvas_id = tree.insert(canvas_layer);
        tree.add_child(opacity_id, canvas_id);

        let scene = Scene::new(
            flui_types::Size::new(px(800.0), px(600.0)),
            tree,
            Some(opacity_id),
            0,
        );

        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        traverse_scene(&scene, &mut batchers, &mut state, 1.0);

        // Opacity should have been popped after traversal.
        assert!((state.opacity.current() - 1.0).abs() < f32::EPSILON);
    }

    // -- Draw order tests -----------------------------------------------------

    #[test]
    fn draw_ops_preserve_per_layer_order() {
        // Create a scene with two canvas layers under a common parent:
        //   root (Offset)
        //     ├── Canvas1 (draws a red rect)
        //     └── Canvas2 (draws a blue circle)
        //
        // The draw_ops should contain two DrawGroup entries in order,
        // ensuring Canvas2's circle is drawn after Canvas1's rect.
        use flui_painting::Canvas;

        let mut tree = LayerTree::new();
        let root = Layer::Offset(OffsetLayer::new(flui_types::Offset::ZERO));
        let root_id = tree.insert(root);

        // Canvas 1: red rect
        let mut canvas1 = Canvas::new();
        canvas1.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(50.0)),
            &Paint::fill(Color::rgb(255, 0, 0)),
        );
        let c1_id = tree.insert(Layer::Canvas(CanvasLayer::from_canvas(canvas1)));
        tree.add_child(root_id, c1_id);

        // Canvas 2: blue circle
        let mut canvas2 = Canvas::new();
        canvas2.draw_circle(
            Point::new(px(50.0), px(50.0)),
            px(25.0),
            &Paint::fill(Color::rgb(0, 0, 255)),
        );
        let c2_id = tree.insert(Layer::Canvas(CanvasLayer::from_canvas(canvas2)));
        tree.add_child(root_id, c2_id);

        let scene = Scene::new(
            flui_types::Size::new(px(800.0), px(600.0)),
            tree,
            Some(root_id),
            0,
        );

        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let ops = traverse_scene(&scene, &mut batchers, &mut state, 1.0);

        // Should have exactly 2 draw groups.
        assert_eq!(ops.len(), 2, "expected 2 draw groups, got {}", ops.len());

        // First group: rect added (rects 0->1), no circles.
        match &ops[0] {
            DrawOp::DrawGroup { before, after } => {
                assert_eq!(before.rects, 0);
                assert_eq!(after.rects, 1, "first group should add 1 rect");
                assert_eq!(before.circles, 0);
                assert_eq!(after.circles, 0, "first group should add 0 circles");
            }
            other => panic!("expected DrawGroup, got {:?}", other),
        }

        // Second group: circle added (circles 0->1), rects unchanged at 1.
        match &ops[1] {
            DrawOp::DrawGroup { before, after } => {
                assert_eq!(before.rects, 1);
                assert_eq!(after.rects, 1, "second group should not add rects");
                assert_eq!(before.circles, 0);
                assert_eq!(after.circles, 1, "second group should add 1 circle");
            }
            other => panic!("expected DrawGroup, got {:?}", other),
        }
    }

    #[test]
    fn draw_ops_emit_scissor_for_clip_layers() {
        use flui_painting::Canvas;
        use flui_types::painting::Clip;

        let mut tree = LayerTree::new();
        let clip_layer = Layer::ClipRect(ClipRectLayer::new(
            Rect::from_xywh(px(10.0), px(20.0), px(200.0), px(100.0)),
            Clip::HardEdge,
        ));
        let clip_id = tree.insert(clip_layer);

        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            &Paint::fill(Color::rgb(255, 0, 0)),
        );
        let canvas_id = tree.insert(Layer::Canvas(CanvasLayer::from_canvas(canvas)));
        tree.add_child(clip_id, canvas_id);

        let scene = Scene::new(
            flui_types::Size::new(px(800.0), px(600.0)),
            tree,
            Some(clip_id),
            0,
        );

        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let ops = traverse_scene(&scene, &mut batchers, &mut state, 1.0);

        // Should be: SetScissor, DrawGroup, ClearScissor
        assert_eq!(ops.len(), 3, "expected 3 ops, got {:?}", ops);
        assert!(
            matches!(ops[0], DrawOp::SetScissor(ref s) if s.x == 10 && s.y == 20 && s.width == 200 && s.height == 100),
            "expected SetScissor, got {:?}",
            ops[0]
        );
        assert!(
            matches!(ops[1], DrawOp::DrawGroup { .. }),
            "expected DrawGroup, got {:?}",
            ops[1]
        );
        assert!(
            matches!(ops[2], DrawOp::ClearScissor),
            "expected ClearScissor, got {:?}",
            ops[2]
        );
    }

    #[test]
    fn snapshot_reflects_batcher_counts() {
        let mut b = Batchers::new();
        let snap0 = b.snapshot();
        assert_eq!(snap0, BatcherSnapshot::default());

        b.shapes.add_rect(
            0.0,
            0.0,
            10.0,
            10.0,
            [1.0; 4],
            [0.0; 4],
            [1.0, 0.0, 0.0, 1.0],
        );
        b.shapes
            .add_circle(5.0, 5.0, 3.0, [1.0; 4], [1.0, 0.0, 0.0, 1.0]);

        let snap1 = b.snapshot();
        assert_eq!(snap1.rects, 1);
        assert_eq!(snap1.circles, 1);
        assert_eq!(snap1.arcs, 0);
    }

    #[test]
    fn empty_canvas_layer_produces_no_draw_group() {
        let mut tree = LayerTree::new();
        let canvas_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        let scene = Scene::new(
            flui_types::Size::new(px(800.0), px(600.0)),
            tree,
            Some(canvas_id),
            0,
        );

        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let ops = traverse_scene(&scene, &mut batchers, &mut state, 1.0);

        // Empty canvas adds nothing to batchers, so no DrawGroup emitted.
        assert!(
            ops.is_empty(),
            "expected no ops for empty canvas, got {:?}",
            ops
        );
    }
}
