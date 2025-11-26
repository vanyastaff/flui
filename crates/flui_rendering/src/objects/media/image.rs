//! RenderImage - Displays a raster image

use crate::core::{BoxProtocol, LayoutContext, Leaf, PaintContext, RenderBox};
use flui_painting::Paint;
use flui_types::{painting::Image, Rect, Size};

/// How an image should be inscribed into the allocated space
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ImageFit {
    /// Fill the entire space, distorting aspect ratio if needed
    Fill,
    /// Maintain aspect ratio, may leave empty space
    #[default]
    Contain,
    /// Maintain aspect ratio, may clip to fill space
    Cover,
    /// Use image's intrinsic size, no scaling
    None,
    /// Scale down to fit if too large, otherwise use intrinsic size
    ScaleDown,
}

/// RenderObject that displays a raster image
///
/// Renders an image using Canvas::draw_image. The image is scaled
/// according to the fit mode and aligned within the constraints.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderImage;
/// use flui_types::painting::Image;
///
/// let image = Image::from_rgba8(100, 100, vec![255; 100 * 100 * 4]);
/// let render_image = RenderImage::new(image);
/// ```
#[derive(Debug)]
pub struct RenderImage {
    /// The image to display
    pub image: Image,

    /// How the image should fit in the available space
    pub fit: ImageFit,

    /// Alignment within the space (0.0-1.0)
    /// (0.0, 0.0) = top-left, (0.5, 0.5) = center, (1.0, 1.0) = bottom-right
    pub alignment: (f32, f32),

    /// Optional paint for tinting or opacity
    pub paint: Option<Paint>,
}

impl RenderImage {
    /// Creates a new RenderImage with default settings
    pub fn new(image: Image) -> Self {
        Self {
            image,
            fit: ImageFit::default(),
            alignment: (0.5, 0.5), // Center by default
            paint: None,
        }
    }

    /// Sets the fit mode
    pub fn with_fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Sets the alignment
    pub fn with_alignment(mut self, x: f32, y: f32) -> Self {
        self.alignment = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
        self
    }

    /// Sets the paint (for tinting or opacity)
    pub fn with_paint(mut self, paint: Paint) -> Self {
        self.paint = Some(paint);
        self
    }

    /// Calculates the destination rectangle for the image
    fn calculate_dest_rect(&self, available_size: Size) -> Rect {
        let image_width = self.image.width() as f32;
        let image_height = self.image.height() as f32;

        if image_width == 0.0 || image_height == 0.0 {
            return Rect::from_xywh(0.0, 0.0, 0.0, 0.0);
        }

        let image_aspect = image_width / image_height;
        let space_aspect = available_size.width / available_size.height;

        let (dest_width, dest_height) = match self.fit {
            ImageFit::Fill => {
                // Fill entire space, distort if needed
                (available_size.width, available_size.height)
            }
            ImageFit::Contain => {
                // Fit inside, maintain aspect ratio
                if space_aspect > image_aspect {
                    // Space is wider than image
                    let width = available_size.height * image_aspect;
                    (width, available_size.height)
                } else {
                    // Space is taller than image
                    let height = available_size.width / image_aspect;
                    (available_size.width, height)
                }
            }
            ImageFit::Cover => {
                // Cover entire space, may clip
                if space_aspect > image_aspect {
                    // Space is wider, fit to width
                    let height = available_size.width / image_aspect;
                    (available_size.width, height)
                } else {
                    // Space is taller, fit to height
                    let width = available_size.height * image_aspect;
                    (width, available_size.height)
                }
            }
            ImageFit::None => {
                // Use intrinsic size
                (image_width, image_height)
            }
            ImageFit::ScaleDown => {
                // Scale down if too large, otherwise use intrinsic
                if image_width <= available_size.width && image_height <= available_size.height {
                    (image_width, image_height)
                } else {
                    // Scale down to fit
                    if space_aspect > image_aspect {
                        let width = available_size.height * image_aspect;
                        (width, available_size.height)
                    } else {
                        let height = available_size.width / image_aspect;
                        (available_size.width, height)
                    }
                }
            }
        };

        // Calculate position based on alignment
        let x = (available_size.width - dest_width) * self.alignment.0;
        let y = (available_size.height - dest_height) * self.alignment.1;

        Rect::from_xywh(x, y, dest_width, dest_height)
    }
}

impl RenderBox<Leaf> for RenderImage {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = &ctx.constraints;

        // If we have specific size constraints, use them
        let is_tight = constraints.min_width == constraints.max_width
            && constraints.min_height == constraints.max_height;

        if is_tight {
            Size::new(constraints.max_width, constraints.max_height)
        } else {
            // Otherwise, use image's intrinsic size within constraints
            let image_width = self.image.width() as f32;
            let image_height = self.image.height() as f32;

            let width = image_width.clamp(constraints.min_width, constraints.max_width);
            let height = image_height.clamp(constraints.min_height, constraints.max_height);

            Size::new(width, height)
        }
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Leaf>)
    where
        T: crate::core::PaintTree,
    {
        // Get the destination rectangle based on fit and alignment
        let dest_rect = self.calculate_dest_rect(Size::new(
            self.image.width() as f32,
            self.image.height() as f32,
        ));

        // Draw the image into the context's canvas
        let paint_opt = self.paint.as_ref();
        ctx.canvas()
            .draw_image(self.image.clone(), dest_rect, paint_opt);
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
        assert_eq!(render_image.alignment, (0.5, 0.5));
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
        let render_image = RenderImage::new(image).with_alignment(1.0, 0.0);

        assert_eq!(render_image.alignment, (1.0, 0.0));
    }

    #[test]
    fn test_render_image_alignment_clamping() {
        let image = create_test_image(100, 100);
        let render_image = RenderImage::new(image).with_alignment(2.0, -1.0);

        assert_eq!(render_image.alignment, (1.0, 0.0));
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

        // Space is wider than image aspect (2:1 vs 2:1)
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
}
