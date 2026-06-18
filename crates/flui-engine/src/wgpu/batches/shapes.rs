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
                let instance = state.apply_active_clip(
                    super::super::instancing::RectInstance::rect(transformed_rect, color),
                );
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
            let instance = state.apply_active_clip(
                super::super::instancing::RectInstance::rounded_rect_corners(
                    transformed_rect,
                    color,
                    rrect.top_left.x.0.max(rrect.top_left.y.0),
                    rrect.top_right.x.0.max(rrect.top_right.y.0),
                    rrect.bottom_right.x.0.max(rrect.bottom_right.y.0),
                    rrect.bottom_left.x.0.max(rrect.bottom_left.y.0),
                ),
            );
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
            // a non-SrcOver blend mode must route through the tessellated path.
            if state.is_axis_aligned() && paint.blend_mode == BlendMode::SrcOver {
                // Fast path: axis-aligned — use GPU instancing.
                let transformed_center = state.apply_transform(center);
                let m = state.current_transform();
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
    pub(in super::super) fn oval(
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
    pub(in super::super) fn draw_arc(
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

                let instance = super::super::instancing::ArcInstance::new(
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
}
