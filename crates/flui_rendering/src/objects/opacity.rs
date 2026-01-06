//! RenderOpacity - applies transparency to a single child.

use flui_types::{Offset, Point, Rect, Size};

use crate::arity::Single;
use crate::context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use crate::parent_data::BoxParentData;
use crate::traits::RenderBox;

/// A render object that applies transparency to its child.
///
/// The opacity value ranges from 0.0 (fully transparent) to 1.0 (fully opaque).
/// When opacity is 0.0, the child is completely invisible but still takes up space
/// and can receive hit tests.
///
/// # Performance
///
/// Opacity creates a compositing layer, which has some performance cost.
/// For animating opacity, consider using `RenderAnimatedOpacity` which can
/// optimize the case where opacity changes frequently.
///
/// # Example
///
/// ```ignore
/// let opacity = RenderOpacity::new(0.5); // 50% transparent
/// let mut wrapper = BoxWrapper::new(opacity);
/// // Add a child, then layout with constraints
/// ```
#[derive(Debug, Clone)]
pub struct RenderOpacity {
    /// Opacity value (0.0 = transparent, 1.0 = opaque).
    opacity: f32,
    /// Alpha as u8 (0-255) for efficient layer operations.
    alpha: u8,
    /// Size after layout.
    size: Size,
    /// Whether we have a child.
    has_child: bool,
    /// Whether opacity is always needed for compositing.
    /// When true, we always create an opacity layer.
    /// When false, we skip the layer when opacity is 1.0.
    always_needs_compositing: bool,
}

impl RenderOpacity {
    /// Creates a new opacity render object with the given opacity.
    ///
    /// # Arguments
    ///
    /// * `opacity` - Opacity value (0.0 = transparent, 1.0 = opaque).
    ///               Values outside [0.0, 1.0] are clamped.
    pub fn new(opacity: f32) -> Self {
        let clamped = opacity.clamp(0.0, 1.0);
        Self {
            opacity: clamped,
            alpha: Self::opacity_to_alpha(clamped),
            size: Size::ZERO,
            has_child: false,
            always_needs_compositing: false,
        }
    }

    /// Creates a fully opaque render object (opacity = 1.0).
    pub fn opaque() -> Self {
        Self::new(1.0)
    }

    /// Creates a fully transparent render object (opacity = 0.0).
    pub fn transparent() -> Self {
        Self::new(0.0)
    }

    /// Returns the current opacity value.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Sets the opacity value.
    ///
    /// # Arguments
    ///
    /// * `opacity` - Opacity value (0.0 = transparent, 1.0 = opaque).
    pub fn set_opacity(&mut self, opacity: f32) {
        let clamped = opacity.clamp(0.0, 1.0);
        if (self.opacity - clamped).abs() > f32::EPSILON {
            self.opacity = clamped;
            self.alpha = Self::opacity_to_alpha(clamped);
            // In full implementation, would mark needs paint
        }
    }

    /// Returns the alpha value (0-255).
    pub fn alpha(&self) -> u8 {
        self.alpha
    }

    /// Sets whether this render object always needs compositing.
    ///
    /// When true, an opacity layer is always created even when opacity is 1.0.
    /// This can be useful for animations where you want consistent compositing behavior.
    pub fn set_always_needs_compositing(&mut self, value: bool) {
        self.always_needs_compositing = value;
    }

    /// Returns whether compositing is needed.
    ///
    /// Returns true if opacity is not 1.0 or if always_needs_compositing is set.
    pub fn needs_compositing(&self) -> bool {
        self.always_needs_compositing || self.alpha != 255
    }

    /// Converts opacity (0.0-1.0) to alpha (0-255).
    fn opacity_to_alpha(opacity: f32) -> u8 {
        (opacity * 255.0).round() as u8
    }
}

impl Default for RenderOpacity {
    fn default() -> Self {
        Self::opaque()
    }
}

impl flui_foundation::Diagnosticable for RenderOpacity {}
impl RenderBox for RenderOpacity {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        let constraints = ctx.constraints().clone();

        if ctx.child_count() > 0 {
            self.has_child = true;

            // Layout child with same constraints
            let child_size = ctx.layout_child(0, constraints);
            self.size = child_size;

            ctx.complete_with_size(self.size);
        } else {
            self.has_child = false;
            // No child - take minimum size
            self.size = constraints.smallest();
            ctx.complete_with_size(self.size);
        }
    }

    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    fn paint(&mut self, ctx: &mut BoxPaintContext<'_, Single, BoxParentData>) {
        // RenderOpacity doesn't paint anything itself.
        // The opacity is applied by paint_node_recursive which checks paint_alpha().
        // Children are painted automatically by the wrapper after this method.
        let _ = ctx;
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Invisible elements can still receive hit tests
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }

        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }

    fn paint_alpha(&self) -> Option<u8> {
        // If fully opaque and not always needing compositing, no layer needed
        if self.alpha == 255 && !self.always_needs_compositing {
            None
        } else {
            Some(self.alpha)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opacity_new() {
        let opacity = RenderOpacity::new(0.5);
        assert!((opacity.opacity() - 0.5).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 128); // 0.5 * 255 ≈ 128
    }

    #[test]
    fn test_opacity_clamp() {
        let opacity = RenderOpacity::new(1.5);
        assert!((opacity.opacity() - 1.0).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 255);

        let opacity = RenderOpacity::new(-0.5);
        assert!((opacity.opacity() - 0.0).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 0);
    }

    #[test]
    fn test_opacity_opaque() {
        let opacity = RenderOpacity::opaque();
        assert!((opacity.opacity() - 1.0).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 255);
        assert!(!opacity.needs_compositing());
    }

    #[test]
    fn test_opacity_transparent() {
        let opacity = RenderOpacity::transparent();
        assert!((opacity.opacity() - 0.0).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 0);
        assert!(opacity.needs_compositing());
    }

    #[test]
    fn test_opacity_set() {
        let mut opacity = RenderOpacity::new(1.0);
        opacity.set_opacity(0.25);
        assert!((opacity.opacity() - 0.25).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 64); // 0.25 * 255 ≈ 64
    }

    #[test]
    fn test_needs_compositing() {
        let mut opacity = RenderOpacity::new(1.0);
        assert!(!opacity.needs_compositing());

        opacity.set_opacity(0.5);
        assert!(opacity.needs_compositing());

        opacity.set_opacity(1.0);
        opacity.set_always_needs_compositing(true);
        assert!(opacity.needs_compositing());
    }

    #[test]
    fn test_default() {
        let opacity = RenderOpacity::default();
        assert!((opacity.opacity() - 1.0).abs() < f32::EPSILON);
    }
}
