// ===== Public Drawing API =====
//
// These methods used to be the `impl Painter for WgpuPainter` trait impl;
// the `Painter` trait was deleted (1 production impl, 6 default
// `tracing::warn!("not implemented")` impls, no second backend planned).
// The methods stay as inherent on `WgpuPainter` for direct use by `Backend`
// (the CommandRenderer impl) and external callers like `examples/painting_demo`.
//
// Moved from `painter.rs` into `painter/draw.rs` as part of the C1 LOC-cap
// refactor.  Zero behaviour changes.

// GPU rendering routinely converts between f32/u8/u32/i32 for pixel
// coordinates, color channels, and buffer indices. These truncations are
// intentional.
//
// These methods were originally `impl Painter for WgpuPainter` trait methods;
// the `Painter` trait was deleted in commit 1b376beb. This doc-sweep
// (engine-painter-doc-sweep) adds per-method docs directly on the inherent impl.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
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
    pub fn rect(
        &mut self,
        rect: flui_types::Rect<flui_types::geometry::Pixels>,
        paint: &flui_painting::Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!("WgpuPainter::rect: rect={:?}, paint={:?}", rect, paint);

        let opacity = self.compositor.current_opacity();
        self.batcher.rect(
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
    pub fn rrect(&mut self, rrect: flui_types::geometry::RRect, paint: &flui_painting::Paint) {
        let opacity = self.compositor.current_opacity();
        self.batcher.rrect(
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
    pub fn circle(
        &mut self,
        center: flui_types::Point<flui_types::geometry::Pixels>,
        radius: f32,
        paint: &flui_painting::Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::circle: center={:?}, radius={}, paint={:?}",
            center,
            radius,
            paint
        );

        let opacity = self.compositor.current_opacity();
        self.batcher.circle(
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
    pub fn oval(
        &mut self,
        rect: flui_types::Rect<flui_types::geometry::Pixels>,
        paint: &flui_painting::Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!("WgpuPainter::oval: rect={:?}, paint={:?}", rect, paint);

        let opacity = self.compositor.current_opacity();
        self.batcher.oval(
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
    pub fn line(
        &mut self,
        p1: flui_types::Point<flui_types::geometry::Pixels>,
        p2: flui_types::Point<flui_types::geometry::Pixels>,
        paint: &flui_painting::Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::line: p1={:?}, p2={:?}, paint={:?}",
            p1,
            p2,
            paint
        );

        self.batcher.line(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            p1,
            p2,
            paint,
        );
    }

    /// Draw a plain-text string at `position` in device pixels.
    ///
    /// `font_size` is in device pixels.  The text is submitted to
    /// `TextRenderer` (glyphon) as a single-style run; shaping and atlas
    /// upload happen during the next `render` call.  For styled spans
    /// with per-run fonts, weights, or colors use [`Self::rich_text`] instead.
    ///
    /// The current transform is applied to `position` before submission so that
    /// glyphs land at the correct device-pixel coordinate even inside a
    /// `save`/`restore` transform block.
    pub fn text(
        &mut self,
        text: &str,
        position: flui_types::Point<flui_types::geometry::Pixels>,
        font_size: f32,
        paint: &flui_painting::Paint,
    ) {
        tracing::trace!(
            text,
            ?position,
            font_size,
            color = ?paint.color,
            "WgpuPainter::text"
        );
        let transformed_position = self.state.apply_transform(position);
        self.text_renderer
            .add_text(text, transformed_position, font_size, paint.color);
    }

    /// Renders a sequence of styled runs as rich text.
    ///
    /// `runs` is the flattened output of `collect_styled_spans`: each entry is
    /// `(text_fragment, merged_style)` with `text_scale_factor` already baked
    /// into `style.font_size`.  `base_font_size` is the buffer-level default
    /// for runs with no explicit size; `base_color` is the fallback for runs
    /// with no color.
    pub fn rich_text(
        &mut self,
        runs: &[(String, Option<flui_types::typography::TextStyle>)],
        position: flui_types::Point<flui_types::geometry::Pixels>,
        base_font_size: f32,
        base_color: flui_types::styling::Color,
        wrap_width: Option<f32>,
    ) {
        tracing::trace!(
            run_count = runs.len(),
            ?position,
            base_font_size,
            ?base_color,
            ?wrap_width,
            "WgpuPainter::rich_text"
        );
        let transformed_position = self.state.apply_transform(position);
        self.text_renderer.add_rich_text(
            runs,
            transformed_position,
            base_font_size,
            base_color,
            wrap_width,
        );
    }

    /// Draw a registered external texture into `dst_rect`.
    ///
    /// `texture_id` must have been registered via
    /// [`Self::external_texture_registry_mut`] before this call.  The full
    /// texture is composited at `dst_rect` (UV `[0,1]×[0,1]`); for a sub-rect
    /// source use [`Self::draw_texture`], which accepts an optional `src` rect.
    ///
    /// The current transform is baked into the instance; no sub-rect UV
    /// remapping is performed by this variant.
    pub fn texture(
        &mut self,
        texture_id: flui_types::painting::TextureId,
        dst_rect: flui_types::Rect<flui_types::geometry::Pixels>,
    ) {
        super::super::batches::DrawBatcher::texture(
            &mut self.current_segment,
            &self.state,
            texture_id,
            dst_rect,
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
    /// to pre-PR-5).  When `blend_mode.is_advanced()` the draw is isolated into a
    /// `DrawItem::AdvancedShape` so `flush_advanced_layer` can dst-read the backdrop.
    pub fn draw_image(
        &mut self,
        image: &flui_types::painting::Image,
        dst_rect: flui_types::Rect<flui_types::geometry::Pixels>,
        blend_mode: flui_painting::BlendMode,
    ) {
        super::super::batches::DrawBatcher::draw_image(
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
        repeat: flui_painting::display_list::ImageRepeat,
        blend_mode: flui_painting::BlendMode,
    ) {
        super::super::batches::DrawBatcher::draw_image_repeat(
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
        super::super::batches::DrawBatcher::draw_image_nine_slice(
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
        filter: flui_painting::display_list::ColorFilter,
        blend_mode: flui_painting::BlendMode,
    ) {
        super::super::batches::DrawBatcher::draw_image_filtered(
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
    /// Cycle 4 E-12: the per-vertex uv extraction IS implemented (the
    /// `tex_coords` slice is consumed at the per-vertex loop, copied into
    /// `Vertex::tex_coord`, and baked into the GPU vertex buffer).  What is
    /// NOT yet wired is the **texture-binding pipeline path**:
    /// `pipeline_key_from_paint(paint)` returns a solid-color pipeline today,
    /// so the uv values reach the vertex shader but the fragment shader has no
    /// texture to sample.  A textured pipeline-key variant is a follow-up
    /// audit item; until then `tex_coords` callers pre-populate the vertex
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
        super::super::batches::DrawBatcher::draw_vertices(
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
        super::super::batches::DrawBatcher::draw_atlas(
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
        super::super::batches::DrawBatcher::draw_texture(
            &mut self.current_segment,
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
