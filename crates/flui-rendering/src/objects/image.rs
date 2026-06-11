//! RenderImage — renders bitmap images with aspect preservation and alignment.
//!
//! Implements the RenderImage protocol object following Flutter's image.dart (22-404).
//! Supports aspect-ratio preservation, fit modes (Fill/Contain/Cover/ScaleDown/None),
//! and alignment.

use flui_foundation::Diagnosticable;
use flui_tree::Leaf;
use flui_types::{Offset, Point, Pixels, Rect, Size};

use crate::{
    constraints::BoxConstraints,
    context::BoxLayoutContext,
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

/// How to inscribe an image into a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFit {
    /// Fill the entire box, distorting the image if necessary.
    Fill,
    /// Contain the image within the box, maintaining aspect ratio.
    /// Image may be smaller than the box.
    Contain,
    /// Cover the entire box, maintaining aspect ratio.
    /// Image may be cropped.
    Cover,
    /// Contain the image and scale to fit, but only shrink (never enlarge).
    ScaleDown,
    /// Do not scale the image; show at natural size.
    None,
}

/// How to align an image within a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageAlignment {
    /// Align to the top-left corner.
    TopLeft,
    /// Align to the top-center edge.
    Top,
    /// Align to the top-right corner.
    TopRight,
    /// Align to the left-center edge.
    Left,
    /// Align to the center.
    Center,
    /// Align to the right-center edge.
    Right,
    /// Align to the bottom-left corner.
    BottomLeft,
    /// Align to the bottom-center edge.
    Bottom,
    /// Align to the bottom-right corner.
    BottomRight,
}

impl ImageAlignment {
    /// Calculates the offset for the given image and container size.
    #[allow(dead_code)]
    fn offset(&self, image_size: Size, container_size: Size) -> Offset {
        let x = match self {
            Self::TopLeft | Self::Left | Self::BottomLeft => Pixels::ZERO,
            Self::Top | Self::Center | Self::Bottom => {
                (container_size.width - image_size.width) * 0.5
            }
            Self::TopRight | Self::Right | Self::BottomRight => {
                container_size.width - image_size.width
            }
        };

        let y = match self {
            Self::TopLeft | Self::Top | Self::TopRight => Pixels::ZERO,
            Self::Left | Self::Center | Self::Right => {
                (container_size.height - image_size.height) * 0.5
            }
            Self::BottomLeft | Self::Bottom | Self::BottomRight => {
                container_size.height - image_size.height
            }
        };

        Offset::new(x, y)
    }
}

/// Render object for displaying images.
///
/// RenderImage displays a bitmap image within a rectangular area with support for:
/// - Aspect-ratio preservation via `ImageFit` mode
/// - Alignment within the containing box
/// - Intrinsic size queries based on image dimensions
#[derive(Debug, Clone)]
pub struct RenderImage {
    /// Natural (intrinsic) size of the image.
    intrinsic_size: Size,
    /// How to fit the image into available space.
    fit: ImageFit,
    /// How to align the image within the box.
    alignment: ImageAlignment,
    /// Cached layout size (set by perform_layout).
    size: Size,
}

impl RenderImage {
    /// Creates a new RenderImage with the given intrinsic size.
    pub fn new(intrinsic_size: Size, fit: ImageFit, alignment: ImageAlignment) -> Self {
        Self {
            intrinsic_size,
            fit,
            alignment,
            size: Size::ZERO,
        }
    }

    /// Sets the intrinsic (natural) size of the image.
    pub fn set_intrinsic_size(&mut self, size: Size) {
        self.intrinsic_size = size;
        // Caller responsible for marking layout dirty
    }

    /// Sets the fit mode for the image.
    pub fn set_fit(&mut self, fit: ImageFit) {
        self.fit = fit;
        // Caller responsible for marking layout dirty
    }

    /// Sets the alignment of the image within the box.
    pub fn set_alignment(&mut self, alignment: ImageAlignment) {
        self.alignment = alignment;
        // Caller responsible for marking repaint dirty
    }

    /// Computes the size of the image given the fit mode and constraints.
    fn compute_size(&self, constraints: &BoxConstraints) -> Size {
        match self.fit {
            ImageFit::Fill => {
                // Fill the entire box
                Size::new(
                    constraints.constrain_width(self.intrinsic_size.width),
                    constraints.constrain_height(self.intrinsic_size.height),
                )
            }
            ImageFit::Contain => {
                // Fit entirely within the box, preserving aspect ratio
                constraints.constrain_size_and_attempt_to_preserve_aspect_ratio(
                    self.intrinsic_size,
                )
            }
            ImageFit::Cover => {
                // Cover the entire box, preserving aspect ratio
                // (image may be cropped)
                let constrained = constraints.constrain_size_and_attempt_to_preserve_aspect_ratio(
                    self.intrinsic_size,
                );
                // Ensure at least minimum dimensions are met
                Size::new(
                    constrained.width.max(constraints.min_width),
                    constrained.height.max(constraints.min_height),
                )
            }
            ImageFit::ScaleDown => {
                // Like Contain, but never enlarge
                let unconstrained = BoxConstraints {
                    min_width: Pixels::ZERO,
                    max_width: self.intrinsic_size.width,
                    min_height: Pixels::ZERO,
                    max_height: self.intrinsic_size.height,
                };
                let size = unconstrained.constrain_size_and_attempt_to_preserve_aspect_ratio(
                    self.intrinsic_size,
                );
                constraints.constrain(size)
            }
            ImageFit::None => {
                // Show at natural size, clamped to constraints
                constraints.constrain(self.intrinsic_size)
            }
        }
    }
}

impl Diagnosticable for RenderImage {}

impl RenderBox for RenderImage {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
        let size = self.compute_size(&ctx.constraints());
        self.size = size;
        ctx.complete_with_size(size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

impl PaintEffectsCapability for RenderImage {}
impl SemanticsCapability for RenderImage {}
impl HotReloadCapability for RenderImage {}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_render_image_creation() {
        let intrinsic = Size::new(px(100.0), px(200.0));
        let image = RenderImage::new(intrinsic, ImageFit::Contain, ImageAlignment::Center);

        assert_eq!(image.intrinsic_size, intrinsic);
        assert_eq!(image.fit, ImageFit::Contain);
        assert_eq!(image.alignment, ImageAlignment::Center);
        assert_eq!(image.size(), &Size::ZERO);
    }

    #[test]
    fn test_image_fit_contain_shrinks_width() {
        let image = RenderImage::new(
            Size::new(px(200.0), px(100.0)),
            ImageFit::Contain,
            ImageAlignment::Center,
        );
        let constraints = BoxConstraints {
            min_width: Pixels::ZERO,
            max_width: px(100.0),
            min_height: Pixels::ZERO,
            max_height: px(100.0),
        };

        let computed = image.compute_size(&constraints);
        // Original 2:1 aspect ratio (200x100)
        // Max 100x100 → should shrink to 100x50 (maintains ratio, fits in height)
        assert!(computed.width <= constraints.max_width);
        assert!(computed.height <= constraints.max_height);
        // Check aspect ratio preserved: width/height = 200/100 = 2/1
        let expected_height = computed.width * 0.5; // height = width / 2
        assert!((computed.height.get() - expected_height.get()).abs() < 0.01);
    }

    #[test]
    fn test_image_fit_fill_stretches() {
        let image = RenderImage::new(
            Size::new(px(100.0), px(100.0)),
            ImageFit::Fill,
            ImageAlignment::Center,
        );
        let constraints = BoxConstraints {
            min_width: px(200.0),
            max_width: px(200.0),
            min_height: px(150.0),
            max_height: px(150.0),
        };

        let computed = image.compute_size(&constraints);
        assert_eq!(computed.width, px(200.0));
        assert_eq!(computed.height, px(150.0));
    }

    #[test]
    fn test_image_fit_none_constrains() {
        let image = RenderImage::new(
            Size::new(px(50.0), px(50.0)),
            ImageFit::None,
            ImageAlignment::Center,
        );
        let constraints = BoxConstraints {
            min_width: Pixels::ZERO,
            max_width: px(100.0),
            min_height: Pixels::ZERO,
            max_height: px(100.0),
        };

        let computed = image.compute_size(&constraints);
        // None fit: show at natural size (50x50), which fits in constraints
        assert_eq!(computed.width, px(50.0));
        assert_eq!(computed.height, px(50.0));
    }

    #[test]
    fn test_image_alignment_center_offset() {
        let alignment = ImageAlignment::Center;
        let image_size = Size::new(px(50.0), px(50.0));
        let container_size = Size::new(px(100.0), px(100.0));

        let offset = alignment.offset(image_size, container_size);
        // Center alignment should place image at (25, 25) in container
        assert_eq!(offset.dx, px(25.0));
        assert_eq!(offset.dy, px(25.0));
    }

    #[test]
    fn test_image_alignment_top_left() {
        let alignment = ImageAlignment::TopLeft;
        let image_size = Size::new(px(50.0), px(50.0));
        let container_size = Size::new(px(100.0), px(100.0));

        let offset = alignment.offset(image_size, container_size);
        assert_eq!(offset.dx, Pixels::ZERO);
        assert_eq!(offset.dy, Pixels::ZERO);
    }
}

