//! RenderImage - Displays a raster image
//!
//! Flutter reference: <https://api.flutter.dev/flutter/rendering/RenderImage-class.html>

use crate::core::{BoxProtocol, LayoutContext, Leaf, PaintContext, RenderBox};
use flui_painting::Paint;
use flui_types::{painting::Image, typography::TextDirection, Alignment, Color, Rect, Size};

/// How an image should be inscribed into the allocated space
///
/// Flutter reference: <https://api.flutter.dev/flutter/painting/BoxFit.html>
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ImageFit {
    /// Fill the entire space, distorting aspect ratio if needed
    Fill,
    /// Maintain aspect ratio, may leave empty space (as large as possible while containing)
    #[default]
    Contain,
    /// Maintain aspect ratio, may clip to fill space (as small as possible while covering)
    Cover,
    /// Fit to width, may overflow or underflow height
    FitWidth,
    /// Fit to height, may overflow or underflow width
    FitHeight,
    /// Use image's intrinsic size, no scaling (centered, clips if too large)
    None,
    /// Scale down to fit if too large, otherwise use intrinsic size
    ScaleDown,
}

/// How to paint any portions of a box not covered by an image
///
/// Flutter reference: <https://api.flutter.dev/flutter/painting/ImageRepeat.html>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageRepeat {
    /// Repeat the image in both the x and y directions
    Repeat,
    /// Repeat the image in the x direction only
    RepeatX,
    /// Repeat the image in the y direction only
    RepeatY,
    /// Leave uncovered portions transparent (no repeat)
    #[default]
    NoRepeat,
}

/// How to blend the image color with pixels
///
/// Common blend modes for image tinting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorBlendMode {
    /// No blending, use original image colors
    #[default]
    None,
    /// Multiply image colors with the blend color
    Multiply,
    /// Screen blend mode
    Screen,
    /// Overlay blend mode
    Overlay,
    /// Darken blend mode
    Darken,
    /// Lighten blend mode
    Lighten,
    /// Color dodge blend mode
    ColorDodge,
    /// Color burn blend mode
    ColorBurn,
    /// Source in - shows source where destination exists
    SrcIn,
    /// Source atop - shows source on top of destination
    SrcATop,
    /// Modulate - multiplies colors
    Modulate,
}

// Re-export FilterQuality from flui_types for backwards compatibility
pub use flui_types::painting::FilterQuality;

/// RenderObject that displays a raster image
///
/// Renders an image using Canvas::draw_image. The image is scaled
/// according to the fit mode and aligned within the constraints.
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `RenderImage` class.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderImage, ImageFit, ImageRepeat};
/// use flui_types::painting::Image;
///
/// let image = Image::from_rgba8(100, 100, vec![255; 100 * 100 * 4]);
/// let render_image = RenderImage::new(image)
///     .with_fit(ImageFit::Cover)
///     .with_repeat(ImageRepeat::Repeat);
/// ```
#[derive(Debug)]
pub struct RenderImage {
    /// The image to display
    pub image: Image,

    /// Optional explicit width (overrides intrinsic)
    pub width: Option<f32>,

    /// Optional explicit height (overrides intrinsic)
    pub height: Option<f32>,

    /// Scale factor for the image (default: 1.0)
    pub scale: f32,

    /// How the image should fit in the available space
    pub fit: ImageFit,

    /// How to align the image within its bounds
    pub alignment: Alignment,

    /// How to repeat the image to fill the space
    pub repeat: ImageRepeat,

    /// Center slice for 9-patch stretching (left, top, right, bottom)
    pub center_slice: Option<Rect>,

    /// Color to blend with the image pixels
    pub color: Option<Color>,

    /// How to blend the color with image pixels
    pub color_blend_mode: ColorBlendMode,

    /// Opacity of the image (0.0 to 1.0)
    pub opacity: f32,

    /// Whether to flip the image horizontally for RTL text direction
    pub match_text_direction: bool,

    /// Text direction for alignment and flipping
    pub text_direction: TextDirection,

    /// Whether to invert the image colors
    pub invert_colors: bool,

    /// Whether to use anti-aliasing when painting the image
    pub is_anti_alias: bool,

    /// Quality of filtering when scaling the image
    pub filter_quality: FilterQuality,

    /// Optional paint for additional effects
    paint: Option<Paint>,
}

impl RenderImage {
    /// Creates a new RenderImage with default settings
    pub fn new(image: Image) -> Self {
        Self {
            image,
            width: None,
            height: None,
            scale: 1.0,
            fit: ImageFit::default(),
            alignment: Alignment::CENTER,
            repeat: ImageRepeat::default(),
            center_slice: None,
            color: None,
            color_blend_mode: ColorBlendMode::default(),
            opacity: 1.0,
            match_text_direction: false,
            text_direction: TextDirection::Ltr,
            invert_colors: false,
            is_anti_alias: false,
            filter_quality: FilterQuality::default(),
            paint: None,
        }
    }

    /// Sets the image
    pub fn set_image(&mut self, image: Image) {
        self.image = image;
    }

    /// Sets explicit width
    pub fn set_width(&mut self, width: Option<f32>) {
        self.width = width;
    }

    /// Sets explicit height
    pub fn set_height(&mut self, height: Option<f32>) {
        self.height = height;
    }

    /// Sets the scale factor
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
    }

    /// Sets the fit mode
    pub fn set_fit(&mut self, fit: ImageFit) {
        self.fit = fit;
    }

    /// Sets the alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }

    /// Sets the repeat mode
    pub fn set_repeat(&mut self, repeat: ImageRepeat) {
        self.repeat = repeat;
    }

    /// Sets the center slice for 9-patch
    pub fn set_center_slice(&mut self, center_slice: Option<Rect>) {
        self.center_slice = center_slice;
    }

    /// Sets the color tint
    pub fn set_color(&mut self, color: Option<Color>) {
        self.color = color;
    }

    /// Sets the color blend mode
    pub fn set_color_blend_mode(&mut self, mode: ColorBlendMode) {
        self.color_blend_mode = mode;
    }

    /// Sets the opacity
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Sets whether to match text direction
    pub fn set_match_text_direction(&mut self, match_text_direction: bool) {
        self.match_text_direction = match_text_direction;
    }

    /// Sets the text direction
    pub fn set_text_direction(&mut self, text_direction: TextDirection) {
        self.text_direction = text_direction;
    }

    /// Sets whether to invert colors
    pub fn set_invert_colors(&mut self, invert_colors: bool) {
        self.invert_colors = invert_colors;
    }

    /// Sets whether to use anti-aliasing
    pub fn set_is_anti_alias(&mut self, is_anti_alias: bool) {
        self.is_anti_alias = is_anti_alias;
    }

    /// Sets the filter quality
    pub fn set_filter_quality(&mut self, filter_quality: FilterQuality) {
        self.filter_quality = filter_quality;
    }

    // ===== Builder methods =====

    /// Sets the fit mode (builder pattern)
    pub fn with_fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Sets the alignment (builder pattern)
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Sets the repeat mode (builder pattern)
    pub fn with_repeat(mut self, repeat: ImageRepeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// Sets explicit dimensions (builder pattern)
    pub fn with_dimensions(mut self, width: Option<f32>, height: Option<f32>) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Sets the scale factor (builder pattern)
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Sets the color tint (builder pattern)
    pub fn with_color(mut self, color: Color, blend_mode: ColorBlendMode) -> Self {
        self.color = Some(color);
        self.color_blend_mode = blend_mode;
        self
    }

    /// Sets the opacity (builder pattern)
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Sets the center slice for 9-patch (builder pattern)
    pub fn with_center_slice(mut self, center_slice: Rect) -> Self {
        self.center_slice = Some(center_slice);
        self
    }

    /// Sets the filter quality (builder pattern)
    pub fn with_filter_quality(mut self, filter_quality: FilterQuality) -> Self {
        self.filter_quality = filter_quality;
        self
    }

    /// Sets the paint (for additional effects, builder pattern)
    pub fn with_paint(mut self, paint: Paint) -> Self {
        self.paint = Some(paint);
        self
    }

    /// Enables anti-aliasing (builder pattern)
    pub fn anti_aliased(mut self) -> Self {
        self.is_anti_alias = true;
        self
    }

    /// Enables color inversion (builder pattern)
    pub fn inverted(mut self) -> Self {
        self.invert_colors = true;
        self
    }

    /// Enables text direction matching (builder pattern)
    pub fn match_direction(mut self, text_direction: TextDirection) -> Self {
        self.match_text_direction = true;
        self.text_direction = text_direction;
        self
    }

    // ===== Internal methods =====

    /// Get the intrinsic width of the image
    fn intrinsic_width(&self) -> f32 {
        (self.image.width() as f32) / self.scale
    }

    /// Get the intrinsic height of the image
    fn intrinsic_height(&self) -> f32 {
        (self.image.height() as f32) / self.scale
    }

    /// Calculates the destination rectangle for the image
    fn calculate_dest_rect(&self, available_size: Size) -> Rect {
        let image_width = self.width.unwrap_or_else(|| self.intrinsic_width());
        let image_height = self.height.unwrap_or_else(|| self.intrinsic_height());

        if image_width == 0.0 || image_height == 0.0 {
            return Rect::from_xywh(0.0, 0.0, 0.0, 0.0);
        }

        let image_aspect = image_width / image_height;
        let space_aspect = if available_size.height > 0.0 {
            available_size.width / available_size.height
        } else {
            1.0
        };

        let (dest_width, dest_height) = match self.fit {
            ImageFit::Fill => (available_size.width, available_size.height),
            ImageFit::Contain => {
                if space_aspect > image_aspect {
                    let width = available_size.height * image_aspect;
                    (width, available_size.height)
                } else {
                    let height = available_size.width / image_aspect;
                    (available_size.width, height)
                }
            }
            ImageFit::Cover => {
                if space_aspect > image_aspect {
                    let height = available_size.width / image_aspect;
                    (available_size.width, height)
                } else {
                    let width = available_size.height * image_aspect;
                    (width, available_size.height)
                }
            }
            ImageFit::FitWidth => {
                let height = available_size.width / image_aspect;
                (available_size.width, height)
            }
            ImageFit::FitHeight => {
                let width = available_size.height * image_aspect;
                (width, available_size.height)
            }
            ImageFit::None => (image_width, image_height),
            ImageFit::ScaleDown => {
                if image_width <= available_size.width && image_height <= available_size.height {
                    (image_width, image_height)
                } else if space_aspect > image_aspect {
                    let width = available_size.height * image_aspect;
                    (width, available_size.height)
                } else {
                    let height = available_size.width / image_aspect;
                    (available_size.width, height)
                }
            }
        };

        // Calculate offset based on alignment
        // Alignment: -1.0 = left/top, 0.0 = center, 1.0 = right/bottom
        let dx = (available_size.width - dest_width) * (self.alignment.x + 1.0) / 2.0;
        let dy = (available_size.height - dest_height) * (self.alignment.y + 1.0) / 2.0;

        Rect::from_xywh(dx, dy, dest_width, dest_height)
    }

    /// Check if the image should be flipped horizontally
    fn should_flip_horizontally(&self) -> bool {
        self.match_text_direction && self.text_direction == TextDirection::Rtl
    }

    /// Prepare paint with opacity, color blending, and color inversion
    fn prepare_paint(&self) -> Option<Paint> {
        if self.opacity >= 1.0
            && self.color.is_none()
            && !self.invert_colors
            && self.paint.is_none()
        {
            return None; // No paint modifications needed
        }

        let mut paint = self.paint.clone().unwrap_or_default();

        // Apply opacity
        if self.opacity < 1.0 {
            paint.color = paint.color.with_alpha((self.opacity * 255.0) as u8);
        }

        // Apply color blending (tinting)
        if let Some(blend_color) = self.color {
            paint.color = self.blend_colors(paint.color, blend_color);
        }

        // Note: Color inversion is typically done via ColorFilter in Flutter,
        // which would be applied at the Canvas level. For now, we mark it in paint.
        // A full implementation would use ctx.canvas().save_layer_with_filter()

        Some(paint)
    }

    /// Blend two colors based on the color blend mode
    fn blend_colors(&self, src: Color, blend: Color) -> Color {
        match self.color_blend_mode {
            ColorBlendMode::None => src,
            ColorBlendMode::Multiply => {
                // Multiply: (src * blend) / 255
                Color::rgba(
                    (src.r as u16 * blend.r as u16 / 255) as u8,
                    (src.g as u16 * blend.g as u16 / 255) as u8,
                    (src.b as u16 * blend.b as u16 / 255) as u8,
                    src.a,
                )
            }
            ColorBlendMode::Screen => {
                // Screen: 255 - ((255 - src) * (255 - blend) / 255)
                Color::rgba(
                    (255 - ((255 - src.r as u16) * (255 - blend.r as u16) / 255)) as u8,
                    (255 - ((255 - src.g as u16) * (255 - blend.g as u16) / 255)) as u8,
                    (255 - ((255 - src.b as u16) * (255 - blend.b as u16) / 255)) as u8,
                    src.a,
                )
            }
            ColorBlendMode::Modulate => {
                // Modulate is same as Multiply in most implementations
                Color::rgba(
                    (src.r as u16 * blend.r as u16 / 255) as u8,
                    (src.g as u16 * blend.g as u16 / 255) as u8,
                    (src.b as u16 * blend.b as u16 / 255) as u8,
                    (src.a as u16 * blend.a as u16 / 255) as u8,
                )
            }
            // Other blend modes would require more complex calculations
            // For now, default to Multiply for unsupported modes
            _ => self.blend_colors(src, blend),
        }
    }

    /// Draw image with tiling/repeat
    fn draw_repeated<T>(
        &self,
        ctx: &mut PaintContext<'_, T, Leaf>,
        dest_rect: Rect,
        paint: Option<&Paint>,
    ) where
        T: crate::core::PaintTree,
    {
        let image_width = self.image.width() as f32;
        let image_height = self.image.height() as f32;

        match self.repeat {
            ImageRepeat::Repeat => {
                // Tile in both directions
                let mut y = dest_rect.top();
                while y < dest_rect.bottom() {
                    let mut x = dest_rect.left();
                    while x < dest_rect.right() {
                        let tile_rect = Rect::from_xywh(x, y, image_width, image_height);
                        ctx.canvas()
                            .draw_image(self.image.clone(), tile_rect, paint);
                        x += image_width;
                    }
                    y += image_height;
                }
            }
            ImageRepeat::RepeatX => {
                // Tile only horizontally
                let mut x = dest_rect.left();
                while x < dest_rect.right() {
                    let tile_rect = Rect::from_xywh(x, dest_rect.top(), image_width, image_height);
                    ctx.canvas()
                        .draw_image(self.image.clone(), tile_rect, paint);
                    x += image_width;
                }
            }
            ImageRepeat::RepeatY => {
                // Tile only vertically
                let mut y = dest_rect.top();
                while y < dest_rect.bottom() {
                    let tile_rect = Rect::from_xywh(dest_rect.left(), y, image_width, image_height);
                    ctx.canvas()
                        .draw_image(self.image.clone(), tile_rect, paint);
                    y += image_height;
                }
            }
            ImageRepeat::NoRepeat => {
                // Single image, no tiling
                ctx.canvas()
                    .draw_image(self.image.clone(), dest_rect, paint);
            }
        }
    }

    /// Draw image with 9-patch slicing (center slice stretching)
    fn draw_nine_patch<T>(
        &self,
        ctx: &mut PaintContext<'_, T, Leaf>,
        dest_rect: Rect,
        center_slice: Rect,
        paint: Option<&Paint>,
    ) where
        T: crate::core::PaintTree,
    {
        let image_width = self.image.width() as f32;
        let image_height = self.image.height() as f32;

        // Source rectangles (in image coordinates)
        let src_left = 0.0;
        let src_top = 0.0;
        let src_right = image_width;
        let src_bottom = image_height;

        // Center slice boundaries in image coordinates
        let slice_left = center_slice.left();
        let slice_top = center_slice.top();
        let slice_right = center_slice.right();
        let slice_bottom = center_slice.bottom();

        // Destination boundaries
        let dst_left = dest_rect.left();
        let dst_top = dest_rect.top();
        let dst_right = dest_rect.right();
        let dst_bottom = dest_rect.bottom();

        // Calculate destination slice boundaries
        // Corners maintain original size, edges stretch in one dimension
        let dst_slice_left = dst_left + slice_left;
        let dst_slice_top = dst_top + slice_top;
        let dst_slice_right = dst_right - (image_width - slice_right);
        let dst_slice_bottom = dst_bottom - (image_height - slice_bottom);

        // Draw 9 patches:
        // Top-left corner
        self.draw_image_rect(
            ctx,
            Rect::from_ltrb(src_left, src_top, slice_left, slice_top),
            Rect::from_ltrb(dst_left, dst_top, dst_slice_left, dst_slice_top),
            paint,
        );

        // Top edge (stretch horizontally)
        self.draw_image_rect(
            ctx,
            Rect::from_ltrb(slice_left, src_top, slice_right, slice_top),
            Rect::from_ltrb(dst_slice_left, dst_top, dst_slice_right, dst_slice_top),
            paint,
        );

        // Top-right corner
        self.draw_image_rect(
            ctx,
            Rect::from_ltrb(slice_right, src_top, src_right, slice_top),
            Rect::from_ltrb(dst_slice_right, dst_top, dst_right, dst_slice_top),
            paint,
        );

        // Left edge (stretch vertically)
        self.draw_image_rect(
            ctx,
            Rect::from_ltrb(src_left, slice_top, slice_left, slice_bottom),
            Rect::from_ltrb(dst_left, dst_slice_top, dst_slice_left, dst_slice_bottom),
            paint,
        );

        // Center (stretch both directions)
        self.draw_image_rect(
            ctx,
            Rect::from_ltrb(slice_left, slice_top, slice_right, slice_bottom),
            Rect::from_ltrb(
                dst_slice_left,
                dst_slice_top,
                dst_slice_right,
                dst_slice_bottom,
            ),
            paint,
        );

        // Right edge (stretch vertically)
        self.draw_image_rect(
            ctx,
            Rect::from_ltrb(slice_right, slice_top, src_right, slice_bottom),
            Rect::from_ltrb(dst_slice_right, dst_slice_top, dst_right, dst_slice_bottom),
            paint,
        );

        // Bottom-left corner
        self.draw_image_rect(
            ctx,
            Rect::from_ltrb(src_left, slice_bottom, slice_left, src_bottom),
            Rect::from_ltrb(dst_left, dst_slice_bottom, dst_slice_left, dst_bottom),
            paint,
        );

        // Bottom edge (stretch horizontally)
        self.draw_image_rect(
            ctx,
            Rect::from_ltrb(slice_left, slice_bottom, slice_right, src_bottom),
            Rect::from_ltrb(
                dst_slice_left,
                dst_slice_bottom,
                dst_slice_right,
                dst_bottom,
            ),
            paint,
        );

        // Bottom-right corner
        self.draw_image_rect(
            ctx,
            Rect::from_ltrb(slice_right, slice_bottom, src_right, src_bottom),
            Rect::from_ltrb(dst_slice_right, dst_slice_bottom, dst_right, dst_bottom),
            paint,
        );
    }

    /// Helper to draw a portion of the image
    fn draw_image_rect<T>(
        &self,
        ctx: &mut PaintContext<'_, T, Leaf>,
        _src: Rect,
        dst: Rect,
        paint: Option<&Paint>,
    ) where
        T: crate::core::PaintTree,
    {
        // For now, we draw the full image into the destination rectangle
        // A full implementation would use draw_image_rect to draw only the source portion
        // This requires Canvas API support for source rectangles
        ctx.canvas().draw_image(self.image.clone(), dst, paint);
    }
}

impl Default for RenderImage {
    fn default() -> Self {
        Self::new(Image::default())
    }
}

impl<T: FullRenderTree> RenderBox<T, Leaf> for RenderImage {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = &ctx.constraints;

        // If we have explicit dimensions, use them
        let width = self.width.unwrap_or_else(|| self.intrinsic_width());
        let height = self.height.unwrap_or_else(|| self.intrinsic_height());

        // Check if constraints are tight
        let is_tight = constraints.min_width == constraints.max_width
            && constraints.min_height == constraints.max_height;

        if is_tight {
            Size::new(constraints.max_width, constraints.max_height)
        } else {
            // Use image's dimensions within constraints
            let final_width = width.clamp(constraints.min_width, constraints.max_width);
            let final_height = height.clamp(constraints.min_height, constraints.max_height);

            Size::new(final_width, final_height)
        }
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Leaf>)
    where
        T: crate::core::PaintTree,
    {
        if self.image.width() == 0 || self.image.height() == 0 {
            return;
        }

        // Calculate size from intrinsic or explicit dimensions
        let width = self.width.unwrap_or_else(|| self.intrinsic_width());
        let height = self.height.unwrap_or_else(|| self.intrinsic_height());
        let available_size = Size::new(width, height);

        // Get the destination rectangle based on fit and alignment
        let dest_rect = self.calculate_dest_rect(available_size);

        // Apply transformations if needed (flipping, inversion)
        let needs_transform = self.should_flip_horizontally() || self.invert_colors;

        // Prepare paint with opacity, color blending, and inversion
        let paint_opt = self.prepare_paint();

        if needs_transform {
            // Use chaining API with saved() for transforms
            if self.should_flip_horizontally() {
                let center_x = dest_rect.left() + dest_rect.width() / 2.0;
                ctx.canvas()
                    .saved()
                    .translated(center_x, 0.0)
                    .scaled_xy(-1.0, 1.0)
                    .translated(-center_x, 0.0);
            } else {
                // Just invert_colors, no flip
                ctx.canvas().saved();
            }
        }

        // Handle different rendering modes
        if let Some(center_slice) = self.center_slice {
            // 9-patch rendering with center slice
            self.draw_nine_patch(ctx, dest_rect, center_slice, paint_opt.as_ref());
        } else if self.repeat != ImageRepeat::NoRepeat {
            // Tiled rendering with repeat mode
            self.draw_repeated(ctx, dest_rect, paint_opt.as_ref());
        } else {
            // Standard single image draw
            ctx.canvas()
                .draw_image(self.image.clone(), dest_rect, paint_opt.as_ref());
        }

        if needs_transform {
            ctx.canvas().restored();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_image(width: u32, height: u32) -> Image {
        let data = vec![255u8; (width * height * 4) as usize];
        Image::from_rgba8(width, height, data)
    }

    #[test]
    fn test_render_image_new() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image.clone());

        assert_eq!(render_image.image.width(), 100);
        assert_eq!(render_image.image.height(), 100);
        assert_eq!(render_image.fit, ImageFit::Contain);
        assert_eq!(render_image.alignment, Alignment::CENTER);
        assert_eq!(render_image.repeat, ImageRepeat::NoRepeat);
        assert_eq!(render_image.scale, 1.0);
        assert_eq!(render_image.opacity, 1.0);
        assert!(!render_image.is_anti_alias);
        assert!(!render_image.invert_colors);
        assert!(!render_image.match_text_direction);
    }

    #[test]
    fn test_render_image_with_fit() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).with_fit(ImageFit::Cover);

        assert_eq!(render_image.fit, ImageFit::Cover);
    }

    #[test]
    fn test_render_image_with_alignment() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).with_alignment(Alignment::TOP_LEFT);

        assert_eq!(render_image.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_image_with_repeat() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).with_repeat(ImageRepeat::Repeat);

        assert_eq!(render_image.repeat, ImageRepeat::Repeat);
    }

    #[test]
    fn test_render_image_with_dimensions() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).with_dimensions(Some(200.0), Some(150.0));

        assert_eq!(render_image.width, Some(200.0));
        assert_eq!(render_image.height, Some(150.0));
    }

    #[test]
    fn test_render_image_with_scale() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).with_scale(2.0);

        assert_eq!(render_image.scale, 2.0);
        assert_eq!(render_image.intrinsic_width(), 50.0);
        assert_eq!(render_image.intrinsic_height(), 50.0);
    }

    #[test]
    fn test_render_image_with_opacity() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).with_opacity(0.5);

        assert_eq!(render_image.opacity, 0.5);
    }

    #[test]
    fn test_render_image_opacity_clamping() {
        let image = create_test_image(100, 100);

        let render_image = RenderImage::new(image.clone()).with_opacity(1.5);
        assert_eq!(render_image.opacity, 1.0);

        let render_image = RenderImage::new(image).with_opacity(-0.5);
        assert_eq!(render_image.opacity, 0.0);
    }

    #[test]
    fn test_render_image_with_color() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).with_color(Color::RED, ColorBlendMode::Multiply);

        assert_eq!(render_image.color, Some(Color::RED));
        assert_eq!(render_image.color_blend_mode, ColorBlendMode::Multiply);
    }

    #[test]
    fn test_render_image_anti_aliased() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).anti_aliased();

        assert!(render_image.is_anti_alias);
    }

    #[test]
    fn test_render_image_inverted() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).inverted();

        assert!(render_image.invert_colors);
    }

    #[test]
    fn test_render_image_match_direction() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).match_direction(TextDirection::Rtl);

        assert!(render_image.match_text_direction);
        assert_eq!(render_image.text_direction, TextDirection::Rtl);
        assert!(render_image.should_flip_horizontally());
    }

    #[test]
    fn test_render_image_with_filter_quality() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).with_filter_quality(FilterQuality::High);

        assert_eq!(render_image.filter_quality, FilterQuality::High);
    }

    #[test]
    fn test_calculate_dest_rect_fill() {
        let image = create_test_image(100, 50);
        let render_image = RenderImage::new(image).with_fit(ImageFit::Fill);

        let dest = render_image.calculate_dest_rect(Size::new(200.0, 100.0));

        assert_eq!(dest.width(), 200.0);
        assert_eq!(dest.height(), 100.0);
    }

    #[test]
    fn test_calculate_dest_rect_contain() {
        let image = create_test_image(100, 50);
        let render_image = RenderImage::new(image).with_fit(ImageFit::Contain);

        let dest = render_image.calculate_dest_rect(Size::new(200.0, 100.0));

        assert_eq!(dest.width(), 200.0);
        assert_eq!(dest.height(), 100.0);
    }

    #[test]
    fn test_calculate_dest_rect_none() {
        let image = create_test_image(100, 50);
        let render_image = RenderImage::new(image).with_fit(ImageFit::None);

        let dest = render_image.calculate_dest_rect(Size::new(200.0, 100.0));

        assert_eq!(dest.width(), 100.0);
        assert_eq!(dest.height(), 50.0);
    }

    #[test]
    fn test_calculate_dest_rect_fit_width() {
        let image = create_test_image(100, 50);
        let render_image = RenderImage::new(image).with_fit(ImageFit::FitWidth);

        let dest = render_image.calculate_dest_rect(Size::new(200.0, 200.0));

        assert_eq!(dest.width(), 200.0);
        assert_eq!(dest.height(), 100.0); // Maintains 2:1 aspect ratio
    }

    #[test]
    fn test_calculate_dest_rect_fit_height() {
        let image = create_test_image(100, 50);
        let render_image = RenderImage::new(image).with_fit(ImageFit::FitHeight);

        let dest = render_image.calculate_dest_rect(Size::new(200.0, 200.0));

        assert_eq!(dest.width(), 400.0); // Maintains 2:1 aspect ratio
        assert_eq!(dest.height(), 200.0);
    }

    #[test]
    fn test_image_fit_variants() {
        assert_ne!(ImageFit::Fill, ImageFit::Cover);
        assert_ne!(ImageFit::Cover, ImageFit::Contain);
        assert_ne!(ImageFit::Contain, ImageFit::None);
        assert_ne!(ImageFit::None, ImageFit::ScaleDown);
        assert_ne!(ImageFit::ScaleDown, ImageFit::FitWidth);
        assert_ne!(ImageFit::FitWidth, ImageFit::FitHeight);
    }

    #[test]
    fn test_image_repeat_variants() {
        assert_ne!(ImageRepeat::Repeat, ImageRepeat::RepeatX);
        assert_ne!(ImageRepeat::RepeatX, ImageRepeat::RepeatY);
        assert_ne!(ImageRepeat::RepeatY, ImageRepeat::NoRepeat);
    }

    #[test]
    fn test_filter_quality_default() {
        // FilterQuality is now unified from flui_types where default is Low
        assert_eq!(FilterQuality::default(), FilterQuality::Low);
    }

    #[test]
    fn test_color_blend_mode_default() {
        assert_eq!(ColorBlendMode::default(), ColorBlendMode::None);
    }

    #[test]
    fn test_render_image_setters() {
        let image = create_test_image(100, 100);
        let mut render_image = RenderImage::new(image);

        render_image.set_width(Some(200.0));
        assert_eq!(render_image.width, Some(200.0));

        render_image.set_height(Some(150.0));
        assert_eq!(render_image.height, Some(150.0));

        render_image.set_scale(2.0);
        assert_eq!(render_image.scale, 2.0);

        render_image.set_fit(ImageFit::Cover);
        assert_eq!(render_image.fit, ImageFit::Cover);

        render_image.set_alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(render_image.alignment, Alignment::BOTTOM_RIGHT);

        render_image.set_repeat(ImageRepeat::RepeatX);
        assert_eq!(render_image.repeat, ImageRepeat::RepeatX);

        render_image.set_opacity(0.5);
        assert_eq!(render_image.opacity, 0.5);

        render_image.set_is_anti_alias(true);
        assert!(render_image.is_anti_alias);

        render_image.set_invert_colors(true);
        assert!(render_image.invert_colors);

        render_image.set_filter_quality(FilterQuality::High);
        assert_eq!(render_image.filter_quality, FilterQuality::High);

        render_image.set_match_text_direction(true);
        assert!(render_image.match_text_direction);

        render_image.set_text_direction(TextDirection::Rtl);
        assert_eq!(render_image.text_direction, TextDirection::Rtl);

        render_image.set_color(Some(Color::BLUE));
        assert_eq!(render_image.color, Some(Color::BLUE));

        render_image.set_color_blend_mode(ColorBlendMode::Screen);
        assert_eq!(render_image.color_blend_mode, ColorBlendMode::Screen);
    }
}
