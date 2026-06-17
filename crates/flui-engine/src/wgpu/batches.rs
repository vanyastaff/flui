//! Record-side draw accumulation helpers extracted from `WgpuPainter`.
//!
//! `DrawBatcher` owns the three mutable-but-non-GPU assets used only during
//! draw recording:
//! - `tessellator`        — Lyon-based path tessellator
//! - `path_cache`         — per-frame tessellation cache (keyed by path hash + scale)
//! - `superellipse_cache` — per-frame iOS-squircle path cache
//!
//! For each record call the caller (`WgpuPainter`) passes in the GPU draw-state
//! and the accumulation targets via **plain borrowed parameters**:
//! - `segment: &mut DrawSegment`      — current accumulation buffer
//! - `draw_order: &mut Vec<DrawItem>` — ordered list of sealed segments
//! - `state: &GpuStateStack`          — read-only transform/scissor queries
//! - `opacity: f32`                   — current opacity (Copy-read before call)
//!
//! This is the borrow seam described in the T9a chief-architect verdict.  Four
//! disjoint `WgpuPainter` fields are borrowed simultaneously; reading `opacity`
//! into a `f32` local before the call prevents `compositor` from being borrowed
//! across the batcher invocation.
//!
//! # Shader dispatch
//!
//! `dispatch_shader_rect` (gradient/shader fills for rect/rrect/circle) is NOT
//! on `DrawBatcher`; it stays on `WgpuPainter` because it calls
//! `gradient_rect`/`radial_gradient_rect`/`sweep_gradient_rect` which write
//! directly into `current_segment`.  Each painter shim pre-checks the shader
//! case (early return) and only delegates to the batcher for the non-shader
//! paths.
//!
//! # Invariants preserved
//!
//! - **Non-`SrcOver` segment seal** fires in `add_tessellated_with_key` at the
//!   identical point as the original painter code (immediately after appending a
//!   non-`SrcOver` batch entry), now threaded via `&mut draw_order`.
//! - **Scissor coalescing** reads `state.current_scissor()` as a `Copy` value at
//!   the same instant as the original code.
//! - **Opacity baked at record time**: the `opacity` value is read in the
//!   `WgpuPainter` shim before the batcher call, preserving the original
//!   read point relative to the compositor stack.
//! - **No new per-draw heap allocations** vs. the pre-extraction baseline.

use flui_painting::{BlendMode, Paint, PaintStyle};
use flui_types::{
    Offset, Point, Rect,
    geometry::{Pixels, RRect, px},
    painting::path::Path,
    styling::Color,
};

use super::{
    command_ir::{DrawItem, DrawSegment, TessellatedBatch},
    path_cache::PathCache,
    pipeline::{self, PipelineKey},
    state_stack::GpuStateStack,
    superellipse_cache::SuperellipsePathCache,
    tessellator::Tessellator,
    vertex::Vertex,
};

/// Owns the tessellator and per-frame geometry caches used during draw recording.
///
/// Separated from `WgpuPainter` so the record-side mutable state (`tessellator`,
/// `path_cache`, `superellipse_cache`) can be borrowed independently from the
/// flush-side state (`texture_batch`) and the draw accumulation targets
/// (`current_segment`, `draw_order`).  See the module-level doc for the borrow
/// seam contract.
pub(super) struct DrawBatcher {
    /// Lyon-based path tessellator for complex shapes.
    pub(super) tessellator: Tessellator,

    /// Per-frame tessellation cache: avoids re-tessellating identical paths within
    /// a frame.
    pub(super) path_cache: PathCache,

    /// Per-frame iOS-squircle path cache.
    ///
    /// Mirrors `PathCache` ownership and eviction semantics (`max_entries` +
    /// frame-based eviction).  Consulted by `WgpuPainter::superellipse_path` (the
    /// `Backend::superellipse_path` override).
    pub(super) superellipse_cache: SuperellipsePathCache,
}

// GPU rendering routinely converts between f32/u8/u32 for pixel coordinates,
// color channels, and buffer indices. These truncations are intentional.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl DrawBatcher {
    /// Construct a `DrawBatcher` with the same cache capacities used by the
    /// original `WgpuPainter::with_shared_device`.
    pub(super) fn new() -> Self {
        Self {
            tessellator: Tessellator::new(),
            path_cache: PathCache::new(512),
            superellipse_cache: SuperellipsePathCache::new(256),
        }
    }

    // ===== Segment accumulation primitives =====

    /// Seal `segment` and push it onto `draw_order`, then start a fresh empty
    /// segment.  An empty segment is never pushed (avoids empty GPU passes).
    ///
    /// This is the **single place** that performs `current_segment → draw_order`
    /// promotion.  Every seal — whether triggered by an explicit Z-interleave
    /// (`WgpuPainter::queue_offscreen_result`), by the non-`SrcOver` draw-order
    /// contract in [`DrawBatcher::add_tessellated_with_key`], or by the final
    /// flush before GPU submission — routes through here.
    pub(super) fn finish_current_segment(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
    ) {
        let completed = std::mem::replace(segment, DrawSegment::new());
        if !completed.is_empty() {
            draw_order.push(DrawItem::Segment(completed));
        }
    }

    /// Append tessellated vertices/indices to `segment` under the given pipeline
    /// key, starting a new `TessellatedBatch` on a key or scissor change.
    ///
    /// # Draw-order contract for non-`SrcOver` blend modes
    ///
    /// After appending a non-`SrcOver` entry the segment is immediately sealed.
    /// This guarantees the blend-mode shape flushes **after** any instanced draws
    /// recorded into the same segment, which is required for destructive modes
    /// (Clear, DstOut, Src, SrcIn, DstIn, SrcOut, SrcATop, DstATop, Xor).
    /// `SrcOver` shapes do not trigger a split; the common path has zero overhead.
    pub(super) fn add_tessellated_with_key(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        vertices: Vec<Vertex>,
        indices: &[u32],
        key: PipelineKey,
    ) {
        if indices.is_empty() {
            return;
        }

        let base_index = segment.vertices.len() as u32;
        let index_start = segment.indices.len() as u32;

        segment.vertices.extend(vertices);
        segment
            .indices
            .extend(indices.iter().map(|&i| i + base_index));

        let index_count = indices.len() as u32;

        if let Some(last) = segment.tess_batches.last_mut()
            && last.pipeline_key == key
            && last.scissor == state.current_scissor()
        {
            last.index_count += index_count;
        } else {
            segment.current_pipeline_key = Some(key);
            segment.tess_batches.push(TessellatedBatch {
                pipeline_key: key,
                scissor: state.current_scissor(),
                index_start,
                index_count,
            });
        }

        // Draw-order contract: close the segment after any non-SrcOver blend.
        if key.blend_mode() != BlendMode::SrcOver {
            Self::finish_current_segment(segment, draw_order);
        }
    }

    /// Apply the current world transform to every vertex position in `vertices`
    /// (already tessellated in local space) and submit to the tessellated batch.
    ///
    /// `shape.wgsl` only converts px→clip via the viewport uniform; it has no
    /// model-matrix uniform, so the CPU must bake the transform at record time.
    pub(super) fn submit_transformed_geometry(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        mut vertices: Vec<Vertex>,
        indices: &[u32],
        key: PipelineKey,
    ) {
        let transform = state.current_transform();
        for v in &mut vertices {
            let transformed = transform * glam::vec4(v.position[0], v.position[1], 0.0, 1.0);
            v.position = [transformed.x, transformed.y];
        }
        Self::add_tessellated_with_key(segment, draw_order, state, vertices, indices, key);
    }

    /// Prime the tessellator's flatten tolerance from the current CTM max-basis
    /// length.  Must be called immediately before any `tessellate_*` invocation.
    pub(super) fn prime_tessellator_scale(&mut self, state: &GpuStateStack) {
        self.tessellator.set_max_scale(state.max_scale());
    }

    /// Convert a `Shader` into GPU `GradientStop`s (max 8 stops).
    ///
    /// Kept on `DrawBatcher` so `WgpuPainter::dispatch_shader_rect` can call it
    /// without owning the batcher.
    pub(super) fn shader_to_gradient_stops(
        shader: &flui_types::painting::Shader,
    ) -> Vec<super::effects::GradientStop> {
        let (colors, stops) = match shader {
            flui_types::painting::Shader::LinearGradient { colors, stops, .. }
            | flui_types::painting::Shader::RadialGradient { colors, stops, .. }
            | flui_types::painting::Shader::SweepGradient { colors, stops, .. } => {
                (colors.as_slice(), stops.as_deref())
            }
            flui_types::painting::Shader::Solid { color } => {
                return vec![
                    super::effects::GradientStop::new(*color, 0.0),
                    super::effects::GradientStop::new(*color, 1.0),
                ];
            }
            _ => return vec![],
        };

        let count = colors.len().min(8);
        (0..count)
            .map(|i| {
                let position = if let Some(s) = stops {
                    s.get(i)
                        .copied()
                        .unwrap_or(i as f32 / (count - 1).max(1) as f32)
                } else {
                    i as f32 / (count - 1).max(1) as f32
                };
                super::effects::GradientStop::new(colors[i], position)
            })
            .collect()
    }

    // ===== Moved record methods (T9a set) =====
    //
    // Each method handles everything EXCEPT the shader/gradient dispatch path
    // (which stays on `WgpuPainter::dispatch_shader_rect`).  The painter shim
    // pre-handles the shader case with an early return before calling into the
    // batcher.

    /// Record a non-shader filled rectangle, or a stroked rectangle.
    ///
    /// The painter shim pre-handles gradient/shader fills via
    /// `dispatch_shader_rect` before calling this method.
    pub(super) fn rect(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        opacity: f32,
        rect: Rect<Pixels>,
        paint: &Paint,
    ) {
        if paint.style == PaintStyle::Fill {
            let color = if opacity < 1.0 {
                let alpha = (f32::from(paint.color.a) * opacity) as u8;
                flui_types::Color::rgba(paint.color.r, paint.color.g, paint.color.b, alpha)
            } else {
                paint.color
            };

            // The instanced fast path renders with a hardcoded SrcOver
            // (ALPHA_BLENDING) pipeline. Non-SrcOver blend modes must route
            // through the tessellated path, whose pipeline is selected per
            // `pipeline_key_from_paint` (Phase A fixed-function Porter-Duff).
            //
            // Phase-A quality limit: the tessellated path produces aliased edges
            // at sample_count=1 (no SDF anti-aliasing) and the scissor clip is
            // an AABB, not a pixel-perfect rounded shape. SDF-quality blended
            // shapes are Phase B.
            if state.is_axis_aligned() && paint.blend_mode == BlendMode::SrcOver {
                // Fast path: GPU instancing for axis-aligned rects.
                let top_left = state.apply_transform(Point::new(rect.left(), rect.top()));
                let bottom_right = state.apply_transform(Point::new(rect.right(), rect.bottom()));
                let transformed_rect =
                    Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);
                let instance = state.apply_active_clip(super::instancing::RectInstance::rect(
                    transformed_rect,
                    color,
                ));
                let _ = segment.rect_batch.add(instance);
                DrawSegment::push_scissor_region(
                    &mut segment.rect_scissors,
                    state.current_scissor(),
                );
            } else {
                // Slow path: tessellate rotated/skewed rects or any non-SrcOver
                // blend mode as a transformed quad.
                //
                // Phase-A quality note: uses the tessellated path → aliased edges
                // (no SDF AA), AABB scissor clip. SDF-quality blended rects are
                // Phase B.
                let tl = state.apply_transform(Point::new(rect.left(), rect.top()));
                let tr = state.apply_transform(Point::new(rect.right(), rect.top()));
                let br = state.apply_transform(Point::new(rect.right(), rect.bottom()));
                let bl = state.apply_transform(Point::new(rect.left(), rect.bottom()));
                let rgba = color.to_rgba_f32_array();
                let vertices = vec![
                    Vertex {
                        position: [tl.x.0, tl.y.0],
                        color: rgba,
                        tex_coord: [0.0, 0.0],
                    },
                    Vertex {
                        position: [tr.x.0, tr.y.0],
                        color: rgba,
                        tex_coord: [1.0, 0.0],
                    },
                    Vertex {
                        position: [br.x.0, br.y.0],
                        color: rgba,
                        tex_coord: [1.0, 1.0],
                    },
                    Vertex {
                        position: [bl.x.0, bl.y.0],
                        color: rgba,
                        tex_coord: [0.0, 1.0],
                    },
                ];
                let indices = [0u32, 1, 2, 0, 2, 3];
                Self::add_tessellated_with_key(
                    segment,
                    draw_order,
                    state,
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
        } else {
            // Stroked rect — tessellate (less common, fallback path).
            self.prime_tessellator_scale(state);
            if let Ok((vertices, indices)) = self.tessellator.tessellate_rect_stroke(rect, paint) {
                Self::submit_transformed_geometry(
                    segment,
                    draw_order,
                    state,
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
        }
    }

    /// Record a non-shader filled rounded rectangle, or a stroked one.
    ///
    /// The painter shim pre-handles gradient/shader fills.
    pub(super) fn rrect(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        opacity: f32,
        rrect: RRect,
        paint: &Paint,
    ) {
        if paint.style == PaintStyle::Fill {
            let color = if opacity < 1.0 {
                let alpha = (f32::from(paint.color.a) * opacity) as u8;
                flui_types::Color::rgba(paint.color.r, paint.color.g, paint.color.b, alpha)
            } else {
                paint.color
            };

            // The instanced fast path renders with a hardcoded SrcOver pipeline.
            // A non-SrcOver blend mode must route through the tessellated path so
            // the blend pipeline keyed by `pipeline_key_from_paint` is selected.
            //
            // Phase-A quality note: the tessellated fallback produces aliased edges
            // (no SDF AA) and the scissor clip is an AABB, not the rounded shape.
            // SDF-quality blended rounded rects are Phase B.
            if paint.blend_mode != BlendMode::SrcOver {
                // Tessellate the filled rounded rect in local space, carry the
                // opacity-adjusted color and the requested blend mode, then bake
                // the transform via `submit_transformed_geometry`.
                let fill_paint = Paint::fill(color).with_blend_mode(paint.blend_mode);
                self.prime_tessellator_scale(state);
                match self.tessellator.tessellate_rrect(rrect, &fill_paint) {
                    Ok((vertices, indices)) => {
                        let key = pipeline::pipeline_key_from_paint(&fill_paint);
                        Self::submit_transformed_geometry(
                            segment, draw_order, state, vertices, &indices, key,
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Failed to tessellate blended rrect: {}", e);
                    }
                }
                return;
            }

            let top_left = state.apply_transform(Point::new(rrect.rect.left(), rrect.rect.top()));
            let bottom_right =
                state.apply_transform(Point::new(rrect.rect.right(), rrect.rect.bottom()));
            let transformed_rect =
                Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);

            // GPU instancing for filled rounded rects.
            let instance =
                state.apply_active_clip(super::instancing::RectInstance::rounded_rect_corners(
                    transformed_rect,
                    color,
                    rrect.top_left.x.0.max(rrect.top_left.y.0),
                    rrect.top_right.x.0.max(rrect.top_right.y.0),
                    rrect.bottom_right.x.0.max(rrect.bottom_right.y.0),
                    rrect.bottom_left.x.0.max(rrect.bottom_left.y.0),
                ));
            let _ = segment.rect_batch.add(instance);
            DrawSegment::push_scissor_region(&mut segment.rect_scissors, state.current_scissor());
        } else {
            // Stroked rounded rect — tessellate (fallback).
            self.prime_tessellator_scale(state);
            if let Ok((vertices, indices)) = self.tessellator.tessellate_rrect(rrect, paint) {
                Self::submit_transformed_geometry(
                    segment,
                    draw_order,
                    state,
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
        }
    }

    /// Record a non-shader filled circle, or a stroked circle.
    ///
    /// The painter shim pre-handles gradient/shader fills.
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/draw_order/state/opacity are disjoint WgpuPainter \
                  fields passed as separate borrows; merging them into a context struct defeats \
                  the T9a borrow split"
    )]
    pub(super) fn circle(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        opacity: f32,
        center: Point<Pixels>,
        radius: f32,
        paint: &Paint,
    ) {
        if paint.style == PaintStyle::Fill {
            let color = if opacity < 1.0 {
                let alpha = (f32::from(paint.color.a) * opacity) as u8;
                flui_types::Color::rgba(paint.color.r, paint.color.g, paint.color.b, alpha)
            } else {
                paint.color
            };

            // The instanced fast path renders with a hardcoded SrcOver pipeline;
            // a non-SrcOver blend mode must route through the tessellated path.
            if state.is_axis_aligned() && paint.blend_mode == BlendMode::SrcOver {
                // Fast path: axis-aligned — use GPU instancing.
                let transformed_center = state.apply_transform(center);
                let m = state.current_transform();
                let sx = (m.x_axis.x * m.x_axis.x + m.x_axis.y * m.x_axis.y).sqrt();
                let sy = (m.y_axis.x * m.y_axis.x + m.y_axis.y * m.y_axis.y).sqrt();

                let instance = super::instancing::CircleInstance::new(
                    transformed_center,
                    radius,
                    color,
                    [sx, sy],
                );
                let _ = segment.circle_batch.add(instance);
                DrawSegment::push_scissor_region(
                    &mut segment.circle_scissors,
                    state.current_scissor(),
                );
            } else {
                // Slow path: rotation/shear or non-SrcOver — tessellate and bake.
                //
                // Phase-A quality note: uses the tessellated path → aliased edges
                // (no SDF AA), AABB scissor clip. SDF-quality blended circles are
                // Phase B.
                let fill_paint = Paint {
                    color,
                    style: PaintStyle::Fill,
                    blend_mode: paint.blend_mode,
                    ..Paint::default()
                };
                self.prime_tessellator_scale(state);
                match self
                    .tessellator
                    .tessellate_circle(center, radius, &fill_paint)
                {
                    Ok((vertices, indices)) => {
                        let key = pipeline::pipeline_key_from_paint(&fill_paint);
                        Self::submit_transformed_geometry(
                            segment, draw_order, state, vertices, &indices, key,
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Failed to tessellate rotated circle: {}", e);
                    }
                }
            }
        } else {
            // Stroked circle — tessellate (fallback).
            self.prime_tessellator_scale(state);
            if let Ok((vertices, indices)) =
                self.tessellator.tessellate_circle(center, radius, paint)
            {
                Self::submit_transformed_geometry(
                    segment,
                    draw_order,
                    state,
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
        }
    }

    /// Record a filled or stroked oval (ellipse).
    pub(super) fn oval(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        rect: Rect<Pixels>,
        paint: &Paint,
    ) {
        let center = rect.center();
        let radii = Point::new(rect.width() / 2.0, rect.height() / 2.0);

        self.prime_tessellator_scale(state);
        if let Ok((vertices, indices)) = self.tessellator.tessellate_ellipse(center, radii, paint) {
            Self::submit_transformed_geometry(
                segment,
                draw_order,
                state,
                vertices,
                &indices,
                pipeline::pipeline_key_from_paint(paint),
            );
        }
    }

    /// Record a double rounded rectangle (ring with inner cutout).
    pub(super) fn draw_drrect(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        outer: RRect,
        inner: RRect,
        paint: &Paint,
    ) {
        self.prime_tessellator_scale(state);
        match self.tessellator.tessellate_drrect(&outer, &inner, paint) {
            Ok((vertices, indices)) => {
                Self::submit_transformed_geometry(
                    segment,
                    draw_order,
                    state,
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
            Err(e) => {
                tracing::error!("Failed to tessellate DRRect: {}", e);
            }
        }
    }

    // ===== Moved record methods (T9b set) =====

    /// Record a filled or stroked arc (pie-slice or arc segment).
    ///
    /// # Instanced fast path
    ///
    /// An axis-aligned, non-reflected, `SrcOver` filled arc is submitted as a
    /// GPU `ArcInstance`. All other cases — rotation, shear, reflection (`det < 0`),
    /// stroked arcs, or any non-`SrcOver` blend mode — fall through to the
    /// tessellated path.
    ///
    /// The **2-D determinant reflection guard** is preserved exactly:
    /// `det = m.x_axis.x * m.y_axis.y − m.x_axis.y * m.y_axis.x`.
    /// `det < 0` means the transform contains a reflection; the arc shader cannot
    /// represent a reflected wedge, so tessellation is required even when the
    /// transform is otherwise axis-aligned.
    ///
    /// `draw_arc` does not read opacity — no opacity baking is performed.
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/draw_order/state are disjoint WgpuPainter fields \
                  passed as separate borrows; arc geometry parameters are all necessary"
    )]
    pub(super) fn draw_arc(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        rect: Rect<Pixels>,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) {
        let center = rect.center();
        // Pixels / Pixels = f32 (dimensionless ratio)
        let radius: f32 = (rect.width() + rect.height()) / px(4.0);

        if paint.style == PaintStyle::Fill {
            // Filled arc (pie slice when use_center, arc segment when !use_center).
            // Gate on axis-aligned transform: the arc instance shader only handles
            // translation + axis-aligned scale; rotation/shear/reflection require
            // tessellation.
            //
            // `is_axis_aligned` already rejects rotation and shear, but it also
            // returns `true` for a reflection like `scale(-1, 1)` because the
            // off-diagonal elements are still zero.  A reflection negates the wedge
            // direction, which the instanced shader cannot represent.  Guard with the
            // 2D determinant: det < 0 means the transform includes a reflection and
            // the wedge would be drawn on the wrong side.
            let m = state.current_transform();
            let det = m.x_axis.x * m.y_axis.y - m.x_axis.y * m.y_axis.x;
            // The instanced fast path renders with a hardcoded SrcOver pipeline;
            // a non-SrcOver blend mode must route through the tessellated path
            // (which selects the blend pipeline via `pipeline_key_from_paint`).
            if state.is_axis_aligned() && det >= 0.0 && paint.blend_mode == BlendMode::SrcOver {
                // Fast path: GPU instancing for filled arcs.
                let transformed_center = state.apply_transform(center);
                let sx = (m.x_axis.x * m.x_axis.x + m.x_axis.y * m.x_axis.y).sqrt();
                let sy = (m.y_axis.x * m.y_axis.x + m.y_axis.y * m.y_axis.y).sqrt();

                let instance = super::instancing::ArcInstance::new(
                    transformed_center,
                    radius,
                    start_angle,
                    sweep_angle,
                    paint.color,
                    [sx, sy],
                );
                let _ = segment.arc_batch.add(instance);
                DrawSegment::push_scissor_region(
                    &mut segment.arc_scissors,
                    state.current_scissor(),
                );
            } else {
                // Slow path: rotation, shear, reflection, or non-SrcOver blend
                // mode — tessellate in local space and bake the full transform
                // into vertex positions.
                //
                // Phase-A quality note: uses the tessellated path → aliased edges
                // (no SDF AA), AABB scissor clip. SDF-quality blended arcs are
                // Phase B.
                self.prime_tessellator_scale(state);
                match self.tessellator.tessellate_arc(
                    rect,
                    start_angle,
                    sweep_angle,
                    use_center,
                    paint,
                ) {
                    Ok((vertices, indices)) => {
                        let key = pipeline::pipeline_key_from_paint(paint);
                        Self::submit_transformed_geometry(
                            segment, draw_order, state, vertices, &indices, key,
                        );
                    }
                    Err(e) => {
                        tracing::error!("Failed to tessellate filled arc: {}", e);
                    }
                }
            }
        } else {
            // Stroked arcs always tessellate (no instanced stroke pipeline).
            self.prime_tessellator_scale(state);
            match self
                .tessellator
                .tessellate_arc(rect, start_angle, sweep_angle, use_center, paint)
            {
                Ok((vertices, indices)) => {
                    Self::submit_transformed_geometry(
                        segment,
                        draw_order,
                        state,
                        vertices,
                        &indices,
                        pipeline::pipeline_key_from_paint(paint),
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to tessellate stroked arc: {}", e);
                }
            }
        }
    }

    /// Record a stroked line segment.
    ///
    /// Always tessellated (no instanced stroke pipeline).
    /// `line` does not read opacity — no opacity baking is performed.
    pub(super) fn line(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
        paint: &Paint,
    ) {
        self.prime_tessellator_scale(state);
        match self.tessellator.tessellate_line(p1, p2, paint) {
            Ok((vertices, indices)) => {
                #[cfg(debug_assertions)]
                tracing::trace!(
                    "DrawBatcher::line: {} vertices, {} indices",
                    vertices.len(),
                    indices.len()
                );
                Self::submit_transformed_geometry(
                    segment,
                    draw_order,
                    state,
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
            Err(e) => {
                tracing::error!("DrawBatcher::line: tessellation failed — {}", e);
            }
        }
    }

    /// Record a multi-layer approximated drop shadow for `path`.
    ///
    /// # State mutation
    ///
    /// This method takes `state: &mut GpuStateStack` because each blur layer
    /// applies a per-layer translate via `state.save()` / `state.translate()`
    /// / `state.restore()`.  The save/restore balance is maintained strictly:
    /// every iteration pushes exactly one save and pops it before the next
    /// iteration.  The net depth change across the entire call is zero, so the
    /// T7 frame-boundary `debug_assert_balanced` remains satisfied.
    ///
    /// # Algorithm
    ///
    /// Material Design-style multi-pass approximation: the shadow path is
    /// tessellated `num_layers` times (≤ 8) with geometrically decreasing alpha
    /// to simulate radial blur.  The tessellator scale is primed **once** before
    /// the loop — the per-layer `translate` does not change scale, so the
    /// flatten tolerance captured before the loop is correct for every layer.
    ///
    /// `draw_shadow` does not read opacity — no opacity baking is performed.
    pub(super) fn draw_shadow(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &mut GpuStateStack,
        path: &Path,
        color: Color,
        elevation: f32,
    ) {
        let blur_radius = elevation.max(0.0);
        let offset_y = elevation / 2.0;

        if blur_radius < 0.1 {
            return;
        }

        // Max 8 layers for performance.
        let num_layers = (blur_radius / 2.0).ceil().min(8.0) as usize;

        if num_layers == 0 {
            return;
        }

        let alpha_per_layer = f32::from(color.a) / num_layers as f32;

        // Prime the tessellator's flatten tolerance to the current CTM scale so
        // shadow curves don't facet on HiDPI / scaled frames. The per-layer
        // `translate` below only shifts the path (no scale change), so the scale
        // captured here is correct for every layer. Without this, the shadow
        // path would tessellate at whatever `max_scale` a previous draw left
        // behind (stale-scale hazard).
        self.prime_tessellator_scale(state);

        for i in 0..num_layers {
            let offset_scale = (i as f32 + 1.0) / num_layers as f32;
            let current_blur = blur_radius * offset_scale;

            let shadow_alpha = (alpha_per_layer * (1.0 - offset_scale * 0.5)) as u8;
            let shadow_color = Color::rgba(color.r, color.g, color.b, shadow_alpha);
            let shadow_paint = Paint::fill(shadow_color);

            // Push a per-layer translate so the tessellated geometry is baked
            // at the offset position (`submit_transformed_geometry` reads the
            // CTM at call time — shape.wgsl has no model matrix).
            state.save();
            state.translate(Offset::new(
                px(current_blur * 0.5),
                px(offset_y + current_blur * 0.5),
            ));

            match self
                .tessellator
                .tessellate_flui_path_fill(path, &shadow_paint)
            {
                Ok((vertices, indices)) => {
                    Self::submit_transformed_geometry(
                        segment,
                        draw_order,
                        state,
                        vertices,
                        &indices,
                        PipelineKey::alpha_blend(),
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to tessellate shadow path: {}", e);
                }
            }

            state.restore();
        }
    }
}
