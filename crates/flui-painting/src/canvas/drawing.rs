//! The `draw_*` methods, each recording one [`DrawOp`] under the current
//! transform (`Canvas::record`).
//!
//! # Allocation hot path
//!
//! `Paint` is now interned through `Canvas::intern_paint` (crate-private).
//! On the second-and-later draw of an identical paint, the call
//! returns an `Arc::clone` (single atomic refcount bump) instead of
//! a full `Paint::clone` (~80–200 bytes incl. optional `Box<Shader>`
//! payload). First-use still allocates one `Arc::new(paint.clone())`
//! to seed the pool; subsequent uses are O(1) refcount bumps
//! amortised across the recording.
//!
//! `Path` clones are O(1): its command buffer is copy-on-write
//! (`Arc<Vec<PathCommand>>`), so `draw_path`, `draw_shadow`, and `clip_path`
//! share the caller's buffer until either side mutates.

use std::sync::Arc;

use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, Rect},
    painting::{Image, Path},
    styling::Color,
};

use super::Canvas;
use crate::display_list::{
    BlendMode, ColorFilter, DisplayList, DrawCommand, DrawOp, FilterQuality, ImageRepeat, Paint,
    PointMode, TextureId,
};
use crate::text_layout::TextLayout;

impl Canvas {
    // ===== Drawing Primitives =====

    /// Draws a line.
    pub fn draw_line(&mut self, p1: Point<Pixels>, p2: Point<Pixels>, paint: &Paint) {
        let paint = self.intern_paint(paint);
        self.record(DrawOp::Line { p1, p2, paint });
    }

    /// Draws a rectangle.
    pub fn draw_rect(&mut self, rect: Rect<Pixels>, paint: &Paint) {
        let paint = self.intern_paint(paint);
        self.record(DrawOp::Rect { rect, paint });
    }

    /// Draws a rounded rectangle.
    pub fn draw_rrect(&mut self, rrect: RRect, paint: &Paint) {
        let paint = self.intern_paint(paint);
        self.record(DrawOp::RRect { rrect, paint });
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

        let paint = self.intern_paint(paint);
        self.record(DrawOp::Circle {
            center,
            radius,
            paint,
        });
    }

    /// Draws an oval (ellipse) inscribed in the given rectangle.
    pub fn draw_oval(&mut self, rect: Rect<Pixels>, paint: &Paint) {
        let paint = self.intern_paint(paint);
        self.record(DrawOp::Oval { rect, paint });
    }

    /// Draws an arbitrary path.
    pub fn draw_path(&mut self, path: &Path, paint: &Paint) {
        let paint = self.intern_paint(paint);
        self.record(DrawOp::Path {
            path: path.clone(),
            paint,
        });
    }

    /// Draws a shaped paragraph with its top-left at `offset`.
    ///
    /// `layout` is the very layout the caller measured (`TextPainter::paint`
    /// hands over its cache's `Arc`), so line breaks, truncation, and the
    /// ellipsis paint exactly as they were laid out. `color` paints every
    /// glyph without a span colour of its own.
    pub fn draw_paragraph(
        &mut self,
        layout: &Arc<TextLayout>,
        offset: Offset<Pixels>,
        color: Color,
    ) {
        self.record(DrawOp::Paragraph {
            layout: Arc::clone(layout),
            offset,
            color,
        });
    }

    /// Draws an image.
    pub fn draw_image(&mut self, image: Image, dst: Rect<Pixels>, paint: Option<&Paint>) {
        let paint = self.intern_optional_paint(paint);
        self.record(DrawOp::Image { image, dst, paint });
    }

    /// Draws an image with tiling/repeat.
    pub fn draw_image_repeat(
        &mut self,
        image: Image,
        dst: Rect<Pixels>,
        repeat: ImageRepeat,
        paint: Option<&Paint>,
    ) {
        let paint = self.intern_optional_paint(paint);
        self.record(DrawOp::ImageRepeat {
            image,
            dst,
            repeat,
            paint,
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
        let paint = self.intern_optional_paint(paint);
        self.record(DrawOp::ImageNineSlice {
            image,
            center_slice,
            dst,
            paint,
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
        let paint = self.intern_optional_paint(paint);
        self.record(DrawOp::ImageFiltered {
            image,
            dst,
            filter,
            paint,
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
        self.record(DrawOp::Texture {
            texture_id,
            dst,
            src,
            filter_quality,
            opacity: opacity.clamp(0.0, 1.0),
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

        self.record(DrawOp::Shadow {
            path: path.clone(),
            color,
            elevation,
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
        let paint = self.intern_paint(paint);
        self.record(DrawOp::Arc {
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint,
        });
    }

    /// Draws difference between two rounded rectangles (ring/border).
    pub fn draw_drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint) {
        let paint = self.intern_paint(paint);
        self.record(DrawOp::DRRect {
            outer,
            inner,
            paint,
        });
    }

    /// Draws a sequence of points with the specified mode.
    pub fn draw_points_with_mode(
        &mut self,
        mode: PointMode,
        points: Vec<Point<Pixels>>,
        paint: &Paint,
    ) {
        let paint = self.intern_paint(paint);
        self.record(DrawOp::Points {
            mode,
            points,
            paint,
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
        let paint = self.intern_paint(paint);
        self.record(DrawOp::Vertices {
            vertices,
            colors,
            tex_coords,
            indices,
            paint,
        });
    }

    /// Fills entire canvas with a color (respects clipping).
    pub fn draw_color(&mut self, color: Color, blend_mode: BlendMode) {
        self.record(DrawOp::Color { color, blend_mode });
    }

    /// Fills entire canvas with a paint (respects clipping).
    pub fn draw_paint(&mut self, paint: &Paint) {
        let paint = self.intern_paint(paint);
        self.record(DrawOp::Paint { paint });
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

        let paint = self.intern_optional_paint(paint);
        self.record(DrawOp::Atlas {
            image,
            sprites,
            transforms,
            colors,
            blend_mode,
            paint,
        });
    }

    /// Replays a recorded display list under the current transform.
    ///
    /// Every command is re-stamped as `ctm * command.transform` — a picture
    /// recorded at the origin lands wherever the canvas is currently
    /// translated, scaled, or rotated to. Paints are shared by `Arc`, so a
    /// replay allocates nothing per command beyond the command itself.
    /// `Save`/`Restore` scopes and clips replay as recorded; the picture's
    /// clips cannot leak into this canvas only if the picture was balanced.
    pub fn draw_picture(&mut self, picture: &DisplayList) {
        let ctm = self.transform;
        for command in picture {
            self.display_list.push(DrawCommand {
                transform: ctm * command.transform,
                op: command.op.clone(),
            });
        }
    }
}
