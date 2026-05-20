//! Chaining API: ~30 fluent wrappers returning `&mut Self`.
//!
//! Each method here delegates to a primary `draw_*` / `clip_*` /
//! transform / state method and returns `&mut self` so call sites
//! can fluently chain operations:
//!
//! ```ignore
//! canvas
//!     .saved()
//!     .translated(20.0, 20.0)
//!     .rect(rect, &paint)
//!     .restored();
//! ```
//!
//! The module also carries the closure combinators
//! [`Canvas::also`], [`Canvas::when`], and [`Canvas::when_else`] for
//! conditional fluent flow.

use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, Rect, Size},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};

use crate::canvas::Canvas;
use crate::display_list::{
    ColorFilter, FilterQuality, ImageRepeat, Paint, PointMode, Shader, TextureId,
};

impl Canvas {
    /// Translates and returns self for chaining.
    #[inline]
    pub fn translated(&mut self, dx: f32, dy: f32) -> &mut Self {
        self.translate(dx, dy);
        self
    }

    /// Scales uniformly and returns self for chaining.
    #[inline]
    pub fn scaled(&mut self, factor: f32) -> &mut Self {
        self.scale_uniform(factor);
        self
    }

    /// Scales non-uniformly and returns self for chaining.
    #[inline]
    pub fn scaled_xy(&mut self, sx: f32, sy: f32) -> &mut Self {
        self.scale_xy(sx, sy);
        self
    }

    /// Rotates and returns self for chaining.
    #[inline]
    pub fn rotated(&mut self, radians: f32) -> &mut Self {
        self.rotate(radians);
        self
    }

    /// Rotates around a pivot and returns self for chaining.
    #[inline]
    pub fn rotated_around(&mut self, radians: f32, pivot_x: f32, pivot_y: f32) -> &mut Self {
        self.rotate_around(radians, pivot_x, pivot_y);
        self
    }

    /// Applies a transform and returns self for chaining.
    #[inline]
    pub fn transformed<T: Into<Matrix4>>(&mut self, transform: T) -> &mut Self {
        self.transform(transform);
        self
    }

    /// Clips to a rectangle and returns self for chaining.
    #[inline]
    pub fn clipped_rect(&mut self, rect: Rect<Pixels>) -> &mut Self {
        self.clip_rect(rect);
        self
    }

    /// Clips to a rounded rectangle and returns self for chaining.
    #[inline]
    pub fn clipped_rrect(&mut self, rrect: RRect) -> &mut Self {
        self.clip_rrect(rrect);
        self
    }

    /// Clips to a path and returns self for chaining.
    #[inline]
    pub fn clipped_path(&mut self, path: &Path) -> &mut Self {
        self.clip_path(path);
        self
    }

    /// Saves state and returns self for chaining.
    #[inline]
    pub fn saved(&mut self) -> &mut Self {
        self.save();
        self
    }

    /// Restores state and returns self for chaining.
    #[inline]
    pub fn restored(&mut self) -> &mut Self {
        self.restore();
        self
    }

    /// Draws a rect and returns self for chaining.
    #[inline]
    pub fn rect(&mut self, rect: Rect<Pixels>, paint: &Paint) -> &mut Self {
        self.draw_rect(rect, paint);
        self
    }

    /// Draws a rounded rect and returns self for chaining.
    #[inline]
    pub fn rrect(&mut self, rrect: RRect, paint: &Paint) -> &mut Self {
        self.draw_rrect(rrect, paint);
        self
    }

    /// Draws a rectangle with uniform corner radius and returns self
    /// for chaining.
    #[inline]
    pub fn rounded_rect(&mut self, rect: Rect<Pixels>, radius: f32, paint: &Paint) -> &mut Self {
        self.draw_rounded_rect(rect, Pixels(radius), paint);
        self
    }

    /// Draws a circle and returns self for chaining.
    #[inline]
    pub fn circle(&mut self, center: Point<Pixels>, radius: f32, paint: &Paint) -> &mut Self {
        self.draw_circle(center, Pixels(radius), paint);
        self
    }

    /// Draws a line and returns self for chaining.
    #[inline]
    pub fn line(&mut self, p1: Point<Pixels>, p2: Point<Pixels>, paint: &Paint) -> &mut Self {
        self.draw_line(p1, p2, paint);
        self
    }

    /// Draws a path and returns self for chaining.
    #[inline]
    pub fn path(&mut self, path: &Path, paint: &Paint) -> &mut Self {
        self.draw_path(path, paint);
        self
    }

    /// Draws text and returns self for chaining.
    #[inline]
    pub fn text(
        &mut self,
        text: &str,
        offset: Offset<Pixels>,
        size: Size<Pixels>,
        style: &TextStyle,
        paint: &Paint,
    ) -> &mut Self {
        self.draw_text(text, offset, size, style, paint);
        self
    }

    /// Draws an oval and returns self for chaining.
    #[inline]
    pub fn oval(&mut self, rect: Rect<Pixels>, paint: &Paint) -> &mut Self {
        self.draw_oval(rect, paint);
        self
    }

    /// Draws a texture and returns self for chaining.
    #[inline]
    pub fn texture(
        &mut self,
        texture_id: TextureId,
        dst: Rect<Pixels>,
        src: Option<Rect<Pixels>>,
        filter_quality: FilterQuality,
        opacity: f32,
    ) -> &mut Self {
        self.draw_texture(texture_id, dst, src, filter_quality, opacity);
        self
    }

    /// Draws an image and returns self for chaining.
    #[inline]
    pub fn image(&mut self, image: Image, dst: Rect<Pixels>, paint: Option<&Paint>) -> &mut Self {
        self.draw_image(image, dst, paint);
        self
    }

    /// Draws a tiled/repeated image and returns self for chaining.
    #[inline]
    pub fn image_repeat(
        &mut self,
        image: Image,
        dst: Rect<Pixels>,
        repeat: ImageRepeat,
        paint: Option<&Paint>,
    ) -> &mut Self {
        self.draw_image_repeat(image, dst, repeat, paint);
        self
    }

    /// Draws an image with 9-slice scaling and returns self for
    /// chaining.
    #[inline]
    pub fn image_nine_slice(
        &mut self,
        image: Image,
        center_slice: Rect<Pixels>,
        dst: Rect<Pixels>,
        paint: Option<&Paint>,
    ) -> &mut Self {
        self.draw_image_nine_slice(image, center_slice, dst, paint);
        self
    }

    /// Draws an image with a color filter and returns self for
    /// chaining.
    #[inline]
    pub fn image_filtered(
        &mut self,
        image: Image,
        dst: Rect<Pixels>,
        filter: ColorFilter,
        paint: Option<&Paint>,
    ) -> &mut Self {
        self.draw_image_filtered(image, dst, filter, paint);
        self
    }

    /// Draws a shadow and returns self for chaining.
    #[inline]
    pub fn shadow(&mut self, path: &Path, color: Color, elevation: f32) -> &mut Self {
        self.draw_shadow(path, color, elevation);
        self
    }

    /// Draws a gradient-filled rectangle and returns self for chaining.
    #[inline]
    pub fn gradient(&mut self, rect: Rect<Pixels>, shader: Shader) -> &mut Self {
        self.draw_gradient(rect, shader);
        self
    }

    /// Draws a gradient-filled rounded rectangle and returns self for
    /// chaining.
    #[inline]
    pub fn gradient_rrect(&mut self, rrect: RRect, shader: Shader) -> &mut Self {
        self.draw_gradient_rrect(rrect, shader);
        self
    }

    /// Draws an arc segment and returns self for chaining.
    #[inline]
    pub fn arc(
        &mut self,
        rect: Rect<Pixels>,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) -> &mut Self {
        self.draw_arc(rect, start_angle, sweep_angle, use_center, paint);
        self
    }

    /// Draws difference between two rounded rectangles and returns
    /// self for chaining.
    #[inline]
    pub fn drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint) -> &mut Self {
        self.draw_drrect(outer, inner, paint);
        self
    }

    /// Draws points with the specified mode and returns self for
    /// chaining.
    #[inline]
    pub fn points(
        &mut self,
        mode: PointMode,
        points: Vec<Point<Pixels>>,
        paint: &Paint,
    ) -> &mut Self {
        self.draw_points_with_mode(mode, points, paint);
        self
    }

    /// Draws custom vertices and returns self for chaining.
    #[inline]
    pub fn vertices(
        &mut self,
        vertices: Vec<Point<Pixels>>,
        colors: Option<Vec<Color>>,
        tex_coords: Option<Vec<Point<Pixels>>>,
        indices: Vec<u16>,
        paint: &Paint,
    ) -> &mut Self {
        self.draw_vertices(vertices, colors, tex_coords, indices, paint);
        self
    }

    /// Executes a closure on self and returns self for chaining.
    #[inline]
    pub fn also<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut Self),
    {
        f(self);
        self
    }

    /// Conditionally executes a closure on self.
    #[inline]
    pub fn when<F>(&mut self, condition: bool, f: F) -> &mut Self
    where
        F: FnOnce(&mut Self) -> &mut Self,
    {
        if condition { f(self) } else { self }
    }

    /// Conditionally executes one of two closures.
    #[inline]
    pub fn when_else<F, G>(&mut self, condition: bool, if_true: F, if_false: G) -> &mut Self
    where
        F: FnOnce(&mut Self) -> &mut Self,
        G: FnOnce(&mut Self) -> &mut Self,
    {
        if condition {
            if_true(self)
        } else {
            if_false(self)
        }
    }
}
