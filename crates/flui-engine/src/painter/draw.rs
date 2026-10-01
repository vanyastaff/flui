// ===== Public Drawing API =====
//
// Inherent methods, not a trait: `LayerDispatcher` (the `CommandRenderer`
// impl) and embedders driving the painter directly (`examples/painting_demo`)
// are the callers, and there is no second painter for a trait to abstract.
//
// GPU rendering routinely converts between f32/u8/u32/i32 for pixel
// coordinates, color channels, and buffer indices. These truncations are
// intentional.

use std::sync::Arc;

impl super::WgpuPainter {
    /// Draw a filled or stroked rectangle.
    ///
    /// The rectangle is batched into the current draw segment as a
    /// `RectInstance` and submitted at the next `render` call.  The
    /// current transform and scissor are baked into the instance; no GPU state
    /// switch is needed between adjacent same-mode rect calls.
    ///
    /// `paint.style` determines fill vs stroke; `paint.color` and
    /// `paint.blend_mode` are applied at composite time.
    pub fn draw_rect(
        &mut self,
        rect: flui_foundation::geometry::Rect<f64>,
        paint: &flui_painting::Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!("WgpuPainter::draw_rect: rect={:?}, paint={:?}", rect, paint);

        let opacity = self.compositor.current_opacity();
        self.batcher.draw_rect(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            opacity,
            rect,
            paint,
        );
    }

    /// Draw a filled or stroked rounded rectangle.
    ///
    /// `rrect` carries the axis-aligned bounds and per-corner radii.  The
    /// shape is batched as a `RectInstance` with the corner radii encoded;
    /// the SDF evaluator in `rect_instanced.wgsl` clips to the rounded
    /// boundary in the fragment shader, so no tessellation is needed for
    /// simple rounded rects.
    pub fn draw_rrect(
        &mut self,
        rrect: flui_foundation::geometry::RRect,
        paint: &flui_painting::Paint,
    ) {
        let opacity = self.compositor.current_opacity();
        self.batcher.draw_rrect(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            opacity,
            rrect,
            paint,
        );
    }

    /// Draw a filled or stroked circle.
    ///
    /// The circle is batched as a `CircleInstance` via the SDF pipeline —
    /// no tessellation, sub-pixel accurate at any scale.  `radius` is in
    /// device pixels; the current transform's scale is baked into the instance
    /// by `DrawBatcher::circle` so the analytical SDF always operates in
    /// the correct device-pixel space.
    pub fn draw_circle(
        &mut self,
        center: flui_foundation::geometry::Point<f64>,
        radius: f32,
        paint: &flui_painting::Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_circle: center={:?}, radius={}, paint={:?}",
            center,
            radius,
            paint
        );

        let opacity = self.compositor.current_opacity();
        self.batcher.draw_circle(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            opacity,
            center,
            radius,
            paint,
        );
    }

    /// Draw a filled or stroked oval (axis-aligned ellipse).
    ///
    /// The bounding rectangle `rect` defines the ellipse axes.  The shape is
    /// rendered via the circle-SDF pipeline with a non-uniform transform that
    /// stretches the unit circle to the ellipse aspect ratio — no tessellation
    /// required.
    pub fn draw_oval(
        &mut self,
        rect: flui_foundation::geometry::Rect<f64>,
        paint: &flui_painting::Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!("WgpuPainter::draw_oval: rect={:?}, paint={:?}", rect, paint);

        let opacity = self.compositor.current_opacity();
        self.batcher.draw_oval(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            opacity,
            rect,
            paint,
        );
    }

    /// Draw an arc segment.
    ///
    /// `rect` is the bounding box of the full ellipse; `start_angle` and
    /// `sweep_angle` are in radians (measured clockwise from the positive X
    /// axis in screen space).  When `use_center` is `true` the arc is closed
    /// with two radii back to the center (pie-slice); otherwise only the arc
    /// itself is drawn.  The shape is batched as an `ArcInstance` via the
    /// analytical arc-SDF pipeline.
    pub fn draw_arc(
        &mut self,
        rect: flui_foundation::geometry::Rect<f64>,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &flui_painting::Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_arc: rect={:?}, start={}, sweep={}, use_center={}, paint={:?}",
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint
        );

        let opacity = self.compositor.current_opacity();
        self.batcher.draw_arc(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            opacity,
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint,
        );
    }

    /// Draw a double rounded rectangle (annular ring / bordered shape).
    ///
    /// Renders the area between `outer` and `inner` rounded rectangles.
    /// Typical use: a border or ring where `inner` carves out the fill.
    /// Both shapes must be coaxial (same center); the behaviour is undefined
    /// if `inner` extends beyond `outer`.
    pub fn draw_drrect(
        &mut self,
        outer: flui_foundation::geometry::RRect,
        inner: flui_foundation::geometry::RRect,
        paint: &flui_painting::Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_drrect: outer={:?}, inner={:?}, paint={:?}",
            outer,
            inner,
            paint
        );

        self.batcher.draw_drrect(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            outer,
            inner,
            paint,
        );
    }

    /// Draw a line segment from `p1` to `p2`.
    ///
    /// The line is tessellated into a quad with half-width `paint.stroke_width / 2.0`
    /// (minimum 0.5 px) and submitted via the tessellated-path pipeline.
    /// `paint.color` sets the stroke color; `paint.style` is ignored (lines are
    /// always stroked).
    pub fn draw_line(
        &mut self,
        p1: flui_foundation::geometry::Point<f64>,
        p2: flui_foundation::geometry::Point<f64>,
        paint: &flui_painting::Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_line: p1={:?}, p2={:?}, paint={:?}",
            p1,
            p2,
            paint
        );

        self.batcher.draw_line(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            p1,
            p2,
            paint,
        );
    }

    /// Draws a shaped paragraph with its top-left at `position` (local
    /// pixels); `color` paints every glyph without a span colour of its own.
    ///
    /// Glyphs are placed under the current transform, clip, and opacity and
    /// recorded into the current segment like any other primitive; bitmaps
    /// not yet in the atlas are rasterised now.
    pub fn draw_paragraph(
        &mut self,
        paragraph: Arc<flui_painting::ShapedParagraph>,
        position: flui_foundation::geometry::Point<f64>,
        color: flui_painting::styling::Color,
    ) {
        tracing::trace!(
            lines = paragraph.line_count(),
            ?position,
            ?color,
            "WgpuPainter::draw_paragraph"
        );
        let opacity = self.compositor.current_opacity();
        crate::batches::DrawBatcher::draw_paragraph(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            &mut self.glyph_atlas,
            opacity,
            &paragraph,
            position,
            color,
        );
    }

    /// Draw an arbitrary path.
    ///
    /// The path is tessellated by lyon into a triangle mesh for filled paths or
    /// a stroke quad-mesh for stroked paths.  For `SrcOver` blend mode the mesh
    /// is accumulated in the current `DrawSegment`; for advanced (dst-read)
    /// blend modes the tessellated segment is isolated into a
    /// `DrawItem::SsaaPath` so `flush_advanced_layer` can dst-read the backdrop.
    ///
    /// Tessellation quality is governed by the current CTM scale (see
    /// `current_max_scale`).
    pub fn draw_path(
        &mut self,
        path: &flui_painting::paint::path::Path,
        paint: &flui_painting::Paint,
    ) {
        self.batcher.draw_path(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            path,
            paint,
        );
    }

    /// Draw an image with an explicit blend mode.
    ///
    /// Pass `BlendMode::SrcOver` for the default compositing behaviour (byte-identical
    /// to the path before advanced-blend support).  When `blend_mode.is_advanced()`
    /// the draw is isolated into a
    /// `DrawItem::AdvancedShape` so `flush_advanced_layer` can dst-read the backdrop.
    pub fn draw_image(
        &mut self,
        image: &flui_painting::paint::Image,
        dst_rect: flui_foundation::geometry::Rect<f64>,
        blend_mode: flui_painting::BlendMode,
    ) {
        crate::batches::DrawBatcher::draw_image(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            self.resources.texture_cache_mut(),
            image,
            dst_rect,
            blend_mode,
        );
    }

    /// Draw a tiled image with an explicit blend mode.
    ///
    /// Pass `BlendMode::SrcOver` for the default tiling behaviour.  When
    /// `blend_mode.is_advanced()` ALL tiles are collected into ONE
    /// `DrawItem::AdvancedShape` so every tile reads the original backdrop.
    pub fn draw_image_repeat(
        &mut self,
        image: &flui_painting::paint::Image,
        dst: flui_foundation::geometry::Rect<f64>,
        repeat: flui_painting::paint::image::ImageRepeat,
        blend_mode: flui_painting::BlendMode,
    ) {
        crate::batches::DrawBatcher::draw_image_repeat(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            self.resources.texture_cache_mut(),
            image,
            dst,
            repeat,
            blend_mode,
        );
    }

    /// Draw a nine-slice image with an explicit blend mode.
    ///
    /// Pass `BlendMode::SrcOver` for the default nine-slice behaviour.  When
    /// `blend_mode.is_advanced()` ALL nine regions are collected into ONE
    /// `DrawItem::AdvancedShape`.
    pub fn draw_image_nine_slice(
        &mut self,
        image: &flui_painting::paint::Image,
        center_slice: flui_foundation::geometry::Rect<f64>,
        dst: flui_foundation::geometry::Rect<f64>,
        blend_mode: flui_painting::BlendMode,
    ) {
        crate::batches::DrawBatcher::draw_image_nine_slice(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            self.resources.texture_cache_mut(),
            image,
            center_slice,
            dst,
            blend_mode,
        );
    }

    /// Draw a color-filtered image with an explicit GPU-level blend mode.
    ///
    /// `filter` bakes a per-pixel CPU operation first; `blend_mode` composites the
    /// result against the framebuffer (GPU).  Pass `BlendMode::SrcOver` for the
    /// default behaviour — the two blend modes are independent (see
    /// `DrawBatcher::draw_image_filtered` for the boundary contract).
    pub fn draw_image_filtered(
        &mut self,
        image: &flui_painting::paint::Image,
        dst: flui_foundation::geometry::Rect<f64>,
        filter: flui_painting::paint::image::ColorFilter,
        blend_mode: flui_painting::BlendMode,
    ) {
        crate::batches::DrawBatcher::draw_image_filtered(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            self.resources.texture_cache_mut(),
            image,
            dst,
            filter,
            blend_mode,
        );
    }

    /// Draw a path shadow.
    ///
    /// Renders an analytical box shadow using Evan Wallace's O(1) technique —
    /// quality indistinguishable from a real Gaussian at a single-pass cost.
    /// `elevation` is in logical pixels and controls the blur radius; `color`
    /// sets the shadow tint (typically `Color::rgba(0,0,0,N)` for a Material
    /// elevation shadow).  Only convex path outlines are supported; complex
    /// paths fall back gracefully without crashing.
    pub fn draw_shadow(
        &mut self,
        path: &flui_painting::paint::path::Path,
        color: flui_painting::styling::Color,
        elevation: f32,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_shadow: elevation={}, color={:?}",
            elevation,
            color
        );

        self.batcher.draw_shadow(
            &mut self.current_segment,
            &mut self.draw_order,
            &mut self.state,
            path,
            color,
            elevation,
        );
    }

    /// Draw indexed triangle geometry with per-vertex color + uv.
    ///
    /// # `tex_coords` parameter
    ///
    /// The per-vertex uv extraction IS implemented (the
    /// `tex_coords` slice is consumed at the per-vertex loop, copied into
    /// `Vertex::tex_coord`, and baked into the GPU vertex buffer).  What is
    /// NOT yet wired is the **texture-binding pipeline path**:
    /// `pipeline_key_from_paint(paint)` returns a solid-color pipeline today,
    /// so the uv values reach the vertex shader but the fragment shader has no
    /// texture to sample.  A textured pipeline-key variant is tracked as
    /// follow-up work; until then `tex_coords` callers pre-populate the vertex
    /// stream for forward-compat (the data path is correct, only the pipeline
    /// binding is missing).
    pub fn draw_vertices(
        &mut self,
        vertices: &[flui_foundation::geometry::Point<f64>],
        colors: Option<&[flui_painting::styling::Color]>,
        tex_coords: Option<&[flui_foundation::geometry::Point<f64>]>,
        indices: &[u16],
        paint: &flui_painting::Paint,
    ) {
        crate::batches::DrawBatcher::draw_vertices(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            vertices,
            colors,
            tex_coords,
            indices,
            paint,
        );
    }

    /// Draw a sprite atlas with an explicit blend mode.
    ///
    /// Pass `BlendMode::SrcOver` for the default per-sprite compositing behaviour.
    /// When `blend_mode.is_advanced()` ALL sprites are collected into ONE
    /// `DrawItem::AdvancedShape` so every sprite reads the original backdrop.
    pub fn draw_atlas(
        &mut self,
        image: &flui_painting::paint::Image,
        sprites: &[flui_foundation::geometry::Rect<f64>],
        transforms: &[flui_foundation::geometry::Matrix4],
        colors: Option<&[flui_painting::styling::Color]>,
        blend_mode: flui_painting::BlendMode,
    ) {
        // Convert Matrix4 transforms to pixel-space origins here, at the
        // painter boundary, so the batcher stays Matrix4-free (C4 rule).
        // Each transform is column-major; m[12] = x translation, m[13] = y.
        let sprite_origins: Vec<flui_foundation::geometry::Offset<f64>> = transforms
            .iter()
            .map(|t| flui_foundation::geometry::Offset {
                dx: (t.m[12]),
                dy: (t.m[13]),
            })
            .collect();
        crate::batches::DrawBatcher::draw_atlas(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            self.resources.texture_cache_mut(),
            image,
            sprites,
            &sprite_origins,
            colors,
            blend_mode,
        );
    }

    /// Draw a registered external texture with explicit per-draw sampling.
    ///
    /// `FilterQuality::None` uses nearest; Low, Medium and High currently use
    /// linear filtering (no mipmap or anisotropy promise). Registration metadata
    /// never overrides this choice. Allocation, dimensions and alpha policy are
    /// captured now; later update/unregister affects subsequent recording only.
    /// Writes into that same allocation remain visible (this is not a texel copy).
    /// Invalid ID, nonfinite/invalid rectangles or opacity outside `[0, 1]` latch
    /// a typed frame error returned by rendering. Finish the failed frame before
    /// beginning another. `src` is an optional positive in-bounds texel rectangle.
    pub fn draw_texture(
        &mut self,
        texture_id: flui_painting::paint::TextureId,
        dst: flui_foundation::geometry::Rect<f64>,
        src: Option<flui_foundation::geometry::Rect<f64>>,
        filter_quality: flui_painting::paint::FilterQuality,
        opacity: f32,
    ) {
        let sampling = match filter_quality {
            flui_painting::paint::FilterQuality::None => {
                crate::external_texture_registry::ExternalSampling::Nearest
            }
            _ => crate::external_texture_registry::ExternalSampling::Linear,
        };
        self.record_external_texture(texture_id, dst, src, Some(sampling), opacity);
    }

    /// Draw using the resource's registered sampling policy.
    ///
    /// Alpha, crop, opacity and allocation lifetime follow [`Self::draw_texture`].
    pub fn draw_texture_with_resource_sampling(
        &mut self,
        texture_id: flui_painting::paint::TextureId,
        dst: flui_foundation::geometry::Rect<f64>,
        src: Option<flui_foundation::geometry::Rect<f64>>,
        opacity: f32,
    ) {
        self.record_external_texture(texture_id, dst, src, None, opacity);
    }

    fn record_external_texture(
        &mut self,
        texture_id: flui_painting::paint::TextureId,
        dst: flui_foundation::geometry::Rect<f64>,
        src: Option<flui_foundation::geometry::Rect<f64>>,
        sampling: Option<crate::external_texture_registry::ExternalSampling>,
        opacity: f32,
    ) {
        use crate::error::ExternalTextureError;
        if self.current_segment.recording_result().is_err() {
            return;
        }
        let result = (|| {
            let lease = self
                .resources
                .external_texture_registry()
                .get(texture_id)
                .map(|entry| entry.lease().clone())
                .ok_or(ExternalTextureError::UnknownTexture {
                    id: texture_id.get(),
                })?;
            if !lease.is_for_domain(&self.domain) {
                return Err(ExternalTextureError::ForeignOwner);
            }
            if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
                return Err(ExternalTextureError::InvalidOpacity);
            }
            let valid_rect = |rect: flui_foundation::geometry::Rect<f64>| {
                [rect.left(), rect.top(), rect.right(), rect.bottom()]
                    .into_iter()
                    .all(f64::is_finite)
                    && rect.width().is_finite()
                    && rect.height().is_finite()
                    && rect.width() > 0.0
                    && rect.height() > 0.0
            };
            if !valid_rect(dst) {
                return Err(ExternalTextureError::InvalidDestination);
            }
            if let Some(rect) = src {
                let (width, height) = lease.size();
                if !valid_rect(rect)
                    || rect.left() < 0.0
                    || rect.top() < 0.0
                    || rect.right() > f64::from(width)
                    || rect.bottom() > f64::from(height)
                {
                    return Err(ExternalTextureError::InvalidSourceRect);
                }
            }
            let effective = sampling.unwrap_or(lease.descriptor().sampling);
            Ok((lease, effective))
        })();
        match result {
            Ok((lease, effective)) => crate::batches::DrawBatcher::draw_texture(
                &mut self.current_segment,
                &self.state,
                lease,
                effective,
                dst,
                src,
                opacity,
            ),
            Err(error) => self.current_segment.record_external_error(error),
        }
    }
}
