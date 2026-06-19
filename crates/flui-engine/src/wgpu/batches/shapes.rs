//! Primitive shape record methods: rect, rrect, circle, oval, drrect, arc.

use flui_painting::{BlendMode, Paint, PaintStyle};
use flui_types::{
    Point, Rect,
    geometry::{Pixels, RRect, px},
};

use super::{
    super::{
        command_ir::DrawItem, command_ir::DrawSegment, pipeline, state_stack::GpuStateStack,
        vertex::Vertex,
    },
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
    /// Record a filled rectangle or a stroked rectangle.
    //
    // Visibility note: `pub(in super::super)` makes these methods visible to the
    // `wgpu` module (the grandparent), which is where `painter.rs` lives.  The
    // original `batches.rs` used `pub(super)` because its `super` was `wgpu`
    // directly; after the file→directory split the child files are one level
    // deeper, so `pub(in super::super)` preserves the identical access scope.
    ///
    /// Shader/gradient fills are dispatched first (T9c): when `paint.style` is
    /// `Fill` and `paint.has_shader()`, `dispatch_shader_rect` is called and the
    /// method returns early.  The non-shader fill and stroke paths are unchanged
    /// from T9a.
    pub(in super::super) fn rect(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        opacity: f32,
        rect: Rect<Pixels>,
        paint: &Paint,
    ) {
        // Shader/gradient fill — dispatch before any opacity or color work.
        // Advanced blend modes are handled inside dispatch_shader_rect (PR-5):
        // an isolated DrawSegment is pushed as DrawItem::AdvancedShape so the
        // replay loop can dst-read blend without coupling the gradient path to
        // the tessellator funnel (add_tessellated_with_key).
        if paint.style == PaintStyle::Fill
            && paint.has_shader()
            && Self::dispatch_shader_rect(segment, draw_order, state, rect, paint, [0.0; 4])
        {
            return;
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
            if paint.blend_mode == BlendMode::SrcOver {
                if state.is_axis_aligned() {
                    // Baked-AABB fast path: axis-aligned SrcOver — bake the
                    // transform into the bounds (scale+translate only) and use
                    // the identity affine. Output is byte-identical to the
                    // pre-affine instanced path.
                    let top_left = state.apply_transform(Point::new(rect.left(), rect.top()));
                    let bottom_right =
                        state.apply_transform(Point::new(rect.right(), rect.bottom()));
                    let device_rect =
                        Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);
                    let instance = state.apply_active_clip(
                        super::super::instancing::RectInstance::rect(device_rect, color),
                    );
                    let _ = segment.rect_batch.add(instance);
                    DrawSegment::push_scissor_region(
                        &mut segment.rect_scissors,
                        state.current_scissor(),
                    );
                } else {
                    // Affine instanced path: rotated/skewed SrcOver rect.
                    //
                    // Pass the local-space bounds and the full 2×3 affine to
                    // the GPU. The vertex shader applies `device = M*local + t`;
                    // the fragment SDF evaluates in local space — `fwidth(dist)`
                    // then gives ~1-device-px AA under any affine.
                    let m = state.current_transform();
                    let linear_cols = [m.x_axis.x, m.x_axis.y, m.y_axis.x, m.y_axis.y];
                    let translation = [m.w_axis.x, m.w_axis.y];
                    let local_bounds =
                        [rect.left().0, rect.top().0, rect.width().0, rect.height().0];
                    let instance = state.apply_active_clip(
                        super::super::instancing::RectInstance::with_affine_transform(
                            local_bounds,
                            color,
                            [0.0; 4],
                            linear_cols,
                            translation,
                        ),
                    );
                    let _ = segment.rect_batch.add(instance);
                    // Scissor = the active damage/clip region, exactly as the
                    // axis-aligned path. The shape is bounded by its own quad +
                    // SDF; a per-shape AABB scissor is unnecessary and (because
                    // the quad is expanded by a ~1.5px AA fringe) would clip the
                    // outer fringe at the shape's extreme corners.
                    DrawSegment::push_scissor_region(
                        &mut segment.rect_scissors,
                        state.current_scissor(),
                    );
                }
            } else {
                // Slow path: any non-SrcOver blend mode.
                //
                // PR-4 routing: tile-safe and advanced modes → SSAA tile (AA'd);
                // coverage-destructive modes → tessellated (aliased, correct).
                // See `pipeline::is_tile_safe_for_ssaa` for the mode table.
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
                let mode = paint.blend_mode;
                let scale = state.max_scale();
                let device_area = rect.width().0 * scale * rect.height().0 * scale;
                if (pipeline::is_tile_safe_for_ssaa(mode) || mode.is_advanced())
                    && device_area >= super::paths::SSAA_AREA_THRESHOLD_PX_SQ
                {
                    // Vertices are already in device-pixel space (apply_transform was
                    // called above). Pass them directly to divert_path_to_ssaa.
                    Self::divert_path_to_ssaa(
                        segment, draw_order, state, &vertices, &indices, mode,
                    );
                } else {
                    // Coverage-destructive modes or sub-threshold rects: tessellated path.
                    // Coverage-destructive: Clear/Src/SrcIn/DstIn/SrcOut/DstATop/Modulate
                    // — routing through SSAA tile would zero dst pixels in the tile border.
                    Self::add_tessellated_with_key(
                        segment,
                        draw_order,
                        state,
                        vertices,
                        &indices,
                        pipeline::pipeline_key_from_paint(paint),
                    );
                }
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
    pub(in super::super) fn rrect(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        opacity: f32,
        rrect: RRect,
        paint: &Paint,
    ) {
        // Shader/gradient fill — dispatch before any opacity or color work.
        // Advanced blend modes are handled inside dispatch_shader_rect (PR-5).
        if paint.style == PaintStyle::Fill && paint.has_shader() {
            let corner_radii = [
                rrect.top_left.x.0.max(rrect.top_left.y.0),
                rrect.top_right.x.0.max(rrect.top_right.y.0),
                rrect.bottom_right.x.0.max(rrect.bottom_right.y.0),
                rrect.bottom_left.x.0.max(rrect.bottom_left.y.0),
            ];
            if Self::dispatch_shader_rect(
                segment,
                draw_order,
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
            // A non-SrcOver blend mode must route through the tessellated path.
            if paint.blend_mode != BlendMode::SrcOver {
                // Non-SrcOver rrect (PR-4 routing):
                // - tile-safe or advanced + above area threshold → SSAA tile (AA'd)
                // - coverage-destructive or sub-threshold → tessellated (aliased, correct)
                let mode = paint.blend_mode;
                let fill_paint = Paint::fill(color).with_blend_mode(mode);
                self.prime_tessellator_scale(state);
                match self.tessellator.tessellate_rrect(rrect, &fill_paint) {
                    Ok((vertices, indices)) => {
                        let scale = state.max_scale();
                        let device_area = rrect.bounding_rect().width().0
                            * scale
                            * rrect.bounding_rect().height().0
                            * scale;
                        let ssaa_eligible = (pipeline::is_tile_safe_for_ssaa(mode)
                            || mode.is_advanced())
                            && device_area >= super::paths::SSAA_AREA_THRESHOLD_PX_SQ;
                        if ssaa_eligible {
                            // Bake current transform into vertices before divert.
                            // `divert_path_to_ssaa` expects pre-transformed device-px coords.
                            let transform = state.current_transform();
                            let mut baked = vertices;
                            for v in &mut baked {
                                let p =
                                    transform * glam::vec4(v.position[0], v.position[1], 0.0, 1.0);
                                v.position = [p.x, p.y];
                            }
                            Self::divert_path_to_ssaa(
                                segment, draw_order, state, &baked, &indices, mode,
                            );
                        } else {
                            // Coverage-destructive or sub-threshold: tessellated path.
                            // `submit_transformed_geometry` applies the CTM itself.
                            let key = pipeline::pipeline_key_from_paint(&fill_paint);
                            Self::submit_transformed_geometry(
                                segment, draw_order, state, vertices, &indices, key,
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to tessellate blended rrect: {}", e);
                    }
                }
                return;
            }

            // SrcOver rrect: split on axis-alignment.
            // Per-corner max radius (x and y components of each corner's radii).
            let radius_top_left = rrect.top_left.x.0.max(rrect.top_left.y.0);
            let radius_top_right = rrect.top_right.x.0.max(rrect.top_right.y.0);
            let radius_bottom_right = rrect.bottom_right.x.0.max(rrect.bottom_right.y.0);
            let radius_bottom_left = rrect.bottom_left.x.0.max(rrect.bottom_left.y.0);

            if state.is_axis_aligned() {
                // Baked-AABB fast path: transform the two diagonal corners to get
                // the device-space AABB, then use identity affine.
                // Fixes the pre-existing bug where `apply_transform` of only 2 corners
                // produced a wrong AABB for a rotated rrect (now gated on axis-aligned).
                let top_left =
                    state.apply_transform(Point::new(rrect.rect.left(), rrect.rect.top()));
                let bottom_right =
                    state.apply_transform(Point::new(rrect.rect.right(), rrect.rect.bottom()));
                let device_rect =
                    Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);

                let instance = state.apply_active_clip(
                    super::super::instancing::RectInstance::rounded_rect_corners(
                        device_rect,
                        color,
                        radius_top_left,
                        radius_top_right,
                        radius_bottom_right,
                        radius_bottom_left,
                    ),
                );
                let _ = segment.rect_batch.add(instance);
                DrawSegment::push_scissor_region(
                    &mut segment.rect_scissors,
                    state.current_scissor(),
                );
            } else {
                // Affine instanced path: rotated/skewed SrcOver rrect.
                //
                // Local-space bounds + full 2×3 affine. The SDF evaluates in local
                // space with per-corner radii; fwidth gives correct ~1px AA under
                // rotation/skew. This also fixes the pre-existing bug: previously a
                // rotated SrcOver rrect fell through to the 2-corner AABB bake,
                // rendering as a wrong-size axis-aligned box.
                let m = state.current_transform();
                let linear_cols = [m.x_axis.x, m.x_axis.y, m.y_axis.x, m.y_axis.y];
                let translation = [m.w_axis.x, m.w_axis.y];
                let local_bounds = [
                    rrect.rect.left().0,
                    rrect.rect.top().0,
                    rrect.rect.width().0,
                    rrect.rect.height().0,
                ];
                let instance = state.apply_active_clip(
                    super::super::instancing::RectInstance::with_affine_transform(
                        local_bounds,
                        color,
                        [
                            radius_top_left,
                            radius_top_right,
                            radius_bottom_right,
                            radius_bottom_left,
                        ],
                        linear_cols,
                        translation,
                    ),
                );
                let _ = segment.rect_batch.add(instance);
                // Scissor = the active damage/clip region (same as the axis-aligned
                // path); the shape is bounded by its own quad + SDF, so a per-shape
                // AABB scissor is unnecessary and would clip the AA fringe.
                DrawSegment::push_scissor_region(
                    &mut segment.rect_scissors,
                    state.current_scissor(),
                );
            }
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
    pub(in super::super) fn circle(
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
        // Advanced blend modes are handled inside dispatch_shader_rect (PR-5).
        if paint.style == PaintStyle::Fill && paint.has_shader() {
            let bounds = Rect::from_xywh(
                center.x - px(radius),
                center.y - px(radius),
                px(radius * 2.0),
                px(radius * 2.0),
            );
            if Self::dispatch_shader_rect(segment, draw_order, state, bounds, paint, [radius; 4]) {
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
            // non-SrcOver blend modes route through tessellation (aliased, until PR-4).
            if paint.blend_mode == BlendMode::SrcOver {
                let m = state.current_transform();
                if state.is_axis_aligned() {
                    // Baked fast path: axis-aligned SrcOver — pre-bake the device-space
                    // center and encode the per-axis scale as diag(sx, sy).  Output is
                    // byte-identical to the pre-affine path.
                    let transformed_center = state.apply_transform(center);
                    let sx = (m.x_axis.x * m.x_axis.x + m.x_axis.y * m.x_axis.y).sqrt();
                    let sy = (m.y_axis.x * m.y_axis.x + m.y_axis.y * m.y_axis.y).sqrt();
                    let instance = super::super::instancing::CircleInstance::new(
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
                    // Affine instanced path: rotated/skewed SrcOver circle.
                    //
                    // Encode the circle as a unit sphere (radius=1, center=origin) with
                    // the full affine M_world * r baked into the linear columns so the
                    // vertex shader places the device-space circle correctly.
                    // The fragment evaluates `length(unit_pos) - 1.0`; fwidth gives
                    // ~1-device-px AA at any scale/rotation.
                    let linear_cols = [
                        m.x_axis.x * radius,
                        m.x_axis.y * radius,
                        m.y_axis.x * radius,
                        m.y_axis.y * radius,
                    ];
                    // Translation: M_world * center_local + t_world.
                    // center is already in local space (pre-transform coordinates).
                    let tx = m.x_axis.x * center.x.0 + m.y_axis.x * center.y.0 + m.w_axis.x;
                    let ty = m.x_axis.y * center.x.0 + m.y_axis.y * center.y.0 + m.w_axis.y;
                    let instance = super::super::instancing::CircleInstance::with_affine_transform(
                        linear_cols,
                        color,
                        [tx, ty],
                    );
                    let _ = segment.circle_batch.add(instance);
                    DrawSegment::push_scissor_region(
                        &mut segment.circle_scissors,
                        state.current_scissor(),
                    );
                }
            } else {
                // Slow path: non-SrcOver — tessellate and maybe divert to SSAA (PR-4).
                let mode = paint.blend_mode;
                let fill_paint = Paint {
                    color,
                    style: PaintStyle::Fill,
                    blend_mode: mode,
                    ..Paint::default()
                };
                self.prime_tessellator_scale(state);
                match self
                    .tessellator
                    .tessellate_circle(center, radius, &fill_paint)
                {
                    Ok((vertices, indices)) => {
                        let scale = state.max_scale();
                        let diameter = radius * 2.0 * scale;
                        let device_area = diameter * diameter;
                        let ssaa_eligible = (pipeline::is_tile_safe_for_ssaa(mode)
                            || mode.is_advanced())
                            && device_area >= super::paths::SSAA_AREA_THRESHOLD_PX_SQ;
                        if ssaa_eligible {
                            let transform = state.current_transform();
                            let mut baked = vertices;
                            for v in &mut baked {
                                let p =
                                    transform * glam::vec4(v.position[0], v.position[1], 0.0, 1.0);
                                v.position = [p.x, p.y];
                            }
                            Self::divert_path_to_ssaa(
                                segment, draw_order, state, &baked, &indices, mode,
                            );
                        } else {
                            let key = pipeline::pipeline_key_from_paint(&fill_paint);
                            Self::submit_transformed_geometry(
                                segment, draw_order, state, vertices, &indices, key,
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to tessellate circle: {}", e);
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
    ///
    /// SrcOver filled ovals are routed through the affine instanced circle path:
    /// an ellipse with semi-axes `(rx, ry)` is a unit circle scaled by `diag(rx, ry)`
    /// in local space, so the combined affine is `M_world * diag(rx, ry)` — exactly
    /// what `CircleInstance::with_affine_transform` encodes. The fragment evaluates
    /// `length(unit_pos) - 1.0` on the resulting oriented ellipse; `fwidth` gives
    /// ~1-device-px AA for circles and moderate-aspect ellipses. (At extreme aspect
    /// ratios the unit-circle SDF is not the true Euclidean distance to the ellipse,
    /// so the AA band at the thin tips is slightly wider than 1px — same limitation
    /// as `rect_instanced`; acceptable for UI shapes.)
    ///
    /// Non-SrcOver and stroked ovals remain tessellated (aliased) until PR-4.
    ///
    /// Paint-order note: like all instanced shapes (rect, circle), an instanced
    /// oval flushes in the engine's fixed bucket order, NOT strict painter order
    /// relative to overlapping *tessellated* SrcOver geometry in the same segment.
    /// This is a pre-existing engine characteristic (bucket order ≠ draw order),
    /// now extended consistently to ovals/rotated circles for the AA win; true
    /// painter-order compositing is a separate, engine-wide concern.
    pub(in super::super) fn oval(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        opacity: f32,
        rect: Rect<Pixels>,
        paint: &Paint,
    ) {
        let center = rect.center();
        let rx = (rect.width() / 2.0).0;
        let ry = (rect.height() / 2.0).0;

        // Fold the compositor layer opacity into the color (the instanced pipeline
        // has no opacity uniform), mirroring `rect`/`rrect`/`circle`.
        let color = if opacity < 1.0 {
            let alpha = (f32::from(paint.color.a) * opacity) as u8;
            flui_types::Color::rgba(paint.color.r, paint.color.g, paint.color.b, alpha)
        } else {
            paint.color
        };

        if paint.style == PaintStyle::Fill && paint.blend_mode == BlendMode::SrcOver {
            // Instanced affine SDF path: encode the ellipse as a unit circle under
            // M_world * diag(rx, ry).  Works for both axis-aligned and rotated ovals.
            let m = state.current_transform();

            // Combined linear: M_w * diag(rx, ry).
            // x-col of M_w scaled by rx; y-col of M_w scaled by ry.
            let linear_cols = [
                m.x_axis.x * rx,
                m.x_axis.y * rx,
                m.y_axis.x * ry,
                m.y_axis.y * ry,
            ];
            // Translation: M_w * center_local + t_w.
            let cx = center.x.0;
            let cy = center.y.0;
            let tx = m.x_axis.x * cx + m.y_axis.x * cy + m.w_axis.x;
            let ty = m.x_axis.y * cx + m.y_axis.y * cy + m.w_axis.y;

            let instance = super::super::instancing::CircleInstance::with_affine_transform(
                linear_cols,
                color,
                [tx, ty],
            );
            let _ = segment.circle_batch.add(instance);
            DrawSegment::push_scissor_region(&mut segment.circle_scissors, state.current_scissor());
        } else if paint.style == PaintStyle::Fill {
            // Non-SrcOver fill — PR-4: tile-safe and advanced modes → SSAA (AA'd);
            // coverage-destructive modes → tessellated (aliased, correct).
            let mode = paint.blend_mode;
            let fill_paint = Paint::fill(color).with_blend_mode(mode);
            let radii = Point::new(rect.width() / 2.0, rect.height() / 2.0);
            self.prime_tessellator_scale(state);
            if let Ok((vertices, indices)) =
                self.tessellator
                    .tessellate_ellipse(center, radii, &fill_paint)
            {
                let scale = state.max_scale();
                let device_area = rect.width().0 * scale * rect.height().0 * scale;
                let ssaa_eligible = (pipeline::is_tile_safe_for_ssaa(mode) || mode.is_advanced())
                    && device_area >= super::paths::SSAA_AREA_THRESHOLD_PX_SQ;
                if ssaa_eligible {
                    let transform = state.current_transform();
                    let mut baked = vertices;
                    for v in &mut baked {
                        let p = transform * glam::vec4(v.position[0], v.position[1], 0.0, 1.0);
                        v.position = [p.x, p.y];
                    }
                    Self::divert_path_to_ssaa(segment, draw_order, state, &baked, &indices, mode);
                } else {
                    Self::submit_transformed_geometry(
                        segment,
                        draw_order,
                        state,
                        vertices,
                        &indices,
                        pipeline::pipeline_key_from_paint(&fill_paint),
                    );
                }
            }
        } else {
            // Stroked oval — tessellate (fallback), unchanged.
            let radii = Point::new(rect.width() / 2.0, rect.height() / 2.0);
            self.prime_tessellator_scale(state);
            if let Ok((vertices, indices)) =
                self.tessellator.tessellate_ellipse(center, radii, paint)
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

    /// Record a double rounded rectangle (ring with inner cutout).
    pub(in super::super) fn draw_drrect(
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
                // drrect is a compound (outer minus inner) tessellated fill with no
                // closed-form SDF, so — like arbitrary paths (PR-3) — SrcOver +
                // tile-safe + advanced FILLS route to the SSAA tile for AA;
                // coverage-destructive modes and strokes keep the coverage-correct
                // tessellated (aliased) path. (Without this, a SrcOver ring/border —
                // a common UI element — would render aliased.)
                let mode = paint.blend_mode;
                let scale = state.max_scale();
                let device_area = outer.width().0 * scale * outer.height().0 * scale;
                if paint.style == PaintStyle::Fill
                    && (mode == BlendMode::SrcOver
                        || pipeline::is_tile_safe_for_ssaa(mode)
                        || mode.is_advanced())
                    && device_area >= super::paths::SSAA_AREA_THRESHOLD_PX_SQ
                {
                    Self::submit_transformed_and_divert_to_ssaa(
                        segment, draw_order, state, vertices, &indices, mode,
                    );
                } else {
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
            Err(e) => {
                tracing::error!("Failed to tessellate DRRect: {}", e);
            }
        }
    }

    /// Record a filled or stroked arc (pie-slice or arc segment).
    ///
    /// # Instanced path (PR-2b: full-affine reroute)
    ///
    /// All SrcOver filled arcs — including rotated and skewed ones — are now
    /// routed through the affine instanced SDF path, which uses:
    ///   - fwidth-based radial AA (radius-independent ~1 device-px)
    ///   - screen-space angular AA via half-plane SDFs (resolution-independent)
    ///
    /// Reflection guard: `det < 0` means the transform contains a reflection,
    /// which negates the sector direction in the shader (the angular half-planes
    /// flip). Reflected arcs fall back to tessellation (PR-4 handles non-SrcOver).
    ///
    /// Non-SrcOver blend modes and stroked arcs remain tessellated (aliased)
    /// until the SSAA tile path (PR-4).
    ///
    /// # Opacity
    ///
    /// Compositor layer opacity is folded into the color alpha before submission
    /// (the instanced pipeline has no opacity uniform), mirroring `oval`/`circle`.
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/draw_order/state are disjoint WgpuPainter fields \
                  passed as separate borrows; arc geometry parameters are all necessary"
    )]
    pub(in super::super) fn draw_arc(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        opacity: f32,
        rect: Rect<Pixels>,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) {
        let center = rect.center();
        let rx = (rect.width() / 2.0).0;
        let ry = (rect.height() / 2.0).0;

        if paint.style == PaintStyle::Fill {
            // Fold compositor layer opacity into the color alpha (the instanced
            // pipeline has no opacity uniform), mirroring `rect`/`circle`/`oval`.
            let color = if opacity < 1.0 {
                let alpha = (f32::from(paint.color.a) * opacity) as u8;
                flui_types::Color::rgba(paint.color.r, paint.color.g, paint.color.b, alpha)
            } else {
                paint.color
            };

            // Instanced affine SDF path conditions (else → tessellate, correct
            // shape but aliased until PR-4):
            // - SrcOver only — the instanced pipeline is hardcoded SrcOver; other
            //   blend modes route through the tessellated Porter-Duff path.
            // - `use_center` only — the SDF shader carves a PIE SECTOR through the
            //   origin. A `use_center == false` arc is a circular SEGMENT (endpoints
            //   joined by a straight chord, NOT through the center): a different
            //   shape the shader does not model, so it must tessellate.
            // - Non-reflected (`det >= 0`) — the angular sector is carved in local
            //   space and mapped by M; the math handles reflection, but the
            //   reflected path is untested, so we conservatively tessellate it.
            let m = state.current_transform();
            let det = m.x_axis.x * m.y_axis.y - m.x_axis.y * m.y_axis.x;
            if paint.blend_mode == BlendMode::SrcOver && use_center && det >= 0.0 {
                // Encode the arc as a unit circle under M_world * diag(rx, ry)
                // (elliptical when rx != ry, mirroring `oval`). The center goes
                // into transform_translate (never scaled by M). Angles parameterise
                // the unit circle, so they map to Flutter's elliptical-arc angles.
                let linear_cols = [
                    m.x_axis.x * rx,
                    m.x_axis.y * rx,
                    m.y_axis.x * ry,
                    m.y_axis.y * ry,
                ];
                // Translation: M_world * center_local + t_world.
                let cx = center.x.0;
                let cy = center.y.0;
                let tx = m.x_axis.x * cx + m.y_axis.x * cy + m.w_axis.x;
                let ty = m.x_axis.y * cx + m.y_axis.y * cy + m.w_axis.y;
                let instance = super::super::instancing::ArcInstance::with_affine_transform(
                    linear_cols,
                    start_angle,
                    sweep_angle,
                    color,
                    [tx, ty],
                );
                let _ = segment.arc_batch.add(instance);
                DrawSegment::push_scissor_region(
                    &mut segment.arc_scissors,
                    state.current_scissor(),
                );
            } else {
                // Slow path: non-pie (use_center=false segment), reflection, or
                // non-SrcOver blend — tessellate in local space and bake the full
                // transform into vertex positions.
                //
                // PR-4: tile-safe and advanced modes → SSAA (AA'd edges).
                // Coverage-destructive modes + use_center=false + reflections remain
                // tessellated (aliased).
                let mode = paint.blend_mode;
                let fill_paint = Paint {
                    color,
                    style: PaintStyle::Fill,
                    blend_mode: mode,
                    ..Paint::default()
                };
                self.prime_tessellator_scale(state);
                match self.tessellator.tessellate_arc(
                    rect,
                    start_angle,
                    sweep_angle,
                    use_center,
                    &fill_paint,
                ) {
                    Ok((vertices, indices)) => {
                        let scale = state.max_scale();
                        // Arc bounding area ≈ rect area (conservative upper bound).
                        let device_area = rect.width().0 * scale * rect.height().0 * scale;
                        // Only route to SSAA for non-SrcOver modes; SrcOver arcs
                        // that reach this branch (reflection fallback) are already
                        // tessellated — a second SSAA path for them is not needed.
                        let ssaa_eligible = mode != BlendMode::SrcOver
                            && (pipeline::is_tile_safe_for_ssaa(mode) || mode.is_advanced())
                            && device_area >= super::paths::SSAA_AREA_THRESHOLD_PX_SQ;
                        if ssaa_eligible {
                            let transform = state.current_transform();
                            let mut baked = vertices;
                            for v in &mut baked {
                                let p =
                                    transform * glam::vec4(v.position[0], v.position[1], 0.0, 1.0);
                                v.position = [p.x, p.y];
                            }
                            Self::divert_path_to_ssaa(
                                segment, draw_order, state, &baked, &indices, mode,
                            );
                        } else {
                            let key = pipeline::pipeline_key_from_paint(&fill_paint);
                            Self::submit_transformed_geometry(
                                segment, draw_order, state, vertices, &indices, key,
                            );
                        }
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
}
