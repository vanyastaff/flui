//! Gradient and shader-dispatch record methods: gradient_rect, radial_gradient_rect,
//! sweep_gradient_rect, shadow_rect, dispatch_shader_rect.

use flui_foundation::geometry::{Point, Rect};
use flui_painting::paint::Shader;
use flui_painting::{BlendMode, Paint};

use super::{
    super::{
        command_ir::{DrawRun, DrawSegment},
        effects::GradientStop,
        state_stack::GpuStateStack,
    },
    DrawBatcher,
};

// GPU rendering routinely converts between f32/u8/u32 for pixel coordinates,
// color channels, and buffer indices. These truncations are intentional.
/// Names the gradient kind in its own overflow warning, so three callers can
/// share one implementation without losing which one fired.
///
/// Kept for `tracing`'s own message rather than as a `Display` type: the
/// warning is the only consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GradientKind {
    Linear,
    Radial,
    Sweep,
}

impl GradientKind {
    const fn operation(self) -> &'static str {
        match self {
            Self::Linear => "gradient_rect",
            Self::Radial => "radial_gradient_rect",
            Self::Sweep => "sweep_gradient_rect",
        }
    }
}

// The fragment shaders scan stops linearly. Bound per-pixel work independently
// of frame storage admission; retain gradients beyond the former eight-stop cap.
const MAX_GRADIENT_STOPS: usize = 256;

/// Checked indexing for the immutable table; failures remain authoritative at seal.
fn reserve_gradient_stops(
    segment: &mut DrawSegment,
    kind: GradientKind,
    stops: &[GradientStop],
) -> Option<u32> {
    if stops.len() > MAX_GRADIENT_STOPS {
        segment.record_limit("gradient stops per draw", stops.len(), MAX_GRADIENT_STOPS);
        return None;
    }
    let current_len = segment.current_gradient_stops.len();
    let Some(total) = current_len.checked_add(stops.len()) else {
        segment.record_limit(kind.operation(), usize::MAX, u32::MAX as usize);
        return None;
    };
    if total > u32::MAX as usize {
        segment.record_limit(kind.operation(), total, u32::MAX as usize);
        return None;
    }
    Some(current_len as u32)
}

impl DrawBatcher {
    /// Record a rectangle with a linear gradient.
    ///
    /// Takes `segment` and `state` as disjoint borrows.
    /// No draw-order slot is consumed — gradient instances are instanced (no
    /// tessellation, no non-`SrcOver` seal).
    ///
    /// # Arguments
    /// * `segment`         — current accumulation buffer
    /// * `state`           — read-only transform/scissor queries
    /// * `bounds`          — rectangle bounds (already in transformed space)
    /// * `gradient_start`  — gradient start point (local to `bounds`)
    /// * `gradient_end`    — gradient end point (local to `bounds`)
    /// * `stops`           — validated gradient color stops
    /// * `corner_radii`    — per-corner radii `[tl, tr, br, bl]` (0.0 = sharp)
    /// * `blend`           — the paint's fixed-function blend mode (never advanced)
    #[expect(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/state are disjoint WgpuPainter fields; \
                  the remaining args mirror the gradient's own parameters"
    )]
    pub(in super::super) fn draw_gradient_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<f64>,
        gradient_start: glam::Vec2,
        gradient_end: glam::Vec2,
        stops: &[GradientStop],
        corner_radii: [f32; 4],
        blend: BlendMode,
    ) {
        if segment.recording_result().is_err() {
            return;
        }

        use crate::instancing::LinearGradientInstance;

        let Some(stop_offset) = reserve_gradient_stops(segment, GradientKind::Linear, stops) else {
            return;
        };
        let stop_count = stops.len();
        segment
            .current_gradient_stops
            .extend_from_slice(&stops[..stop_count]);

        let instance = LinearGradientInstance::new(
            [
                (bounds.left() as f32),
                (bounds.top() as f32),
                (bounds.width() as f32),
                (bounds.height() as f32),
            ],
            gradient_start,
            gradient_end,
            corner_radii,
            stop_count as u32,
        )
        .with_stop_offset(stop_offset);
        let instance = state.apply_active_clip(instance);

        let _ = segment.linear_gradient_batch.add(instance);
        segment.record_run(DrawRun::LinearGradient(
            segment.linear_gradient_batch.len().saturating_sub(1)
                ..segment.linear_gradient_batch.len(),
        ));
        DrawSegment::push_gradient_run(
            &mut segment.linear_gradient_runs,
            state.current_scissor(),
            blend,
        );
    }

    /// Record a rectangle with a radial gradient.
    ///
    /// Takes `segment` and `state` as disjoint borrows.
    /// No draw-order slot — instanced, no tessellation.
    ///
    /// # Arguments
    /// * `segment`        — current accumulation buffer
    /// * `state`          — read-only transform/scissor queries
    /// * `bounds`         — rectangle bounds (already in transformed space)
    /// * `center`         — gradient center (local to `bounds`)
    /// * `radius`         — gradient radius
    /// * `stops`          — validated gradient color stops
    /// * `corner_radii`   — per-corner radii `[tl, tr, br, bl]` (0.0 = sharp)
    /// * `blend`          — the paint's fixed-function blend mode (never advanced)
    #[expect(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/state are disjoint WgpuPainter fields; \
                  the remaining args mirror the gradient's own parameters"
    )]
    pub(in super::super) fn draw_radial_gradient_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<f64>,
        center: glam::Vec2,
        radius: f32,
        stops: &[GradientStop],
        corner_radii: [f32; 4],
        blend: BlendMode,
    ) {
        if segment.recording_result().is_err() {
            return;
        }

        use crate::instancing::RadialGradientInstance;

        let Some(stop_offset) = reserve_gradient_stops(segment, GradientKind::Radial, stops) else {
            return;
        };
        let stop_count = stops.len();
        segment
            .current_gradient_stops
            .extend_from_slice(&stops[..stop_count]);

        let instance = RadialGradientInstance::new(
            [
                (bounds.left() as f32),
                (bounds.top() as f32),
                (bounds.width() as f32),
                (bounds.height() as f32),
            ],
            center,
            radius,
            corner_radii,
            stop_count as u32,
        )
        .with_stop_offset(stop_offset);
        let instance = state.apply_active_clip(instance);

        let _ = segment.radial_gradient_batch.add(instance);
        segment.record_run(DrawRun::RadialGradient(
            segment.radial_gradient_batch.len().saturating_sub(1)
                ..segment.radial_gradient_batch.len(),
        ));
        DrawSegment::push_gradient_run(
            &mut segment.radial_gradient_runs,
            state.current_scissor(),
            blend,
        );
    }

    /// Record a rectangle with a sweep (angular/conic) gradient.
    ///
    /// Takes `segment` and `state` as disjoint borrows.
    /// No draw-order slot — instanced, no tessellation.
    ///
    /// # Arguments
    /// * `segment`      — current accumulation buffer
    /// * `state`        — read-only transform/scissor queries
    /// * `bounds`       — rectangle bounds (already in transformed space)
    /// * `center`       — gradient center (local to `bounds`)
    /// * `start_angle`  — start angle in radians
    /// * `end_angle`    — end angle in radians
    /// * `stops`        — validated gradient color stops
    /// * `corner_radii` — per-corner radii `[tl, tr, br, bl]` (0.0 = sharp)
    /// * `blend`        — the paint's fixed-function blend mode (never advanced)
    #[expect(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/state are disjoint WgpuPainter fields; \
                  the remaining args mirror the gradient's own parameters"
    )]
    pub(in super::super) fn draw_sweep_gradient_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<f64>,
        center: glam::Vec2,
        start_angle: f32,
        end_angle: f32,
        stops: &[GradientStop],
        corner_radii: [f32; 4],
        blend: BlendMode,
    ) {
        if segment.recording_result().is_err() {
            return;
        }

        use crate::instancing::SweepGradientInstance;

        let Some(stop_offset) = reserve_gradient_stops(segment, GradientKind::Sweep, stops) else {
            return;
        };
        let stop_count = stops.len();
        segment
            .current_gradient_stops
            .extend_from_slice(&stops[..stop_count]);

        let instance = SweepGradientInstance::new(
            [
                (bounds.left() as f32),
                (bounds.top() as f32),
                (bounds.width() as f32),
                (bounds.height() as f32),
            ],
            center,
            start_angle,
            end_angle,
            corner_radii,
            stop_count as u32,
        )
        .with_stop_offset(stop_offset);
        let instance = state.apply_active_clip(instance);

        let _ = segment.sweep_gradient_batch.add(instance);
        segment.record_run(DrawRun::SweepGradient(
            segment.sweep_gradient_batch.len().saturating_sub(1)
                ..segment.sweep_gradient_batch.len(),
        ));
        DrawSegment::push_gradient_run(
            &mut segment.sweep_gradient_runs,
            state.current_scissor(),
            blend,
        );
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
    pub(in super::super) fn draw_shadow_rect(
        segment: &mut DrawSegment,
        rect_pos: [f32; 2],
        rect_size: [f32; 2],
        corner_radius: f32,
        params: &crate::effects::ShadowParams,
    ) {
        if segment.recording_result().is_err() {
            return;
        }

        use crate::instancing::ShadowInstance;

        let instance = ShadowInstance::new(rect_pos, rect_size, corner_radius, params);
        let _ = segment.shadow_batch.add(instance);
        segment.record_run(DrawRun::Shadow(
            segment.shadow_batch.len().saturating_sub(1)..segment.shadow_batch.len(),
        ));
    }

    /// Dispatch a filled rect/rrect/circle with a shader paint to the correct
    /// gradient pipeline, or divert to an isolated `DrawItem::AdvancedShape`
    /// when the paint carries an advanced (dst-read) blend mode.
    ///
    /// Returns `true` if the shader was handled; `false` means fall through to
    /// solid-color fill.
    ///
    /// When `paint.blend_mode.is_advanced()`:
    ///   1. The current segment is sealed (Z-order guarantee: prior content lands
    ///      on the surface before the backdrop is copied).
    ///   2. The gradient instance + its stops are isolated in a fresh
    ///      `DrawSegment` with stop offsets relative to that fresh buffer.
    ///   3. The `DrawSegment` is wrapped in `DrawItem::AdvancedShape` and pushed
    ///      onto `draw_order` — `flush_advanced_layer` picks it up at replay.
    ///
    /// Every non-advanced mode reaches the instanced gradient pipelines with
    /// `paint.blend_mode` recorded on its draw run, which is what keys the
    /// pipeline; `SrcOver` is one of them rather than the only one.
    ///
    /// # AA note
    ///
    /// Gradients are shader-AA (analytic SDF), not rasterised geometry; they have
    /// smooth sub-pixel anti-aliasing independent of the tessellation path.
    ///
    /// Callers must check `paint.has_shader()` **and** `paint.style ==
    /// PaintStyle::Fill` before calling this; it is not rechecked here.
    ///
    /// # Arguments
    /// * `segment`       — current accumulation buffer
    /// * `draw_order`    — ordered list of sealed segments / advanced ops
    /// * `state`         — read-only transform/scissor queries
    /// * `bounds`        — local-space bounds of the shape
    /// * `paint`         — fill paint carrying the shader
    /// * `corner_radii`  — per-corner radii `[tl, tr, br, bl]`
    pub(in super::super) fn dispatch_shader_rect(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<crate::command_ir::DrawItem>,
        state: &GpuStateStack,
        bounds: Rect<f64>,
        paint: &Paint,
        corner_radii: [f32; 4],
    ) -> bool {
        if segment.recording_result().is_err() {
            return true;
        }

        let Some(shader) = &paint.shader else {
            return false;
        };

        let color_count = match shader {
            Shader::LinearGradient { colors, .. }
            | Shader::RadialGradient { colors, .. }
            | Shader::SweepGradient { colors, .. } => colors.len(),
            _ => 0,
        };
        if color_count > MAX_GRADIENT_STOPS {
            segment.record_limit("gradient stops per draw", color_count, MAX_GRADIENT_STOPS);
            return true;
        }
        let stops = Self::shader_to_gradient_stops(shader, &segment.budget);
        if stops.is_empty() {
            return false;
        }

        // Compute transformed bounds using the read-only state — read before any
        // mutable gradient call so there is no aliasing.
        let top_left = state.apply_transform(Point::new(bounds.left(), bounds.top()));
        let bottom_right = state.apply_transform(Point::new(bounds.right(), bounds.bottom()));
        let transformed = Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);

        // ── Advanced (dst-read) diversion ─────────────────────────────────────
        //
        // Advanced blend modes cannot be expressed as fixed-function blends and
        // require a backdrop read.  Isolate the gradient into its own DrawSegment
        // so flush_advanced_layer can composite it correctly.
        //
        // Condition: must divert here because gradients bypass
        // `add_tessellated_with_key` (they are instanced, not tessellated), so
        // the diversion in that funnel does not fire.
        //
        // Keying the ordinary modes by pipeline did not make this branch
        // redundant: an advanced mode has no fixed-function blend state to key
        // a pipeline BY, which is what `blend_state_for`'s defensive SrcOver arm
        // and `GradientPipelines::ensure`'s debug assertion both say. It stays
        // ahead of the keyed path, and it stays first.
        if paint.blend_mode.is_advanced() {
            // Step 1: seal prior content so it lands on the surface before the
            // backdrop is sampled.
            super::DrawBatcher::finish_current_segment(segment, draw_order);

            // Step 2: build an isolated DrawSegment carrying exactly this gradient.
            // Stop offsets in the fresh segment start at 0.
            let stop_count = stops.len();
            let mut shape_segment = segment.empty_sibling();
            shape_segment
                .current_gradient_stops
                .extend_from_slice(&stops[..stop_count]);

            match shader {
                Shader::LinearGradient { from, to, .. } => {
                    use crate::instancing::LinearGradientInstance;
                    let start = glam::Vec2::new(
                        (from.dx - bounds.left()) as f32,
                        (from.dy - bounds.top()) as f32,
                    );
                    let end = glam::Vec2::new(
                        (to.dx - bounds.left()) as f32,
                        (to.dy - bounds.top()) as f32,
                    );
                    let instance = LinearGradientInstance::new(
                        [
                            (transformed.left() as f32),
                            (transformed.top() as f32),
                            (transformed.width() as f32),
                            (transformed.height() as f32),
                        ],
                        start,
                        end,
                        corner_radii,
                        stop_count as u32,
                    )
                    .with_stop_offset(0);
                    let instance = state.apply_active_clip(instance);
                    let _ = shape_segment.linear_gradient_batch.add(instance);
                    shape_segment.record_run(DrawRun::LinearGradient(
                        shape_segment.linear_gradient_batch.len().saturating_sub(1)
                            ..shape_segment.linear_gradient_batch.len(),
                    ));
                    // `SrcOver` inside the isolated segment, not the paint's
                    // mode: `flush_advanced_layer` renders this segment into an
                    // offscreen and applies the advanced mode when compositing
                    // it. Keying the run by the advanced mode would ask
                    // `GradientPipelines` for a fixed-function pipeline that
                    // cannot exist.
                    DrawSegment::push_gradient_run(
                        &mut shape_segment.linear_gradient_runs,
                        state.current_scissor(),
                        BlendMode::SrcOver,
                    );
                }
                Shader::RadialGradient { center, radius, .. } => {
                    use crate::instancing::RadialGradientInstance;
                    let c = glam::Vec2::new(
                        (center.dx - bounds.left()) as f32,
                        (center.dy - bounds.top()) as f32,
                    );
                    let instance = RadialGradientInstance::new(
                        [
                            (transformed.left() as f32),
                            (transformed.top() as f32),
                            (transformed.width() as f32),
                            (transformed.height() as f32),
                        ],
                        c,
                        *radius as f32,
                        corner_radii,
                        stop_count as u32,
                    )
                    .with_stop_offset(0);
                    let instance = state.apply_active_clip(instance);
                    let _ = shape_segment.radial_gradient_batch.add(instance);
                    shape_segment.record_run(DrawRun::RadialGradient(
                        shape_segment.radial_gradient_batch.len().saturating_sub(1)
                            ..shape_segment.radial_gradient_batch.len(),
                    ));
                    // `SrcOver` inside the isolated segment, not the paint's
                    // mode: `flush_advanced_layer` renders this segment into an
                    // offscreen and applies the advanced mode when compositing
                    // it. Keying the run by the advanced mode would ask
                    // `GradientPipelines` for a fixed-function pipeline that
                    // cannot exist.
                    DrawSegment::push_gradient_run(
                        &mut shape_segment.radial_gradient_runs,
                        state.current_scissor(),
                        BlendMode::SrcOver,
                    );
                }
                Shader::SweepGradient {
                    center,
                    start_angle,
                    end_angle,
                    ..
                } => {
                    use crate::instancing::SweepGradientInstance;
                    let c = glam::Vec2::new(
                        (center.dx - bounds.left()) as f32,
                        (center.dy - bounds.top()) as f32,
                    );
                    let instance = SweepGradientInstance::new(
                        [
                            (transformed.left() as f32),
                            (transformed.top() as f32),
                            (transformed.width() as f32),
                            (transformed.height() as f32),
                        ],
                        c,
                        *start_angle as f32,
                        *end_angle as f32,
                        corner_radii,
                        stop_count as u32,
                    )
                    .with_stop_offset(0);
                    let instance = state.apply_active_clip(instance);
                    let _ = shape_segment.sweep_gradient_batch.add(instance);
                    shape_segment.record_run(DrawRun::SweepGradient(
                        shape_segment.sweep_gradient_batch.len().saturating_sub(1)
                            ..shape_segment.sweep_gradient_batch.len(),
                    ));
                    // `SrcOver` inside the isolated segment, not the paint's
                    // mode: `flush_advanced_layer` renders this segment into an
                    // offscreen and applies the advanced mode when compositing
                    // it. Keying the run by the advanced mode would ask
                    // `GradientPipelines` for a fixed-function pipeline that
                    // cannot exist.
                    DrawSegment::push_gradient_run(
                        &mut shape_segment.sweep_gradient_runs,
                        state.current_scissor(),
                        BlendMode::SrcOver,
                    );
                }
                Shader::Solid { .. } | _ => return false,
            }

            // Step 3: wrap and push as AdvancedShape.
            draw_order.push(crate::command_ir::DrawItem::AdvancedShape(
                crate::command_ir::AdvancedShapeOp {
                    segment: shape_segment.seal(),
                    mode: paint.blend_mode,
                    device_bounds: transformed,
                },
            ));

            return true;
        }

        // ── Fixed-function path ───────────────────────────────────────────────
        //
        // Every remaining mode is expressible as a blend state, so the run
        // carries `paint.blend_mode` and `GradientPipelines` keys the pipeline
        // by it. Before that key existed the mode was dropped here and every
        // gradient rendered as `SrcOver`.

        match shader {
            Shader::LinearGradient { from, to, .. } => {
                let start = glam::Vec2::new(
                    (from.dx - bounds.left()) as f32,
                    (from.dy - bounds.top()) as f32,
                );
                let end = glam::Vec2::new(
                    (to.dx - bounds.left()) as f32,
                    (to.dy - bounds.top()) as f32,
                );
                Self::draw_gradient_rect(
                    segment,
                    state,
                    transformed,
                    start,
                    end,
                    &stops,
                    corner_radii,
                    paint.blend_mode,
                );
            }
            Shader::RadialGradient { center, radius, .. } => {
                let c = glam::Vec2::new(
                    (center.dx - bounds.left()) as f32,
                    (center.dy - bounds.top()) as f32,
                );
                Self::draw_radial_gradient_rect(
                    segment,
                    state,
                    transformed,
                    c,
                    *radius as f32,
                    &stops,
                    corner_radii,
                    paint.blend_mode,
                );
            }
            Shader::SweepGradient {
                center,
                start_angle,
                end_angle,
                ..
            } => {
                let c = glam::Vec2::new(
                    (center.dx - bounds.left()) as f32,
                    (center.dy - bounds.top()) as f32,
                );
                Self::draw_sweep_gradient_rect(
                    segment,
                    state,
                    transformed,
                    c,
                    *start_angle as f32,
                    *end_angle as f32,
                    &stops,
                    corner_radii,
                    paint.blend_mode,
                );
            }
            Shader::Solid { .. } | _ => return false,
        }

        true
    }
}
