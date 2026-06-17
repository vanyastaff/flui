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
//! `dispatch_shader_rect` (gradient/shader fills for rect/rrect/circle) lives on
//! `DrawBatcher` (T9c).  The gradient methods (`gradient_rect`,
//! `radial_gradient_rect`, `sweep_gradient_rect`) and `shadow_rect` also live here,
//! taking `(&mut DrawSegment, &GpuStateStack, …)` via the same borrow seam.
//! Each painter shim (`rect`/`rrect`/`circle`) folds the shader pre-check into the
//! batcher call; the shim becomes a thin opacity-read + delegation.
//!
//! `draw_path` and `draw_vertices` also live here (T9d), using the same seam.
//! `draw_path` owns the tessellation cache hit/miss logic; `draw_vertices` owns
//! the per-vertex color/uv assembly and u16→u32 index conversion.
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
    painting::{Shader, path::Path},
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
    /// Called by `DrawBatcher::dispatch_shader_rect` which lives in the same module.
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
    // Each method (rect/rrect/circle) dispatches the shader/gradient case first
    // via `self.dispatch_shader_rect(…)` (T9c), then handles solid fills and
    // strokes.  Painter shims are now thin: opacity-read + delegation.

    /// Record a filled rectangle or a stroked rectangle.
    ///
    /// Shader/gradient fills are dispatched first (T9c): when `paint.style` is
    /// `Fill` and `paint.has_shader()`, `dispatch_shader_rect` is called and the
    /// method returns early.  The non-shader fill and stroke paths are unchanged
    /// from T9a.
    pub(super) fn rect(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        opacity: f32,
        rect: Rect<Pixels>,
        paint: &Paint,
    ) {
        // Shader/gradient fill — dispatch before any opacity or color work.
        if paint.style == PaintStyle::Fill && paint.has_shader() {
            if paint.blend_mode != BlendMode::SrcOver {
                static GRADIENT_BLEND_WARNED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !GRADIENT_BLEND_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    tracing::warn!(
                        blend_mode = ?paint.blend_mode,
                        "gradient/shader fill with blend_mode {:?} is not supported by the \
                         Phase A fixed-function path; rendering as SrcOver. \
                         Phase B will add dst-sample blended gradients. (logged once per process)",
                        paint.blend_mode,
                    );
                }
            }
            if Self::dispatch_shader_rect(segment, state, rect, paint, [0.0; 4]) {
                return;
            }
        }

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

    /// Record a filled rounded rectangle or a stroked one.
    ///
    /// Shader/gradient fills are dispatched first (T9c): when `paint.style` is
    /// `Fill` and `paint.has_shader()`, `dispatch_shader_rect` is called with the
    /// max per-corner radii and returns early.  The non-shader fill and stroke
    /// paths are unchanged from T9a.
    pub(super) fn rrect(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        opacity: f32,
        rrect: RRect,
        paint: &Paint,
    ) {
        // Shader/gradient fill — dispatch before any opacity or color work.
        if paint.style == PaintStyle::Fill && paint.has_shader() {
            if paint.blend_mode != BlendMode::SrcOver {
                static GRADIENT_BLEND_WARNED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !GRADIENT_BLEND_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    tracing::warn!(
                        blend_mode = ?paint.blend_mode,
                        "gradient/shader rrect fill with blend_mode {:?} is not supported by \
                         the Phase A fixed-function path; rendering as SrcOver. \
                         Phase B will add dst-sample blended gradients. (logged once per process)",
                        paint.blend_mode,
                    );
                }
            }
            let corner_radii = [
                rrect.top_left.x.0.max(rrect.top_left.y.0),
                rrect.top_right.x.0.max(rrect.top_right.y.0),
                rrect.bottom_right.x.0.max(rrect.bottom_right.y.0),
                rrect.bottom_left.x.0.max(rrect.bottom_left.y.0),
            ];
            if Self::dispatch_shader_rect(
                segment,
                state,
                rrect.bounding_rect(),
                paint,
                corner_radii,
            ) {
                return;
            }
        }

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

    /// Record a filled circle or a stroked circle.
    ///
    /// Shader/gradient fills are dispatched first (T9c): when `paint.style` is
    /// `Fill` and `paint.has_shader()`, `dispatch_shader_rect` is called with
    /// `[radius; 4]` corner radii and the center±radius bounding rect, then
    /// returns early.  The non-shader fill and stroke paths are unchanged from T9a.
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
        // Shader/gradient fill — dispatch before any opacity or color work.
        if paint.style == PaintStyle::Fill && paint.has_shader() {
            if paint.blend_mode != BlendMode::SrcOver {
                static GRADIENT_BLEND_WARNED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !GRADIENT_BLEND_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    tracing::warn!(
                        blend_mode = ?paint.blend_mode,
                        "gradient/shader circle fill with blend_mode {:?} is not supported by \
                         the Phase A fixed-function path; rendering as SrcOver. \
                         Phase B will add dst-sample blended gradients. (logged once per process)",
                        paint.blend_mode,
                    );
                }
            }
            let bounds = Rect::from_xywh(
                center.x - px(radius),
                center.y - px(radius),
                px(radius * 2.0),
                px(radius * 2.0),
            );
            if Self::dispatch_shader_rect(segment, state, bounds, paint, [radius; 4]) {
                return;
            }
        }

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

    /// Record a rectangle with a linear gradient.
    ///
    /// Takes `segment` and `state` as disjoint borrows (borrow seam, T9c).
    /// No draw-order slot is consumed — gradient instances are instanced (no
    /// tessellation, no non-`SrcOver` seal).
    ///
    /// # Arguments
    /// * `segment`         — current accumulation buffer
    /// * `state`           — read-only transform/scissor queries
    /// * `bounds`          — rectangle bounds (already in transformed space)
    /// * `gradient_start`  — gradient start point (local to `bounds`)
    /// * `gradient_end`    — gradient end point (local to `bounds`)
    /// * `stops`           — gradient color stops (max 8)
    /// * `corner_radius`   — uniform corner radius (0.0 = sharp)
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/state are disjoint WgpuPainter fields; \
                  the remaining args mirror the gradient's own parameters"
    )]
    pub(super) fn gradient_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<Pixels>,
        gradient_start: glam::Vec2,
        gradient_end: glam::Vec2,
        stops: &[super::effects::GradientStop],
        corner_radius: f32,
    ) {
        use super::instancing::LinearGradientInstance;

        // Append gradient stops to global buffer (max 8 per gradient).
        let stop_count = stops.len().min(8);
        let current_len = segment.current_gradient_stops.len();
        if current_len + stop_count > super::effects_pipeline::MAX_GRADIENT_STOPS {
            // Logged once per process: a >MAX_GRADIENT_STOPS frame would otherwise
            // spam this for every overflowing instance, every frame.
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!(
                    current_stops = current_len,
                    requested = stop_count,
                    limit = super::effects_pipeline::MAX_GRADIENT_STOPS,
                    "gradient_rect: gradient stop buffer full; dropping linear gradient \
                     instance (logged once per process)"
                );
            }
            return;
        }
        let stop_offset = current_len as u32;
        segment
            .current_gradient_stops
            .extend_from_slice(&stops[..stop_count]);

        let instance = LinearGradientInstance::new(
            [
                bounds.left().0,
                bounds.top().0,
                bounds.width().0,
                bounds.height().0,
            ],
            gradient_start,
            gradient_end,
            [corner_radius; 4],
            stop_count as u32,
        )
        .with_stop_offset(stop_offset);

        let _ = segment.linear_gradient_batch.add(instance);
        DrawSegment::push_scissor_region(
            &mut segment.linear_grad_scissors,
            state.current_scissor(),
        );
    }

    /// Record a rectangle with a radial gradient.
    ///
    /// Takes `segment` and `state` as disjoint borrows (borrow seam, T9c).
    /// No draw-order slot — instanced, no tessellation.
    ///
    /// # Arguments
    /// * `segment`        — current accumulation buffer
    /// * `state`          — read-only transform/scissor queries
    /// * `bounds`         — rectangle bounds (already in transformed space)
    /// * `center`         — gradient center (local to `bounds`)
    /// * `radius`         — gradient radius
    /// * `stops`          — gradient color stops (max 8)
    /// * `corner_radius`  — uniform corner radius (0.0 = sharp)
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/state are disjoint WgpuPainter fields; \
                  the remaining args mirror the gradient's own parameters"
    )]
    pub(super) fn radial_gradient_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<Pixels>,
        center: glam::Vec2,
        radius: f32,
        stops: &[super::effects::GradientStop],
        corner_radius: f32,
    ) {
        use super::instancing::RadialGradientInstance;

        let stop_count = stops.len().min(8);
        let current_len = segment.current_gradient_stops.len();
        if current_len + stop_count > super::effects_pipeline::MAX_GRADIENT_STOPS {
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!(
                    current_stops = current_len,
                    requested = stop_count,
                    limit = super::effects_pipeline::MAX_GRADIENT_STOPS,
                    "radial_gradient_rect: gradient stop buffer full; dropping radial \
                     gradient instance (logged once per process)"
                );
            }
            return;
        }
        let stop_offset = current_len as u32;
        segment
            .current_gradient_stops
            .extend_from_slice(&stops[..stop_count]);

        let instance = RadialGradientInstance::new(
            [
                bounds.left().0,
                bounds.top().0,
                bounds.width().0,
                bounds.height().0,
            ],
            center,
            radius,
            [corner_radius; 4],
            stop_count as u32,
        )
        .with_stop_offset(stop_offset);

        let _ = segment.radial_gradient_batch.add(instance);
        DrawSegment::push_scissor_region(
            &mut segment.radial_grad_scissors,
            state.current_scissor(),
        );
    }

    /// Record a rectangle with a sweep (angular/conic) gradient.
    ///
    /// Takes `segment` and `state` as disjoint borrows (borrow seam, T9c).
    /// No draw-order slot — instanced, no tessellation.
    ///
    /// # Arguments
    /// * `segment`      — current accumulation buffer
    /// * `state`        — read-only transform/scissor queries
    /// * `bounds`       — rectangle bounds (already in transformed space)
    /// * `center`       — gradient center (local to `bounds`)
    /// * `start_angle`  — start angle in radians
    /// * `end_angle`    — end angle in radians
    /// * `stops`        — gradient color stops (max 8)
    /// * `corner_radius`— uniform corner radius (0.0 = sharp)
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/state are disjoint WgpuPainter fields; \
                  the remaining args mirror the gradient's own parameters"
    )]
    pub(super) fn sweep_gradient_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<Pixels>,
        center: glam::Vec2,
        start_angle: f32,
        end_angle: f32,
        stops: &[super::effects::GradientStop],
        corner_radius: f32,
    ) {
        use super::instancing::SweepGradientInstance;

        let stop_count = stops.len().min(8);
        let current_len = segment.current_gradient_stops.len();
        if current_len + stop_count > super::effects_pipeline::MAX_GRADIENT_STOPS {
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!(
                    current_stops = current_len,
                    requested = stop_count,
                    limit = super::effects_pipeline::MAX_GRADIENT_STOPS,
                    "sweep_gradient_rect: gradient stop buffer full; dropping sweep \
                     gradient instance (logged once per process)"
                );
            }
            return;
        }
        let stop_offset = current_len as u32;
        segment
            .current_gradient_stops
            .extend_from_slice(&stops[..stop_count]);

        let instance = SweepGradientInstance::new(
            [
                bounds.left().0,
                bounds.top().0,
                bounds.width().0,
                bounds.height().0,
            ],
            center,
            start_angle,
            end_angle,
            [corner_radius; 4],
            stop_count as u32,
        )
        .with_stop_offset(stop_offset);

        let _ = segment.sweep_gradient_batch.add(instance);
        DrawSegment::push_scissor_region(&mut segment.sweep_grad_scissors, state.current_scissor());
    }

    /// Record an analytical shadow for a rectangle (Evan Wallace technique).
    ///
    /// Single-pass O(1) rendering; quality is indistinguishable from a real
    /// Gaussian blur at typical shadow radii.  Instanced — no draw-order slot,
    /// no tessellation.
    ///
    /// # Arguments
    /// * `segment`        — current accumulation buffer
    /// * `rect_pos`       — rectangle position [x, y]
    /// * `rect_size`      — rectangle size [width, height]
    /// * `corner_radius`  — uniform corner radius
    /// * `params`         — shadow offset, blur sigma, and color
    pub(super) fn shadow_rect(
        segment: &mut DrawSegment,
        rect_pos: [f32; 2],
        rect_size: [f32; 2],
        corner_radius: f32,
        params: &super::effects::ShadowParams,
    ) {
        use super::instancing::ShadowInstance;

        let instance = ShadowInstance::new(rect_pos, rect_size, corner_radius, params);
        let _ = segment.shadow_batch.add(instance);
    }

    /// Dispatch a filled rect/rrect/circle with a shader paint to the correct
    /// gradient pipeline.  Returns `true` if the shader was handled; `false`
    /// means fall through to solid-color fill.
    ///
    /// Reads `state` for the current-transform (to convert local bounds to
    /// device space) and calls the appropriate `gradient_rect` /
    /// `radial_gradient_rect` / `sweep_gradient_rect` associated function.
    ///
    /// Callers must check `paint.has_shader()` **and** `paint.style ==
    /// PaintStyle::Fill` before calling this; it is not rechecked here.
    ///
    /// # Arguments
    /// * `segment`       — current accumulation buffer
    /// * `state`         — read-only transform/scissor queries
    /// * `bounds`        — local-space bounds of the shape
    /// * `paint`         — fill paint carrying the shader
    /// * `corner_radii`  — per-corner radii `[tl, tr, br, bl]`
    pub(super) fn dispatch_shader_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<Pixels>,
        paint: &Paint,
        corner_radii: [f32; 4],
    ) -> bool {
        let Some(shader) = &paint.shader else {
            return false;
        };

        let stops = Self::shader_to_gradient_stops(shader);
        if stops.is_empty() {
            return false;
        }

        // Compute transformed bounds using the read-only state — read before any
        // mutable gradient call so there is no aliasing.
        let top_left = state.apply_transform(Point::new(bounds.left(), bounds.top()));
        let bottom_right = state.apply_transform(Point::new(bounds.right(), bounds.bottom()));
        let transformed = Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);

        match shader {
            Shader::LinearGradient { from, to, .. } => {
                let start =
                    glam::Vec2::new(from.dx.0 - bounds.left().0, from.dy.0 - bounds.top().0);
                let end = glam::Vec2::new(to.dx.0 - bounds.left().0, to.dy.0 - bounds.top().0);
                Self::gradient_rect(
                    segment,
                    state,
                    transformed,
                    start,
                    end,
                    &stops,
                    corner_radii[0],
                );
            }
            Shader::RadialGradient { center, radius, .. } => {
                let c =
                    glam::Vec2::new(center.dx.0 - bounds.left().0, center.dy.0 - bounds.top().0);
                Self::radial_gradient_rect(
                    segment,
                    state,
                    transformed,
                    c,
                    *radius,
                    &stops,
                    corner_radii[0],
                );
            }
            Shader::SweepGradient {
                center,
                start_angle,
                end_angle,
                ..
            } => {
                let c =
                    glam::Vec2::new(center.dx.0 - bounds.left().0, center.dy.0 - bounds.top().0);
                Self::sweep_gradient_rect(
                    segment,
                    state,
                    transformed,
                    c,
                    *start_angle,
                    *end_angle,
                    &stops,
                    corner_radii[0],
                );
            }
            Shader::Solid { .. } | _ => return false,
        }

        true
    }

    /// Record a filled or stroked path, using the per-frame tessellation cache
    /// to avoid re-tessellating identical paths.
    ///
    /// # Branch ordering (must be preserved for byte-identical output)
    ///
    /// 1. Prime the tessellator flatten-tolerance from the current CTM scale.
    /// 2. **Dashed-stroke early return** — dashes are not cached (the pattern
    ///    affects geometry but is not part of `compute_path_hash`; caching
    ///    would collide a solid and a dashed stroke of the same path).
    /// 3. **Cache hit** — reconstruct `Vertex`s from UNTRANSFORMED cached
    ///    positions with the *current* `paint.color`, then submit.
    /// 4. **Cache miss** — tessellate fill or stroke, extract untransformed
    ///    positions, store in cache, then submit.
    ///
    /// `draw_path` does not read opacity — no opacity baking is performed,
    /// consistent with the other record methods in this module.
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/draw_order/state are disjoint WgpuPainter fields \
                  passed as separate borrows; path geometry parameters are all necessary"
    )]
    pub(super) fn draw_path(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        path: &Path,
        paint: &Paint,
    ) {
        // Snapshot world scale once: it drives flatten-tolerance in the tessellator
        // AND the cache-key bucket, so a single read guarantees they can never desync
        // (scale-1 geometry must not be reused at scale 8, which would facet).
        let max_scale = state.max_scale();
        self.tessellator.set_max_scale(max_scale);

        // Dashed strokes cannot use the path cache: the dash pattern affects
        // geometry but is not part of compute_path_hash, so caching would
        // collide a solid and a dashed stroke of the same path.
        if paint.style != PaintStyle::Fill
            && let Some(ref dash) = paint.dash_pattern
        {
            match self
                .tessellator
                .tessellate_flui_path_dashed_stroke(path, paint, dash)
            {
                Ok((vertices, indices)) => {
                    // Bake current_transform into vertices: shape.wgsl has no model matrix.
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
                    tracing::warn!("Failed to tessellate dashed path stroke: {}", e);
                }
            }
            return;
        }

        // Compute cache key from path geometry + paint tessellation parameters
        // + the quantized world scale (so a scale-1 entry is not reused at a
        // larger scale with scale-1 chord density).
        let path_hash = PathCache::compute_path_hash(
            path,
            paint.style,
            paint.stroke_width,
            paint.stroke_cap,
            paint.stroke_join,
            max_scale,
        );

        // Check cache for previously tessellated geometry
        if let Some((positions, cached_indices)) = self.path_cache.get(path_hash) {
            // Reconstruct full Vertex data with current paint color.
            // The cache stores UNTRANSFORMED positions; bake the current transform now.
            let rgba = paint.color.to_rgba_f32_array();
            let vertices: Vec<Vertex> = positions
                .iter()
                .map(|&pos| Vertex::new(pos, rgba, [0.0, 0.0]))
                .collect();
            let indices: Vec<u32> = cached_indices.to_vec();
            // Bake current_transform into vertices: shape.wgsl has no model matrix.
            Self::submit_transformed_geometry(
                segment,
                draw_order,
                state,
                vertices,
                &indices,
                pipeline::pipeline_key_from_paint(paint),
            );
            return;
        }

        // Cache miss — tessellate and store
        let result = if paint.style == PaintStyle::Fill {
            self.tessellator.tessellate_flui_path_fill(path, paint)
        } else {
            self.tessellator.tessellate_flui_path_stroke(path, paint)
        };

        match result {
            Ok((vertices, indices)) => {
                // Extract position data for cache BEFORE baking the transform.
                // The cache stores local (untransformed) positions so that cached
                // geometry can be re-used across frames with different transforms.
                let positions: Vec<[f32; 2]> = vertices.iter().map(|v| v.position).collect();
                self.path_cache
                    .insert(path_hash, positions, indices.clone());

                // Bake current_transform into vertices: shape.wgsl has no model matrix.
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
                tracing::warn!("Failed to tessellate path: {}", e);
            }
        }
    }

    /// Draw indexed triangle geometry with per-vertex color + uv.
    ///
    /// # Validation
    ///
    /// Returns early (silently) on empty input or a color-count mismatch in
    /// debug builds (the mismatch is logged via `tracing::error!`).
    ///
    /// # `tex_coords` parameter
    ///
    /// The per-vertex uv extraction IS implemented (the `tex_coords` slice is
    /// consumed at the per-vertex loop, copied into `Vertex::tex_coord`, and
    /// baked into the GPU vertex buffer).  What is NOT yet wired is the
    /// **texture-binding pipeline path**: `pipeline_key_from_paint(paint)`
    /// returns a solid-color pipeline today, so the uv values reach the vertex
    /// shader but the fragment shader has no texture to sample.  A textured
    /// pipeline-key variant is a follow-up audit item; until then `tex_coords`
    /// callers pre-populate the vertex stream for forward-compat.
    ///
    /// `draw_vertices` does not read opacity — no opacity baking is performed,
    /// consistent with the other record methods in this module.
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/draw_order/state are disjoint WgpuPainter fields \
                  passed as separate borrows; vertex geometry slices are all necessary"
    )]
    pub(super) fn draw_vertices(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        vertices: &[Point<Pixels>],
        colors: Option<&[Color]>,
        tex_coords: Option<&[Point<Pixels>]>,
        indices: &[u16],
        paint: &Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "DrawBatcher::draw_vertices: vertices={}, indices={}",
            vertices.len(),
            indices.len()
        );

        // Validate input
        if vertices.is_empty() || indices.is_empty() {
            return;
        }

        if let Some(colors_arr) = colors
            && colors_arr.len() != vertices.len()
        {
            #[cfg(debug_assertions)]
            tracing::error!(
                "DrawVertices: color count ({}) doesn't match vertex count ({})",
                colors_arr.len(),
                vertices.len()
            );
            return;
        }

        // Convert to our Vertex format
        let default_color = paint.color;
        let gpu_vertices: Vec<Vertex> = vertices
            .iter()
            .enumerate()
            .map(|(i, pos)| {
                let color = colors
                    .and_then(|c| c.get(i))
                    .copied()
                    .unwrap_or(default_color);

                let uv = tex_coords
                    .and_then(|tc| tc.get(i))
                    .map_or([0.0, 0.0], |p| [p.x.0, p.y.0]);

                Vertex {
                    position: [pos.x.0, pos.y.0],
                    color: color.to_f32_array(),
                    tex_coord: uv,
                }
            })
            .collect();

        // Convert indices to u32
        let gpu_indices: Vec<u32> = indices.iter().map(|&i| u32::from(i)).collect();

        // Add to tessellated geometry (bypassing tessellator since we already have
        // triangles).  Bake current_transform into vertex positions: shape.wgsl has
        // no model-matrix uniform.
        Self::submit_transformed_geometry(
            segment,
            draw_order,
            state,
            gpu_vertices,
            &gpu_indices,
            pipeline::pipeline_key_from_paint(paint),
        );
    }
}
