//! Canvas drawing primitives: 29 `draw_*` methods, each emitting one
//! `DrawCommand` variant.
//!
//! Mythos chain U4 extracted these from the 3,305-LOC `canvas.rs` god
//! module. Every method here pushes one `DrawCommand` onto the inner
//! `DisplayList` with the current transform baked in.
//!
//! For batch operations (`draw_rects`, `draw_circles`), conditional
//! draws (`draw_if`, `draw_rect_if`), grid/repeat patterns, debug
//! visualization, and convenience shapes (pill, ring), see
//! [`super::sugar`]. For closure-based scoped operations (`with_*`),
//! see [`super::scoped`]. For multi-canvas composition, see
//! [`super::composition`]. For the fluent chaining API, see
//! [`super::sugar`].
//!
//! # Allocation hot path (Mythos chain U9 audit)
//!
//! Every `draw_*` call clones `Paint` (~80-200 bytes incl. optional
//! `Box<Shader>` payload). `draw_path`/`draw_shadow` additionally
//! clones the `Path` (`Vec<PathCommand>` heap allocation). `clip_path`
//! additionally `Box::new`-wraps the cloned path for `ClipShape`
//! variant uniformity (see [`super::clipping`]).
//!
//! Paint interning + flat-bytecode + Path-Cow are tracked in
//! `crates/flui-painting/ARCHITECTURE.md` `## Outstanding refactors`
//! and require measured benefit before adoption.

use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, Rect, Size},
    painting::{Image, Path},
    styling::Color,
    typography::{InlineSpan, TextStyle},
};

use super::Canvas;
use crate::display_list::{
    BlendMode, ColorFilter, DisplayList, DrawCommand, FilterQuality, ImageFilter, ImageRepeat,
    Paint, PointMode, Shader, TextureId,
};

impl Canvas {
    // ===== Drawing Primitives =====

    /// Draws a line.
    pub fn draw_line(&mut self, p1: Point<Pixels>, p2: Point<Pixels>, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawLine {
            p1,
            p2,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a rectangle.
    pub fn draw_rect(&mut self, rect: Rect<Pixels>, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawRect {
            rect,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a rounded rectangle.
    pub fn draw_rrect(&mut self, rrect: RRect, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawRRect {
            rrect,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a circle.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `radius` is negative or NaN.
    pub fn draw_circle(&mut self, center: Point<Pixels>, radius: Pixels, paint: &Paint) {
        debug_assert!(
            radius.0 >= 0.0 && !radius.0.is_nan(),
            "Circle radius must be non-negative and not NaN, got: {}",
            radius.0
        );

        self.display_list.push(DrawCommand::DrawCircle {
            center,
            radius,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws an oval (ellipse) inscribed in the given rectangle.
    pub fn draw_oval(&mut self, rect: Rect<Pixels>, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawOval {
            rect,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws an arbitrary path.
    pub fn draw_path(&mut self, path: &Path, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawPath {
            path: path.clone(),
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws text.
    pub fn draw_text(
        &mut self,
        text: &str,
        offset: Offset<Pixels>,
        size: Size<Pixels>,
        style: &TextStyle,
        paint: &Paint,
    ) {
        self.display_list.push(DrawCommand::DrawText {
            text: text.to_string(),
            offset,
            size,
            style: style.clone(),
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws rich text with inline spans.
    pub fn draw_text_span(
        &mut self,
        span: &InlineSpan,
        offset: Offset<Pixels>,
        text_scale_factor: f64,
    ) {
        self.display_list.push(DrawCommand::DrawTextSpan {
            span: span.clone(),
            offset,
            text_scale_factor,
            transform: self.transform,
        });
    }

    /// Draws an image.
    pub fn draw_image(&mut self, image: Image, dst: Rect<Pixels>, paint: Option<&Paint>) {
        self.display_list.push(DrawCommand::DrawImage {
            image,
            dst,
            paint: paint.cloned(),
            transform: self.transform,
        });
    }

    /// Draws an image with tiling/repeat.
    pub fn draw_image_repeat(
        &mut self,
        image: Image,
        dst: Rect<Pixels>,
        repeat: ImageRepeat,
        paint: Option<&Paint>,
    ) {
        self.display_list.push(DrawCommand::DrawImageRepeat {
            image,
            dst,
            repeat,
            paint: paint.cloned(),
            transform: self.transform,
        });
    }

    /// Draws an image with 9-slice/9-patch scaling.
    pub fn draw_image_nine_slice(
        &mut self,
        image: Image,
        center_slice: Rect<Pixels>,
        dst: Rect<Pixels>,
        paint: Option<&Paint>,
    ) {
        self.display_list.push(DrawCommand::DrawImageNineSlice {
            image,
            center_slice,
            dst,
            paint: paint.cloned(),
            transform: self.transform,
        });
    }

    /// Draws an image with a color filter applied.
    pub fn draw_image_filtered(
        &mut self,
        image: Image,
        dst: Rect<Pixels>,
        filter: ColorFilter,
        paint: Option<&Paint>,
    ) {
        self.display_list.push(DrawCommand::DrawImageFiltered {
            image,
            dst,
            filter,
            paint: paint.cloned(),
            transform: self.transform,
        });
    }

    /// Draws a GPU texture referenced by ID.
    pub fn draw_texture(
        &mut self,
        texture_id: TextureId,
        dst: Rect<Pixels>,
        src: Option<Rect<Pixels>>,
        filter_quality: FilterQuality,
        opacity: f32,
    ) {
        self.display_list.push(DrawCommand::DrawTexture {
            texture_id,
            dst,
            src,
            filter_quality,
            opacity: opacity.clamp(0.0, 1.0),
            transform: self.transform,
        });
    }

    /// Draws a shadow.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `elevation` is negative or NaN.
    pub fn draw_shadow(&mut self, path: &Path, color: Color, elevation: f32) {
        debug_assert!(
            elevation >= 0.0 && !elevation.is_nan(),
            "Shadow elevation must be non-negative and not NaN, got: {}",
            elevation
        );

        self.display_list.push(DrawCommand::DrawShadow {
            path: path.clone(),
            color,
            elevation,
            transform: self.transform,
        });
    }

    /// Draws a gradient-filled rectangle.
    pub fn draw_gradient(&mut self, rect: Rect<Pixels>, shader: Shader) {
        self.display_list.push(DrawCommand::DrawGradient {
            rect,
            shader,
            transform: self.transform,
        });
    }

    /// Draws a gradient-filled rounded rectangle.
    pub fn draw_gradient_rrect(&mut self, rrect: RRect, shader: Shader) {
        self.display_list.push(DrawCommand::DrawGradientRRect {
            rrect,
            shader,
            transform: self.transform,
        });
    }

    /// Draws an arc segment.
    pub fn draw_arc(
        &mut self,
        rect: Rect<Pixels>,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) {
        self.display_list.push(DrawCommand::DrawArc {
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws difference between two rounded rectangles (ring/border).
    pub fn draw_drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawDRRect {
            outer,
            inner,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a sequence of points with the specified mode.
    pub fn draw_points_with_mode(
        &mut self,
        mode: PointMode,
        points: Vec<Point<Pixels>>,
        paint: &Paint,
    ) {
        self.display_list.push(DrawCommand::DrawPoints {
            mode,
            points,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws custom vertices with optional colors and texture
    /// coordinates.
    pub fn draw_vertices(
        &mut self,
        vertices: Vec<Point<Pixels>>,
        colors: Option<Vec<Color>>,
        tex_coords: Option<Vec<Point<Pixels>>>,
        indices: Vec<u16>,
        paint: &Paint,
    ) {
        self.display_list.push(DrawCommand::DrawVertices {
            vertices,
            colors,
            tex_coords,
            indices,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Fills entire canvas with a color (respects clipping).
    pub fn draw_color(&mut self, color: Color, blend_mode: BlendMode) {
        self.display_list.push(DrawCommand::DrawColor {
            color,
            blend_mode,
            transform: self.transform,
        });
    }

    /// Fills entire canvas with a paint (respects clipping).
    pub fn draw_paint(&mut self, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawPaint {
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a previously recorded `DisplayList` into this canvas.
    ///
    /// Replays all commands from the `DisplayList`. Useful for caching
    /// and reusing drawing commands.
    ///
    /// # Performance
    ///
    /// This always clones `picture`'s command vector (`O(N)`), even
    /// when `self` is empty. The zero-copy path is
    /// [`Self::extend_from`], which takes the source `Canvas` by
    /// value and swaps the vectors when `self` is empty (`O(1)`).
    /// Prefer `extend_from` when you control the source canvas;
    /// `draw_picture` is the right choice when the same `DisplayList`
    /// is replayed multiple times.
    pub fn draw_picture(&mut self, picture: &DisplayList) {
        self.display_list.append(picture.clone());
    }

    /// Draws multiple sprites from a texture atlas.
    ///
    /// `sprites[i]` is drawn under `transforms[i]`; if `colors` is
    /// `Some`, `colors[i]` tints the i-th sprite. The renderer
    /// (`flui-engine`) walks these vectors with `zip`, which silently
    /// truncates if lengths differ. A debug assertion catches the
    /// shape mismatch up front during tests; the release path falls
    /// through to `zip`'s truncation (cheaper than runtime checking
    /// in the hot path).
    pub fn draw_atlas(
        &mut self,
        image: Image,
        sprites: Vec<Rect<Pixels>>,
        transforms: Vec<Matrix4>,
        colors: Option<Vec<Color>>,
        blend_mode: BlendMode,
        paint: Option<&Paint>,
    ) {
        debug_assert_eq!(
            sprites.len(),
            transforms.len(),
            "Canvas::draw_atlas sprites and transforms length mismatch"
        );
        if let Some(ref c) = colors {
            debug_assert_eq!(
                sprites.len(),
                c.len(),
                "Canvas::draw_atlas sprites and colors length mismatch"
            );
        }

        self.display_list.push(DrawCommand::DrawAtlas {
            image,
            sprites,
            transforms,
            colors,
            blend_mode,
            paint: paint.cloned(),
            transform: self.transform,
        });
    }

    /// Applies a shader as a mask to child content.
    ///
    /// Wraps child drawing commands and applies a shader as an alpha
    /// mask. The shader determines the opacity at each pixel.
    pub fn draw_shader_mask<F>(
        &mut self,
        bounds: Rect<Pixels>,
        shader: Shader,
        blend_mode: BlendMode,
        draw_child: F,
    ) where
        F: FnOnce(&mut Canvas),
    {
        let mut child_canvas = Canvas::new();
        draw_child(&mut child_canvas);

        self.display_list.push(DrawCommand::ShaderMask {
            child: Box::new(child_canvas.finish()),
            shader,
            bounds,
            blend_mode,
            transform: self.transform,
        });
    }

    /// Draws a backdrop filter effect (frosted glass, blur, etc.).
    ///
    /// Applies an image filter to the backdrop content behind this
    /// layer, then optionally renders child content on top. Perfect
    /// for frosted glass modals, blurred backgrounds, and creative
    /// backdrop effects.
    pub fn draw_backdrop_filter<F>(
        &mut self,
        bounds: Rect<Pixels>,
        filter: ImageFilter,
        blend_mode: BlendMode,
        draw_child: Option<F>,
    ) where
        F: FnOnce(&mut Canvas),
    {
        let child_display_list = draw_child.map(|draw_fn| {
            let mut child_canvas = Canvas::new();
            draw_fn(&mut child_canvas);
            Box::new(child_canvas.finish())
        });

        self.display_list.push(DrawCommand::BackdropFilter {
            child: child_display_list,
            filter,
            bounds,
            blend_mode,
            transform: self.transform,
        });
    }

    // ===== Convenience Methods =====

    /// Draws a point as a small circle.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `radius` is negative or NaN.
    #[inline]
    pub fn draw_point(&mut self, point: Point<Pixels>, radius: f32, paint: &Paint) {
        self.draw_circle(point, Pixels(radius), paint);
    }

    /// Draws multiple points.
    pub fn draw_points(&mut self, points: &[Point<Pixels>], radius: f32, paint: &Paint) {
        for &point in points {
            self.draw_circle(point, Pixels(radius), paint);
        }
    }

    /// Draws a polyline (connected line segments).
    pub fn draw_polyline(&mut self, points: &[Point<Pixels>], paint: &Paint) {
        if points.len() < 2 {
            return;
        }

        for i in 0..points.len() - 1 {
            self.draw_line(points[i], points[i + 1], paint);
        }
    }
}
