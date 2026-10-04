//! Gradient and shader-dispatch record methods: gradient_rect, radial_gradient_rect,
//! sweep_gradient_rect, shadow_rect, dispatch_shader_rect.

use flui_foundation::geometry::Rect;
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
pub(crate) const MAX_GRADIENT_STOPS: usize = 256;

// Reduce only the phase in f64. The signed span is independent: reducing
// a complete turn modulo TAU would turn a real gradient into a solid stop.
fn packed_sweep_angles(start: f64, end: f64) -> Result<[f32; 2], crate::error::GeometryError> {
    use crate::error::GeometryError;
    if !start.is_finite() || !end.is_finite() {
        return Err(GeometryError::NonFinite {
            context: "gradient angle",
        });
    }
    let span = end - start;
    let phase = start.rem_euclid(std::f64::consts::TAU);
    let packed = [phase as f32, (phase + span) as f32];
    let packed_span = packed[1] - packed[0];
    if !span.is_finite()
        || !packed[1].is_finite()
        || !packed_span.is_finite()
        || (span != 0.0 && packed_span == 0.0)
    {
        return Err(GeometryError::Unrepresentable {
            context: "gradient angle span",
        });
    }
    Ok(packed)
}

// Compile the projection into a local affine parameter before narrowing.
// GPU subtraction of two distant endpoints loses small visible coordinates;
// t = a*x + b*y + c does not subtract those endpoints per fragment.
fn packed_linear_parameter(
    from: [f64; 2],
    to: [f64; 2],
    bounds: Rect<f64>,
) -> Result<[f32; 4], crate::error::GeometryError> {
    use crate::error::GeometryError;
    let start = [from[0] - bounds.left(), from[1] - bounds.top()];
    let delta = [to[0] - from[0], to[1] - from[1]];
    if start.iter().chain(delta.iter()).any(|v| !v.is_finite()) {
        return Err(GeometryError::NonFinite {
            context: "gradient projection",
        });
    }
    let scale = delta[0].abs().max(delta[1].abs());
    if scale == 0.0 {
        return Ok([0.0; 4]);
    }
    let normalized = [delta[0] / scale, delta[1] / scale];
    let norm = normalized[0] * normalized[0] + normalized[1] * normalized[1];
    // Preserve the existing small-direction solid-stop threshold.
    if scale <= 0.01 && scale * scale * norm <= 0.0001 {
        return Ok([0.0; 4]);
    }
    let a = (normalized[0] / norm) / scale;
    let b = (normalized[1] / norm) / scale;
    let c = -(start[0] * a + start[1] * b);
    let coefficients = [a, b, c];
    if coefficients
        .iter()
        .any(|&v| !v.is_finite() || !(v as f32).is_finite() || (v != 0.0 && v as f32 == 0.0))
    {
        return Err(GeometryError::Unrepresentable {
            context: "gradient projection",
        });
    }
    let packed = [a as f32, b as f32, c as f32, 0.0];
    // Each partial sum in the shader remains finite throughout the rectangle.
    let bound = f64::from(packed[0]).abs() * f64::from(bounds.width() as f32)
        + f64::from(packed[1]).abs() * f64::from(bounds.height() as f32)
        + f64::from(packed[2]).abs();
    if !bound.is_finite() || bound > f64::from(f32::MAX) * 0.5 {
        return Err(GeometryError::Unrepresentable {
            context: "gradient projection arithmetic",
        });
    }
    Ok(packed)
}

/// Both circles share a normalized, bounds-local coordinate system.
#[derive(Clone, Copy)]
struct PackedRadial {
    center: glam::Vec2,
    radius: f32,
    focal: [f32; 4],
    tile: f32,
}

fn packed_radial(
    shader: &Shader,
    bounds: Rect<f64>,
) -> Result<PackedRadial, crate::error::GeometryError> {
    use crate::error::GeometryError;
    use flui_painting::paint::TileMode;
    let Shader::RadialGradient {
        center,
        radius,
        focal,
        focal_radius,
        tile_mode,
        ..
    } = shader
    else {
        return Err(GeometryError::Unrepresentable {
            context: "radial shader kind",
        });
    };
    let focal = focal.unwrap_or(*center);
    let r0 = focal_radius.unwrap_or(0.0);
    let coordinates = [center.dx, center.dy, focal.dx, focal.dy, *radius, r0];
    if coordinates.iter().any(|v| !v.is_finite()) {
        return Err(GeometryError::NonFinite {
            context: "radial circles",
        });
    }
    if *radius < 0.0 || r0 < 0.0 {
        return Err(GeometryError::InvalidRadius);
    }
    if focal == *center && r0 == *radius && r0 != 0.0 {
        return Err(GeometryError::Unrepresentable {
            context: "coincident radial circles",
        });
    }
    let local = [
        center.dx - bounds.left(),
        center.dy - bounds.top(),
        focal.dx - bounds.left(),
        focal.dy - bounds.top(),
    ];
    let scale = local.iter().fold(
        bounds
            .width()
            .abs()
            .max(bounds.height().abs())
            .max(*radius)
            .max(r0),
        |scale, v| scale.max(v.abs()),
    );
    let scale = if scale == 0.0 { 1.0 } else { scale };
    let values = [
        local[0] / scale,
        local[1] / scale,
        *radius / scale,
        local[2] / scale,
        local[3] / scale,
        r0 / scale,
        scale.recip(),
    ];
    if values
        .iter()
        .any(|&v| !v.is_finite() || !(v as f32).is_finite() || (v != 0.0 && v as f32 == 0.0))
    {
        return Err(GeometryError::Unrepresentable {
            context: "radial normalization",
        });
    }
    let packed = values.map(|v| v as f32);
    let normalized_width = bounds.width() as f32 * packed[6];
    let normalized_height = bounds.height() as f32 * packed[6];
    if (bounds.width() != 0.0
        && (normalized_width == 0.0 || normalized_width - packed[3] == -packed[3]))
        || (bounds.height() != 0.0
            && (normalized_height == 0.0 || normalized_height - packed[4] == -packed[4]))
    {
        return Err(GeometryError::Unrepresentable {
            context: "radial fragment coordinates",
        });
    }
    let dx = packed[0] - packed[3];
    let dy = packed[1] - packed[4];
    let dr = packed[2] - packed[5];
    let a = dx * dx + dy * dy - dr * dr;
    let native_dx = values[0] - values[3];
    let native_dy = values[1] - values[4];
    let native_dr = values[2] - values[5];
    let native_a = native_dx * native_dx + native_dy * native_dy - native_dr * native_dr;
    // A collapsed circle difference or quadratic changes which equation is solved.
    if (native_dx != 0.0 && dx == 0.0)
        || (native_dy != 0.0 && dy == 0.0)
        || (native_dr != 0.0 && dr == 0.0)
        || (native_a != 0.0 && a == 0.0)
        || (native_a == 0.0 && a != 0.0)
        || (native_a > 0.0 && a < 0.0)
        || (native_a < 0.0 && a > 0.0)
        || !a.is_finite()
        || !(bounds.width() as f32 * packed[6]).is_finite()
        || !(bounds.height() as f32 * packed[6]).is_finite()
    {
        return Err(GeometryError::Unrepresentable {
            context: "radial equation",
        });
    }
    Ok(PackedRadial {
        center: glam::vec2(packed[0], packed[1]),
        radius: packed[2],
        focal: [packed[3], packed[4], packed[5], packed[6]],
        tile: match tile_mode {
            TileMode::Clamp => 0.0,
            TileMode::Repeat => 1.0,
            TileMode::Mirror => 2.0,
            TileMode::Decal => 3.0,
        },
    })
}

// Validate the payload before stop storage or a destination-read segment is admitted.
// Coordinates are rebased in f64 first: a large logical origin is not itself
// an unrepresentable local gradient.
fn validate_gradient_payload(
    shader: &Shader,
    bounds: Rect<f64>,
) -> Result<(), crate::error::GeometryError> {
    use crate::error::GeometryError;
    let pack = |value: f64| {
        if !value.is_finite() {
            Err(GeometryError::NonFinite {
                context: "gradient payload",
            })
        } else if !(value as f32).is_finite() || (value != 0.0 && value as f32 == 0.0) {
            Err(GeometryError::Unrepresentable {
                context: "gradient payload",
            })
        } else {
            Ok(())
        }
    };
    let point = |horizontal: f64, vertical: f64| {
        pack(horizontal - bounds.left())?;
        pack(vertical - bounds.top())
    };
    pack(bounds.width())?;
    pack(bounds.height())?;
    let (colors, stops) = match shader {
        Shader::LinearGradient {
            from,
            to,
            colors,
            stops,
            ..
        } => {
            packed_linear_parameter([from.dx, from.dy], [to.dx, to.dy], bounds)?;
            (colors, stops)
        }
        Shader::RadialGradient { colors, stops, .. } => {
            packed_radial(shader, bounds)?;
            (colors, stops)
        }
        Shader::SweepGradient {
            center,
            start_angle,
            end_angle,
            colors,
            stops,
            ..
        } => {
            point(center.dx, center.dy)?;
            packed_sweep_angles(*start_angle, *end_angle)?;
            (colors, stops)
        }
        _ => return Ok(()),
    };
    let mut previous = 0.0_f32;
    for index in 0..colors.len() {
        let even = index as f32 / colors.len().saturating_sub(1).max(1) as f32;
        let position = if let Some(value) = stops.as_deref().and_then(|values| values.get(index)) {
            pack(*value)?;
            (*value as f32).clamp(0.0, 1.0)
        } else {
            even
        };
        if position < previous {
            return Err(GeometryError::Unrepresentable {
                context: "gradient stop order",
            });
        }
        previous = position;
    }
    Ok(())
}

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
    /// * `bounds`          — rectangle bounds in local space
    /// * `linear_parameter` - validated local affine parameter `[a, b, c, 0]`
    /// * `stops`           — validated gradient color stops
    /// * `corner_radii`    — per-corner radii `[tl, tr, br, bl]` (0.0 = sharp)
    /// * `blend`           — the paint's fixed-function blend mode (never advanced)
    pub(in super::super) fn draw_gradient_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<f64>,
        linear_parameter: [f32; 4],
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
            linear_parameter,
            corner_radii,
            stop_count as u32,
        )
        .with_stop_offset(stop_offset);
        let instance = state.apply_active_clip(instance.with_transform(
            glam::DMat4::from_cols_array(&state.current_transform_matrix().m),
            [bounds.left(), bounds.top()],
        ));

        let _ = segment.linear_gradient_batch.add(instance);
        segment.record_run(
            DrawRun::LinearGradient(
                segment.linear_gradient_batch.len().saturating_sub(1)
                    ..segment.linear_gradient_batch.len(),
            ),
            state.clip_chain(),
        );
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
    /// * `bounds`         — rectangle bounds in local space
    /// * `radial`         — validated, normalized two-circle equation
    /// * `stops`          — validated gradient color stops
    /// * `corner_radii`   — per-corner radii `[tl, tr, br, bl]` (0.0 = sharp)
    /// * `blend`          — the paint's fixed-function blend mode (never advanced)
    fn draw_radial_gradient_rect(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        bounds: Rect<f64>,
        radial: PackedRadial,
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
            radial.center,
            radial.radius,
            corner_radii,
            stop_count as u32,
        )
        .with_circles(radial.focal, radial.tile)
        .with_stop_offset(stop_offset);
        let instance = state.apply_active_clip(instance.with_transform(
            glam::DMat4::from_cols_array(&state.current_transform_matrix().m),
            [bounds.left(), bounds.top()],
        ));

        let _ = segment.radial_gradient_batch.add(instance);
        segment.record_run(
            DrawRun::RadialGradient(
                segment.radial_gradient_batch.len().saturating_sub(1)
                    ..segment.radial_gradient_batch.len(),
            ),
            state.clip_chain(),
        );
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
    /// * `bounds`       — rectangle bounds in local space
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
        let instance = state.apply_active_clip(instance.with_transform(
            glam::DMat4::from_cols_array(&state.current_transform_matrix().m),
            [bounds.left(), bounds.top()],
        ));

        let _ = segment.sweep_gradient_batch.add(instance);
        segment.record_run(
            DrawRun::SweepGradient(
                segment.sweep_gradient_batch.len().saturating_sub(1)
                    ..segment.sweep_gradient_batch.len(),
            ),
            state.clip_chain(),
        );
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
        state: &GpuStateStack,
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
        segment.record_run(
            DrawRun::Shadow(
                segment.shadow_batch.len().saturating_sub(1)..segment.shadow_batch.len(),
            ),
            state.clip_chain(),
        );
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
        if let Err(error) = validate_gradient_payload(shader, bounds) {
            segment
                .budget
                .record_error(crate::command_ir::RecordError::Geometry(error));
            return true;
        }
        let stops = Self::shader_to_gradient_stops(shader, &segment.budget);
        if stops.is_empty() {
            return false;
        }

        let matrix = glam::DMat4::from_cols_array(&state.current_transform_matrix().m);
        // The geometry validator uses f64; the instance affine is an f32 ABI.
        // A finite root bound does not imply its individual coefficients survive
        // packing (a huge scale can be cancelled by tiny local geometry).
        for coefficient in [
            matrix.x_axis.x,
            matrix.x_axis.y,
            matrix.y_axis.x,
            matrix.y_axis.y,
            matrix.w_axis.x,
            matrix.w_axis.y,
        ] {
            let packed = coefficient as f32;
            if !packed.is_finite() || (coefficient != 0.0 && packed == 0.0) {
                segment
                    .budget
                    .record_error(crate::command_ir::RecordError::Geometry(
                        crate::error::GeometryError::Unrepresentable {
                            context: "gradient affine coefficient",
                        },
                    ));
                return true;
            }
        }
        let transformed = match crate::clip_geometry::ValidatedAffine::new(matrix)
            .and_then(|affine| affine.map_bounds(bounds))
        {
            Ok(Some(bounds)) => bounds,
            Ok(None) => return true,
            Err(error) => {
                segment
                    .budget
                    .record_error(crate::command_ir::RecordError::Geometry(error));
                return true;
            }
        };

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
        let origin = matrix * glam::dvec4(bounds.left(), bounds.top(), 0.0, 1.0);
        for coordinate in [origin.x, origin.y] {
            let packed = coordinate as f32;
            if !packed.is_finite() || (coordinate != 0.0 && packed == 0.0) {
                segment
                    .budget
                    .record_error(crate::command_ir::RecordError::Geometry(
                        crate::error::GeometryError::Unrepresentable {
                            context: "gradient rebased origin",
                        },
                    ));
                return true;
            }
        }

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
                    let parameter =
                        packed_linear_parameter([from.dx, from.dy], [to.dx, to.dy], bounds)
                            .expect("BUG: linear parameter validated before recording");
                    let instance = LinearGradientInstance::new(
                        [
                            (bounds.left() as f32),
                            (bounds.top() as f32),
                            (bounds.width() as f32),
                            (bounds.height() as f32),
                        ],
                        parameter,
                        corner_radii,
                        stop_count as u32,
                    )
                    .with_stop_offset(0);
                    let instance = state.apply_active_clip(instance.with_transform(
                        glam::DMat4::from_cols_array(&state.current_transform_matrix().m),
                        [bounds.left(), bounds.top()],
                    ));
                    let _ = shape_segment.linear_gradient_batch.add(instance);
                    shape_segment.record_run(
                        DrawRun::LinearGradient(
                            shape_segment.linear_gradient_batch.len().saturating_sub(1)
                                ..shape_segment.linear_gradient_batch.len(),
                        ),
                        state.clip_chain(),
                    );
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
                Shader::RadialGradient { .. } => {
                    use crate::instancing::RadialGradientInstance;
                    let radial = packed_radial(shader, bounds)
                        .expect("BUG: radial circles validated before recording");
                    let instance = RadialGradientInstance::new(
                        [
                            (bounds.left() as f32),
                            (bounds.top() as f32),
                            (bounds.width() as f32),
                            (bounds.height() as f32),
                        ],
                        radial.center,
                        radial.radius,
                        corner_radii,
                        stop_count as u32,
                    )
                    .with_circles(radial.focal, radial.tile)
                    .with_stop_offset(0);
                    let instance = state.apply_active_clip(instance.with_transform(
                        glam::DMat4::from_cols_array(&state.current_transform_matrix().m),
                        [bounds.left(), bounds.top()],
                    ));
                    let _ = shape_segment.radial_gradient_batch.add(instance);
                    shape_segment.record_run(
                        DrawRun::RadialGradient(
                            shape_segment.radial_gradient_batch.len().saturating_sub(1)
                                ..shape_segment.radial_gradient_batch.len(),
                        ),
                        state.clip_chain(),
                    );
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
                            (bounds.left() as f32),
                            (bounds.top() as f32),
                            (bounds.width() as f32),
                            (bounds.height() as f32),
                        ],
                        c,
                        packed_sweep_angles(*start_angle, *end_angle)
                            .expect("BUG: sweep angles validated before recording")[0],
                        packed_sweep_angles(*start_angle, *end_angle)
                            .expect("BUG: sweep angles validated before recording")[1],
                        corner_radii,
                        stop_count as u32,
                    )
                    .with_stop_offset(0);
                    let instance = state.apply_active_clip(instance.with_transform(
                        glam::DMat4::from_cols_array(&state.current_transform_matrix().m),
                        [bounds.left(), bounds.top()],
                    ));
                    let _ = shape_segment.sweep_gradient_batch.add(instance);
                    shape_segment.record_run(
                        DrawRun::SweepGradient(
                            shape_segment.sweep_gradient_batch.len().saturating_sub(1)
                                ..shape_segment.sweep_gradient_batch.len(),
                        ),
                        state.clip_chain(),
                    );
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
                let parameter = packed_linear_parameter([from.dx, from.dy], [to.dx, to.dy], bounds)
                    .expect("BUG: linear parameter validated before recording");
                Self::draw_gradient_rect(
                    segment,
                    state,
                    bounds,
                    parameter,
                    &stops,
                    corner_radii,
                    paint.blend_mode,
                );
            }
            Shader::RadialGradient { .. } => {
                let radial = packed_radial(shader, bounds)
                    .expect("BUG: radial circles validated before recording");
                Self::draw_radial_gradient_rect(
                    segment,
                    state,
                    bounds,
                    radial,
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
                    bounds,
                    c,
                    packed_sweep_angles(*start_angle, *end_angle)
                        .expect("BUG: sweep angles validated before recording")[0],
                    packed_sweep_angles(*start_angle, *end_angle)
                        .expect("BUG: sweep angles validated before recording")[1],
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
