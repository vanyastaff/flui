//! RenderFittedBox - scales and positions its child within itself.
//!
//! This render object scales its child to fit within itself according to a BoxFit mode.

use flui_types::{BoxConstraints, Matrix4, Offset, Point, Rect, Size};

use crate::containers::ProxyBox;
use crate::pipeline::PaintingContext;
use crate::traits::TextBaseline;

/// How a box should be inscribed into another box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoxFit {
    /// Fill the target box by distorting the aspect ratio.
    Fill,
    /// As large as possible while still containing the source within the target.
    #[default]
    Contain,
    /// As small as possible while still covering the entire target box.
    Cover,
    /// Make sure the full width of the source is shown.
    FitWidth,
    /// Make sure the full height of the source is shown.
    FitHeight,
    /// Align the source within the target and don't scale.
    None,
    /// Align the source within the target and scale down if necessary.
    ScaleDown,
}

impl BoxFit {
    /// Computes the fitted size for the given source and destination.
    pub fn apply_to(&self, source: Size, destination: Size) -> FittedSizes {
        let source_aspect = source.width / source.height;
        let dest_aspect = destination.width / destination.height;

        let (width, height) = match self {
            BoxFit::Fill => (destination.width, destination.height),
            BoxFit::Contain => {
                if dest_aspect > source_aspect {
                    // Destination is wider, fit to height
                    (destination.height * source_aspect, destination.height)
                } else {
                    // Destination is taller, fit to width
                    (destination.width, destination.width / source_aspect)
                }
            }
            BoxFit::Cover => {
                if dest_aspect > source_aspect {
                    // Destination is wider, fit to width
                    (destination.width, destination.width / source_aspect)
                } else {
                    // Destination is taller, fit to height
                    (destination.height * source_aspect, destination.height)
                }
            }
            BoxFit::FitWidth => (destination.width, destination.width / source_aspect),
            BoxFit::FitHeight => (destination.height * source_aspect, destination.height),
            BoxFit::None => (source.width, source.height),
            BoxFit::ScaleDown => {
                let contained = BoxFit::Contain.apply_to(source, destination);
                if contained.destination.width < source.width
                    || contained.destination.height < source.height
                {
                    return contained;
                } else {
                    (source.width, source.height)
                }
            }
        };

        FittedSizes {
            source,
            destination: Size::new(width, height),
        }
    }
}

/// The result of applying a BoxFit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FittedSizes {
    /// The original source size.
    pub source: Size,
    /// The resulting destination size.
    pub destination: Size,
}

impl FittedSizes {
    /// Returns the scale factors.
    pub fn scale(&self) -> (f32, f32) {
        (
            self.destination.width / self.source.width,
            self.destination.height / self.source.height,
        )
    }
}

/// Alignment within a container (for fitted box positioning).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FittedAlignment {
    /// X alignment (-1.0 = start, 0.0 = center, 1.0 = end)
    pub x: f32,
    /// Y alignment (-1.0 = start, 0.0 = center, 1.0 = end)
    pub y: f32,
}

impl FittedAlignment {
    /// Center alignment.
    pub const CENTER: Self = Self { x: 0.0, y: 0.0 };
    /// Top-left alignment.
    pub const TOP_LEFT: Self = Self { x: -1.0, y: -1.0 };
    /// Top-center alignment.
    pub const TOP_CENTER: Self = Self { x: 0.0, y: -1.0 };
    /// Top-right alignment.
    pub const TOP_RIGHT: Self = Self { x: 1.0, y: -1.0 };
    /// Center-left alignment.
    pub const CENTER_LEFT: Self = Self { x: -1.0, y: 0.0 };
    /// Center-right alignment.
    pub const CENTER_RIGHT: Self = Self { x: 1.0, y: 0.0 };
    /// Bottom-left alignment.
    pub const BOTTOM_LEFT: Self = Self { x: -1.0, y: 1.0 };
    /// Bottom-center alignment.
    pub const BOTTOM_CENTER: Self = Self { x: 0.0, y: 1.0 };
    /// Bottom-right alignment.
    pub const BOTTOM_RIGHT: Self = Self { x: 1.0, y: 1.0 };

    /// Computes the offset within a container.
    pub fn along_offset(&self, container: Size, child: Size) -> Offset {
        let dx = (container.width - child.width) * (self.x + 1.0) / 2.0;
        let dy = (container.height - child.height) * (self.y + 1.0) / 2.0;
        Offset::new(dx, dy)
    }
}

impl Default for FittedAlignment {
    fn default() -> Self {
        Self::CENTER
    }
}

/// A render object that scales its child to fit.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::effects::{RenderFittedBox, BoxFit};
///
/// // Scale child to fit, maintaining aspect ratio
/// let fitted = RenderFittedBox::new(BoxFit::Contain);
///
/// // Cover the entire area
/// let cover = RenderFittedBox::new(BoxFit::Cover);
/// ```
#[derive(Debug)]
pub struct RenderFittedBox {
    /// Container holding the child and geometry.
    proxy: ProxyBox,

    /// How to inscribe the child in the parent.
    fit: BoxFit,

    /// Alignment within the parent.
    alignment: FittedAlignment,

    /// Cached child size (would come from actual child in real implementation).
    child_size: Size,
}

impl RenderFittedBox {
    /// Creates a new fitted box.
    pub fn new(fit: BoxFit) -> Self {
        Self {
            proxy: ProxyBox::new(),
            fit,
            alignment: FittedAlignment::CENTER,
            child_size: Size::ZERO,
        }
    }

    /// Creates with fit and alignment.
    pub fn with_alignment(fit: BoxFit, alignment: FittedAlignment) -> Self {
        Self {
            proxy: ProxyBox::new(),
            fit,
            alignment,
            child_size: Size::ZERO,
        }
    }

    /// Returns the fit mode.
    pub fn fit(&self) -> BoxFit {
        self.fit
    }

    /// Sets the fit mode.
    pub fn set_fit(&mut self, fit: BoxFit) {
        if self.fit != fit {
            self.fit = fit;
        }
    }

    /// Returns the alignment.
    pub fn alignment(&self) -> FittedAlignment {
        self.alignment
    }

    /// Sets the alignment.
    pub fn set_alignment(&mut self, alignment: FittedAlignment) {
        if self.alignment != alignment {
            self.alignment = alignment;
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        *self.proxy.geometry()
    }

    /// Computes the transform for painting the child.
    pub fn compute_transform(&self) -> (Matrix4, Offset) {
        let parent_size = self.size();
        let child_size = self.child_size;

        if child_size.width == 0.0 || child_size.height == 0.0 || parent_size == Size::ZERO {
            return (Matrix4::IDENTITY, Offset::ZERO);
        }

        let sizes = self.fit.apply_to(child_size, parent_size);
        let (scale_x, scale_y) = sizes.scale();

        let scaled_size = sizes.destination;
        let offset = self.alignment.along_offset(parent_size, scaled_size);

        let transform = Matrix4::scaling(scale_x, scale_y, 1.0);

        (transform, offset)
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let size = constraints.smallest();
        self.proxy.set_geometry(size);
        size
    }

    /// Performs layout with a child size.
    pub fn perform_layout_with_child(
        &mut self,
        constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        self.child_size = child_size;
        // FittedBox takes whatever size it's given
        let size = constraints.biggest();
        self.proxy.set_geometry(size);
        size
    }

    /// Returns constraints for the child (unconstrained).
    pub fn constraints_for_child(&self, _constraints: BoxConstraints) -> BoxConstraints {
        // Child gets unconstrained layout
        BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY)
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.child_size == Size::ZERO {
            return;
        }

        let (transform, child_offset) = self.compute_transform();
        let paint_offset = Offset::new(offset.dx + child_offset.dx, offset.dy + child_offset.dy);

        // In real implementation:
        // context.push_transform(paint_offset, transform, |ctx| {
        //     ctx.paint_child(child, Offset::ZERO);
        // });
        let _ = (context, paint_offset, transform);
    }

    /// Hit test with transformation applied.
    pub fn hit_test(&self, position: Offset) -> bool {
        let (transform, child_offset) = self.compute_transform();

        // Transform position to child coordinates
        let local = Offset::new(position.dx - child_offset.dx, position.dy - child_offset.dy);

        // Inverse transform
        let inverse = transform.try_inverse().unwrap_or(Matrix4::IDENTITY);
        let (tx, ty) = inverse.transform_point(local.dx, local.dy);

        // Check if in child bounds
        let child_rect = Rect::from_origin_size(Point::ZERO, self.child_size);
        child_rect.contains(Point::new(tx, ty))
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, _height: f32, _child_width: Option<f32>) -> f32 {
        0.0
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, _height: f32, _child_width: Option<f32>) -> f32 {
        0.0
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, _width: f32, _child_height: Option<f32>) -> f32 {
        0.0
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, _width: f32, _child_height: Option<f32>) -> f32 {
        0.0
    }

    /// Computes distance to baseline.
    pub fn compute_distance_to_baseline(
        &self,
        _baseline: TextBaseline,
        _child_baseline: Option<f32>,
    ) -> Option<f32> {
        None
    }
}

impl Default for RenderFittedBox {
    fn default() -> Self {
        Self::new(BoxFit::Contain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_fit_contain() {
        let source = Size::new(100.0, 50.0);
        let dest = Size::new(200.0, 200.0);

        let result = BoxFit::Contain.apply_to(source, dest);

        // Should scale to fit, maintaining aspect ratio
        assert!((result.destination.width - 200.0).abs() < f32::EPSILON);
        assert!((result.destination.height - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_box_fit_cover() {
        let source = Size::new(100.0, 50.0);
        let dest = Size::new(200.0, 200.0);

        let result = BoxFit::Cover.apply_to(source, dest);

        // Should scale to cover, maintaining aspect ratio
        assert!((result.destination.height - 200.0).abs() < f32::EPSILON);
        assert!((result.destination.width - 400.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_box_fit_fill() {
        let source = Size::new(100.0, 50.0);
        let dest = Size::new(200.0, 200.0);

        let result = BoxFit::Fill.apply_to(source, dest);

        // Should match destination exactly
        assert_eq!(result.destination, dest);
    }

    #[test]
    fn test_box_fit_none() {
        let source = Size::new(100.0, 50.0);
        let dest = Size::new(200.0, 200.0);

        let result = BoxFit::None.apply_to(source, dest);

        // Should keep source size
        assert_eq!(result.destination, source);
    }

    #[test]
    fn test_fitted_alignment_center() {
        let container = Size::new(200.0, 200.0);
        let child = Size::new(100.0, 100.0);

        let offset = FittedAlignment::CENTER.along_offset(container, child);

        assert!((offset.dx - 50.0).abs() < f32::EPSILON);
        assert!((offset.dy - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_fitted_alignment_top_left() {
        let container = Size::new(200.0, 200.0);
        let child = Size::new(100.0, 100.0);

        let offset = FittedAlignment::TOP_LEFT.along_offset(container, child);

        assert!((offset.dx - 0.0).abs() < f32::EPSILON);
        assert!((offset.dy - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_fitted_box_layout() {
        let mut fitted = RenderFittedBox::new(BoxFit::Contain);
        let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
        let child_size = Size::new(100.0, 50.0);

        let size = fitted.perform_layout_with_child(constraints, child_size);

        assert_eq!(size, Size::new(200.0, 200.0));
    }

    #[test]
    fn test_fitted_box_child_unconstrained() {
        let fitted = RenderFittedBox::new(BoxFit::Contain);
        let parent_constraints = BoxConstraints::tight(Size::new(200.0, 200.0));

        let child_constraints = fitted.constraints_for_child(parent_constraints);

        assert_eq!(child_constraints.min_width, 0.0);
        assert_eq!(child_constraints.max_width, f32::INFINITY);
    }
}
