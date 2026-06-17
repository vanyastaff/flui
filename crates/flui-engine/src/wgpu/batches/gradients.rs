//! Gradient and shader-dispatch record methods: gradient_rect, radial_gradient_rect,
//! sweep_gradient_rect, shadow_rect, dispatch_shader_rect.

use flui_painting::Paint;
use flui_types::painting::Shader;
use flui_types::{Point, Rect, geometry::Pixels};

use super::{
    super::{command_ir::DrawSegment, effects, effects_pipeline, state_stack::GpuStateStack},
    DrawBatcher,
};

// GPU rendering routinely converts between f32/u8/u32 for pixel coordinates,
// color channels, and buffer indices. These truncations are intentional.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl DrawBatcher {
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
    pub(in super::super) fn gradient_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<Pixels>,
        gradient_start: glam::Vec2,
        gradient_end: glam::Vec2,
        stops: &[effects::GradientStop],
        corner_radius: f32,
    ) {
        use super::super::instancing::LinearGradientInstance;

        // Append gradient stops to global buffer (max 8 per gradient).
        let stop_count = stops.len().min(8);
        let current_len = segment.current_gradient_stops.len();
        if current_len + stop_count > effects_pipeline::MAX_GRADIENT_STOPS {
            // Logged once per process: a >MAX_GRADIENT_STOPS frame would otherwise
            // spam this for every overflowing instance, every frame.
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!(
                    current_stops = current_len,
                    requested = stop_count,
                    limit = effects_pipeline::MAX_GRADIENT_STOPS,
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
    pub(in super::super) fn radial_gradient_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<Pixels>,
        center: glam::Vec2,
        radius: f32,
        stops: &[effects::GradientStop],
        corner_radius: f32,
    ) {
        use super::super::instancing::RadialGradientInstance;

        let stop_count = stops.len().min(8);
        let current_len = segment.current_gradient_stops.len();
        if current_len + stop_count > effects_pipeline::MAX_GRADIENT_STOPS {
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!(
                    current_stops = current_len,
                    requested = stop_count,
                    limit = effects_pipeline::MAX_GRADIENT_STOPS,
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
    pub(in super::super) fn sweep_gradient_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<Pixels>,
        center: glam::Vec2,
        start_angle: f32,
        end_angle: f32,
        stops: &[effects::GradientStop],
        corner_radius: f32,
    ) {
        use super::super::instancing::SweepGradientInstance;

        let stop_count = stops.len().min(8);
        let current_len = segment.current_gradient_stops.len();
        if current_len + stop_count > effects_pipeline::MAX_GRADIENT_STOPS {
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!(
                    current_stops = current_len,
                    requested = stop_count,
                    limit = effects_pipeline::MAX_GRADIENT_STOPS,
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
    pub(in super::super) fn shadow_rect(
        segment: &mut DrawSegment,
        rect_pos: [f32; 2],
        rect_size: [f32; 2],
        corner_radius: f32,
        params: &effects::ShadowParams,
    ) {
        use super::super::instancing::ShadowInstance;

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
    pub(in super::super) fn dispatch_shader_rect(
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
}
