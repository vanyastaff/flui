//! Image painting implementation
//!
//! Provides painting functionality for images with fit, repeat, alignment,
//! and color filtering support.

use flui_engine::{Paint, Painter};
use flui_types::{
    layout::Alignment,
    painting::{BoxFit, ColorFilter, Image, ImageRepeat},
    styling::{Color, DecorationImage},
    Rect,
};

/// Painter for images
///
/// Handles painting images with various fitting, repeating, and filtering options.
pub struct ImagePainter;

impl ImagePainter {
    /// Paint an image with full decoration support
    ///
    /// # Arguments
    ///
    /// * `painter` - The backend-agnostic painter to draw with
    /// * `rect` - The rectangle to paint the image into
    /// * `decoration_image` - The decoration image configuration
    pub fn paint(painter: &mut dyn Painter, rect: Rect, decoration_image: &DecorationImage) {
        let image = &decoration_image.image;
        let fit = decoration_image.fit.unwrap_or(BoxFit::Contain);
        let alignment = decoration_image.alignment;
        let repeat = decoration_image.repeat;
        let opacity = decoration_image.opacity;
        let color_filter = decoration_image.color_filter;

        // Paint based on repeat mode
        match repeat {
            ImageRepeat::NoRepeat => {
                Self::paint_single(painter, rect, image, fit, alignment, opacity, color_filter);
            }
            ImageRepeat::Repeat => {
                Self::paint_repeated(painter, rect, image, fit, opacity, color_filter, true, true);
            }
            ImageRepeat::RepeatX => {
                Self::paint_repeated(
                    painter,
                    rect,
                    image,
                    fit,
                    opacity,
                    color_filter,
                    true,
                    false,
                );
            }
            ImageRepeat::RepeatY => {
                Self::paint_repeated(
                    painter,
                    rect,
                    image,
                    fit,
                    opacity,
                    color_filter,
                    false,
                    true,
                );
            }
        }
    }

    /// Paint a single image (no repeat)
    fn paint_single(
        painter: &mut dyn Painter,
        rect: Rect,
        image: &Image,
        fit: BoxFit,
        alignment: Alignment,
        opacity: f32,
        _color_filter: Option<ColorFilter>,
    ) {
        let image_size = image.size();
        let fitted = fit.apply(image_size, rect.size());

        // Calculate the destination rectangle based on alignment
        let dest_rect = Self::align_rect(fitted.destination, rect, alignment);

        // Create paint with opacity
        let paint = Paint::builder()
            .color(Color::BLACK.with_opacity(opacity))
            .build();

        // TODO: Apply color_filter - need to convert from painting::ColorFilter
        // to effects::ColorFilter for apply_image_filter

        // For now, use the entire image as source
        // TODO: Handle fitted.source size to crop the source image if needed
        let src_rect: Option<Rect> = None;

        // Draw the image (stubbed API - only accepts image name and position)
        // TODO: Full implementation will use src_rect, dest_rect, and paint
        let image_name = format!("Image({:?})", image);
        painter.draw_image(&image_name, dest_rect.top_left());
    }

    /// Paint a repeated image (tiled)
    fn paint_repeated(
        painter: &mut dyn Painter,
        rect: Rect,
        image: &Image,
        fit: BoxFit,
        opacity: f32,
        _color_filter: Option<ColorFilter>,
        repeat_x: bool,
        repeat_y: bool,
    ) {
        let image_size = image.size();
        let fitted = fit.apply(image_size, rect.size());
        let tile_size = fitted.destination;

        // Calculate how many times to repeat
        let repeat_count_x = if repeat_x {
            (rect.width() / tile_size.width).ceil() as i32
        } else {
            1
        };

        let repeat_count_y = if repeat_y {
            (rect.height() / tile_size.height).ceil() as i32
        } else {
            1
        };

        // Draw each tile
        for y in 0..repeat_count_y {
            for x in 0..repeat_count_x {
                let tile_rect = Rect::from_xywh(
                    rect.left() + x as f32 * tile_size.width,
                    rect.top() + y as f32 * tile_size.height,
                    tile_size.width,
                    tile_size.height,
                );

                // Only draw if the tile intersects with the target rect
                if tile_rect.intersects(&rect) {
                    // Clip to target rect
                    let clipped_rect = tile_rect.intersection(&rect).unwrap_or(tile_rect);

                    Self::paint_single(
                        painter,
                        clipped_rect,
                        image,
                        BoxFit::Fill, // Fill each tile
                        Alignment::CENTER,
                        opacity,
                        None, // TODO: pass color_filter once type conversion is implemented
                    );
                }
            }
        }
    }

    /// Align a rectangle within a container based on alignment
    fn align_rect(size: flui_types::Size, container: Rect, alignment: Alignment) -> Rect {
        let x = container.left() + (container.width() - size.width) * alignment.x;
        let y = container.top() + (container.height() - size.height) * alignment.y;

        Rect::from_xywh(x, y, size.width, size.height)
    }

    /// Simple helper to paint an image with basic settings
    ///
    /// This is a convenience method for common use cases.
    pub fn paint_simple(painter: &mut dyn Painter, rect: Rect, image: &Image, fit: BoxFit) {
        let decoration_image = DecorationImage::new(image.clone()).with_fit(fit);

        Self::paint(painter, rect, &decoration_image);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::Size;

    #[test]
    fn test_image_painter_exists() {
        // Basic smoke test
        let _painter = ImagePainter;
    }

    #[test]
    fn test_align_rect_center() {
        let size = Size::new(100.0, 100.0);
        let container = Rect::from_xywh(0.0, 0.0, 200.0, 200.0);
        let aligned = ImagePainter::align_rect(size, container, Alignment::CENTER);

        assert_eq!(aligned.left(), 50.0);
        assert_eq!(aligned.top(), 50.0);
        assert_eq!(aligned.width(), 100.0);
        assert_eq!(aligned.height(), 100.0);
    }

    #[test]
    fn test_align_rect_top_left() {
        let size = Size::new(100.0, 100.0);
        let container = Rect::from_xywh(0.0, 0.0, 200.0, 200.0);
        let aligned = ImagePainter::align_rect(size, container, Alignment::TOP_LEFT);

        assert_eq!(aligned.left(), 0.0);
        assert_eq!(aligned.top(), 0.0);
    }

    #[test]
    fn test_align_rect_bottom_right() {
        let size = Size::new(100.0, 100.0);
        let container = Rect::from_xywh(0.0, 0.0, 200.0, 200.0);
        let aligned = ImagePainter::align_rect(size, container, Alignment::BOTTOM_RIGHT);

        assert_eq!(aligned.left(), 100.0);
        assert_eq!(aligned.top(), 100.0);
    }
}
