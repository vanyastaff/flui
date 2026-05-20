//! Canvas syntactic sugar: caller-side ergonomic wrappers over the
//! primary `draw_*` API.
//!
//! Mythos chain U4 extracted these from the 3,305-LOC `canvas.rs` god
//! module. None of these methods emit `DrawCommand` variants directly;
//! they all delegate to the primary methods in [`super::drawing`],
//! [`super::transform`], [`super::clipping`], [`super::state`], or
//! [`super::scoped`].
//!
//! Contents:
//!
//! - **Chaining API** (~40 methods): fluent `translated()` /
//!   `rotated()` / `rect()` / `circle()` / `also()` / `when()` -- all
//!   returning `&mut Self`.
//! - **Batch drawing**: `draw_rects` / `draw_circles` / `draw_lines` /
//!   `draw_rrects` / `draw_paths` -- loop wrappers over the
//!   single-shape primary methods.
//! - **Convenience shapes**: `draw_rounded_rect` /
//!   `draw_rounded_rect_corners` / `draw_pill` / `draw_ring` -- common
//!   compound shapes built from primary primitives.
//! - **Debug visualization**: `debug_rect` / `debug_point` /
//!   `debug_axes` / `debug_grid` -- diagnostic drawing helpers.
//! - **Conditional draws**: `draw_rect_if` / `draw_circle_if` /
//!   `draw_if` / `draw_unless` / `draw_if_some` -- avoid verbose `if`
//!   statements.
//! - **Grid / repeat patterns**: `draw_grid` / `repeat_x` / `repeat_y`
//!   / `repeat_radial` -- recurrent layouts.

use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, Radius, Rect, Size, px},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};

use super::Canvas;
use crate::display_list::{
    ColorFilter, FilterQuality, ImageRepeat, Paint, PointMode, Shader, TextureId,
};

// ===== Batch Drawing =====

impl Canvas {
    /// Draws multiple rectangles with the same paint.
    #[inline]
    pub fn draw_rects(&mut self, rects: &[Rect<Pixels>], paint: &Paint) {
        for rect in rects {
            self.draw_rect(*rect, paint);
        }
    }

    /// Draws multiple circles with the same paint.
    #[inline]
    pub fn draw_circles(&mut self, circles: &[(Point<Pixels>, f32)], paint: &Paint) {
        for (center, radius) in circles {
            self.draw_circle(*center, Pixels(*radius), paint);
        }
    }

    /// Draws multiple lines with the same paint.
    #[inline]
    pub fn draw_lines(&mut self, lines: &[(Point<Pixels>, Point<Pixels>)], paint: &Paint) {
        for (p1, p2) in lines {
            self.draw_line(*p1, *p2, paint);
        }
    }

    /// Draws multiple rounded rectangles with the same paint.
    #[inline]
    pub fn draw_rrects(&mut self, rrects: &[RRect], paint: &Paint) {
        for rrect in rrects {
            self.draw_rrect(*rrect, paint);
        }
    }

    /// Draws multiple paths with the same paint.
    #[inline]
    pub fn draw_paths(&mut self, paths: &[&Path], paint: &Paint) {
        for path in paths {
            self.draw_path(path, paint);
        }
    }
}

// ===== Conditional Drawing =====

impl Canvas {
    /// Draws a rectangle only if the condition is true.
    #[inline]
    pub fn draw_rect_if(&mut self, condition: bool, rect: Rect<Pixels>, paint: &Paint) {
        if condition {
            self.draw_rect(rect, paint);
        }
    }

    /// Draws a circle only if the condition is true.
    #[inline]
    pub fn draw_circle_if(
        &mut self,
        condition: bool,
        center: Point<Pixels>,
        radius: Pixels,
        paint: &Paint,
    ) {
        if condition {
            self.draw_circle(center, radius, paint);
        }
    }

    /// Executes drawing closure only if the condition is true.
    #[inline]
    pub fn draw_if<F>(&mut self, condition: bool, f: F)
    where
        F: FnOnce(&mut Self),
    {
        if condition {
            f(self);
        }
    }

    /// Executes drawing closure only if the condition is false.
    #[inline]
    pub fn draw_unless<F>(&mut self, condition: bool, f: F)
    where
        F: FnOnce(&mut Self),
    {
        if !condition {
            f(self);
        }
    }

    /// Draws based on Option - draws if Some, skips if None.
    #[inline]
    pub fn draw_if_some<T, F>(&mut self, option: Option<T>, f: F)
    where
        F: FnOnce(&mut Self, T),
    {
        if let Some(value) = option {
            f(self, value);
        }
    }
}

// ===== Grid and Repeat Patterns =====

impl Canvas {
    /// Draws a grid of items using a closure.
    pub fn draw_grid<F>(
        &mut self,
        cols: usize,
        rows: usize,
        cell_width: f32,
        cell_height: f32,
        f: F,
    ) where
        F: Fn(&mut Self, usize, usize),
    {
        for row in 0..rows {
            for col in 0..cols {
                self.with_translate(col as f32 * cell_width, row as f32 * cell_height, |c| {
                    f(c, col, row);
                });
            }
        }
    }

    /// Repeats a drawing operation in a horizontal line.
    pub fn repeat_x<F>(&mut self, count: usize, spacing: f32, f: F)
    where
        F: Fn(&mut Self, usize),
    {
        for i in 0..count {
            self.with_translate(i as f32 * spacing, 0.0, |c| {
                f(c, i);
            });
        }
    }

    /// Repeats a drawing operation in a vertical line.
    pub fn repeat_y<F>(&mut self, count: usize, spacing: f32, f: F)
    where
        F: Fn(&mut Self, usize),
    {
        for i in 0..count {
            self.with_translate(0.0, i as f32 * spacing, |c| {
                f(c, i);
            });
        }
    }

    /// Repeats a drawing operation around a circle.
    pub fn repeat_radial<F>(&mut self, count: usize, radius: f32, f: F)
    where
        F: Fn(&mut Self, usize, f32),
    {
        use std::f32::consts::PI;
        let angle_step = 2.0 * PI / count as f32;

        for i in 0..count {
            let angle = i as f32 * angle_step;
            let x = angle.cos() * radius;
            let y = angle.sin() * radius;

            self.with_translate(x, y, |c| {
                f(c, i, angle);
            });
        }
    }
}

// ===== Debug Visualization =====

impl Canvas {
    /// Draws a debug rectangle showing bounds (outline only).
    #[inline]
    pub fn debug_rect(&mut self, rect: Rect<Pixels>, color: Color) {
        let paint = Paint::stroke(color, 1.0);
        self.draw_rect(rect, &paint);
    }

    /// Draws a debug cross at the specified point.
    #[inline]
    pub fn debug_point(&mut self, point: Point<Pixels>, size: f32, color: Color) {
        let half = size / 2.0;
        let paint = Paint::stroke(color, 1.0);
        self.draw_line(
            Point::new(Pixels(point.x.0 - half), point.y),
            Point::new(Pixels(point.x.0 + half), point.y),
            &paint,
        );
        self.draw_line(
            Point::new(point.x, Pixels(point.y.0 - half)),
            Point::new(point.x, Pixels(point.y.0 + half)),
            &paint,
        );
    }

    /// Draws debug visualization of the current transform origin.
    #[inline]
    pub fn debug_axes(&mut self, length: f32) {
        let origin = Point::new(Pixels(0.0), Pixels(0.0));

        self.draw_line(
            origin,
            Point::new(Pixels(length), Pixels(0.0)),
            &Paint::stroke(Color::RED, 2.0),
        );

        self.draw_line(
            origin,
            Point::new(Pixels(0.0), Pixels(length)),
            &Paint::stroke(Color::GREEN, 2.0),
        );

        self.draw_circle(origin, px(3.0), &Paint::fill(Color::BLUE));
    }

    /// Draws a debug grid overlay.
    ///
    /// # Panics
    ///
    /// Panics if `spacing` is non-positive (`<= 0.0`), NaN, or
    /// `INFINITY` — otherwise the `while x += spacing` loops below
    /// run forever (or step by zero).
    pub fn debug_grid(&mut self, bounds: Rect<Pixels>, spacing: f32, color: Color) {
        assert!(
            spacing > 0.0 && spacing.is_finite(),
            "Canvas::debug_grid spacing must be positive and finite, got {spacing}"
        );

        let paint = Paint::stroke(color, 0.5);

        let mut x = bounds.left();
        while x <= bounds.right() {
            self.draw_line(
                Point::new(x, bounds.top()),
                Point::new(x, bounds.bottom()),
                &paint,
            );
            x += Pixels(spacing);
        }

        let mut y = bounds.top();
        while y <= bounds.bottom() {
            self.draw_line(
                Point::new(bounds.left(), y),
                Point::new(bounds.right(), y),
                &paint,
            );
            y += Pixels(spacing);
        }
    }
}

// ===== Convenience Shape Methods =====

impl Canvas {
    /// Draws a rounded rectangle with uniform corner radius.
    #[inline]
    pub fn draw_rounded_rect(&mut self, rect: Rect<Pixels>, radius: Pixels, paint: &Paint) {
        let rrect = RRect::from_rect_circular(rect, radius);
        self.draw_rrect(rrect, paint);
    }

    /// Draws a rectangle with different corner radii.
    #[inline]
    pub fn draw_rounded_rect_corners(
        &mut self,
        rect: Rect<Pixels>,
        top_left: f32,
        top_right: f32,
        bottom_right: f32,
        bottom_left: f32,
        paint: &Paint,
    ) {
        let rrect = RRect::from_rect_and_corners(
            rect,
            Radius::circular(px(top_left)),
            Radius::circular(px(top_right)),
            Radius::circular(px(bottom_right)),
            Radius::circular(px(bottom_left)),
        );
        self.draw_rrect(rrect, paint);
    }

    /// Draws a pill shape (fully rounded rectangle).
    ///
    /// The corner radius is automatically set to half the smaller
    /// dimension.
    #[inline]
    pub fn draw_pill(&mut self, rect: Rect<Pixels>, paint: &Paint) {
        let radius = rect.width().min(rect.height()).0 / 2.0;
        self.draw_rounded_rect(rect, Pixels(radius), paint);
    }

    /// Draws a ring (donut shape).
    #[inline]
    pub fn draw_ring(
        &mut self,
        center: Point<Pixels>,
        outer_radius: f32,
        inner_radius: f32,
        paint: &Paint,
    ) {
        let outer = RRect::from_rect_circular(
            Rect::from_center_size(
                center,
                Size::new(px(outer_radius * 2.0), px(outer_radius * 2.0)),
            ),
            px(outer_radius),
        );
        let inner = RRect::from_rect_circular(
            Rect::from_center_size(
                center,
                Size::new(px(inner_radius * 2.0), px(inner_radius * 2.0)),
            ),
            px(inner_radius),
        );
        self.draw_drrect(outer, inner, paint);
    }
}

// ===== Chaining API =====
//
// These methods return `&mut Self` for fluent method chaining.

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
