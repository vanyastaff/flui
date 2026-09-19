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
        rect: flui_types::Rect<flui_types::geometry::Pixels>,
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
    pub fn draw_rrect(&mut self, rrect: flui_types::geometry::RRect, paint: &flui_painting::Paint) {
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
        center: flui_types::Point<flui_types::geometry::Pixels>,
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
        rect: flui_types::Rect<flui_types::geometry::Pixels>,
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
        rect: flui_types::Rect<flui_types::geometry::Pixels>,
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
        outer: flui_types::geometry::RRect,
        inner: flui_types::geometry::RRect,
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
        p1: flui_types::Point<flui_types::geometry::Pixels>,
        p2: flui_types::Point<flui_types::geometry::Pixels>,
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

    /// Draws a plain-text string at `position` (device pixels) in a single
    /// style: shapes it through the shared font system and records the
    /// paragraph. For anything richer, shape a
    /// [`flui_painting::TextLayout`] yourself and call
    /// [`Self::draw_paragraph`].
    pub fn draw_text(
        &mut self,
        text: &str,
        position: flui_types::Point<flui_types::geometry::Pixels>,
        font_size: f32,
        paint: &flui_painting::Paint,
    ) {
        let layout = flui_painting::TextLayout::new(
            text,
            None,
            font_size,
            None,
            None,
            flui_types::typography::TextDirection::Ltr,
        );
        self.draw_paragraph(Arc::new(layout), position, paint.color);
    }

    /// Draws a shaped paragraph with its top-left at `position` (local
    /// pixels); `color` paints every glyph without a span colour of its own.
    ///
    /// Glyphs are placed under the current transform, clip, and opacity and
    /// recorded into the current segment like any other primitive; bitmaps
    /// not yet in the atlas are rasterised now.
    pub fn draw_paragraph(
        &mut self,
        layout: Arc<flui_painting::TextLayout>,
        position: flui_types::Point<flui_types::geometry::Pixels>,
        color: flui_types::styling::Color,
    ) {
        tracing::trace!(
            lines = layout.metrics().line_count,
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
            &layout,
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
        path: &flui_types::painting::path::Path,
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
        image: &flui_types::painting::Image,
        dst_rect: flui_types::Rect<flui_types::geometry::Pixels>,
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
        image: &flui_types::painting::Image,
        dst: flui_types::Rect<flui_types::geometry::Pixels>,
        repeat: flui_types::painting::image::ImageRepeat,
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
        image: &flui_types::painting::Image,
        center_slice: flui_types::Rect<flui_types::geometry::Pixels>,
        dst: flui_types::Rect<flui_types::geometry::Pixels>,
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
        image: &flui_types::painting::Image,
        dst: flui_types::Rect<flui_types::geometry::Pixels>,
        filter: flui_types::painting::image::ColorFilter,
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
        path: &flui_types::painting::path::Path,
        color: flui_types::styling::Color,
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
        vertices: &[flui_types::Point<flui_types::geometry::Pixels>],
        colors: Option<&[flui_types::styling::Color]>,
        tex_coords: Option<&[flui_types::Point<flui_types::geometry::Pixels>]>,
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
        image: &flui_types::painting::Image,
        sprites: &[flui_types::Rect<flui_types::geometry::Pixels>],
        transforms: &[flui_types::Matrix4],
        colors: Option<&[flui_types::styling::Color]>,
        blend_mode: flui_painting::BlendMode,
    ) {
        // Convert Matrix4 transforms to pixel-space origins here, at the
        // painter boundary, so the batcher stays Matrix4-free (C4 rule).
        // Each transform is column-major; m[12] = x translation, m[13] = y.
        let sprite_origins: Vec<flui_types::Offset<flui_types::geometry::Pixels>> = transforms
            .iter()
            .map(|t| flui_types::Offset {
                dx: flui_types::geometry::px(t.m[12]),
                dy: flui_types::geometry::px(t.m[13]),
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

    /// Draw a registered external texture, optionally cropped to a source sub-rect.
    ///
    /// `texture_id` must have been registered via
    /// [`Self::external_texture_registry_mut`] before this call.
    ///
    /// `dst` is the destination rect in device pixels.  `src`, when `Some`,
    /// selects a sub-rectangle of the texture in texel coordinates; the batcher
    /// normalises these to UV space `[0,1]` using the registered dimensions.
    /// When `src` is `None` the full texture is used (`UV [0,1]×[0,1]`).
    ///
    /// `filter_quality` controls the GPU sampler (Linear vs Nearest).
    /// `opacity` is pre-multiplied into the instance alpha before submission.
    pub fn draw_texture(
        &mut self,
        texture_id: flui_types::painting::TextureId,
        dst: flui_types::Rect<flui_types::geometry::Pixels>,
        src: Option<flui_types::Rect<flui_types::geometry::Pixels>>,
        filter_quality: flui_types::painting::FilterQuality,
        opacity: f32,
    ) {
        // Read dimensions only when a `src` sub-rect was supplied, so the
        // batcher can normalize pixel coordinates to UV in [0,1].  The
        // TextureView stays in the registry until replay time.
        let src_dimensions = src.and_then(|_| {
            self.resources
                .external_texture_registry()
                .get(texture_id)
                .map(|entry| (entry.width, entry.height))
        });
        crate::batches::DrawBatcher::draw_texture(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            src_dimensions,
            texture_id,
            dst,
            src,
            filter_quality,
            opacity,
        );
    }
}
