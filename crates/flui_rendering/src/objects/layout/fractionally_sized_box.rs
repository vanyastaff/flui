//! RenderFractionallySizedBox - sizes child as fraction of parent
//!
//! Flutter equivalent: `RenderFractionallySizedOverflowBox`
//! Source: https://api.flutter.dev/flutter/rendering/RenderFractionallySizedOverflowBox-class.html

use crate::core::{
    FullRenderTree,
    FullRenderTree, RenderBox, Single, {BoxProtocol, LayoutContext, PaintContext},
};
use flui_types::{Alignment, Size};

/// RenderObject that sizes child as a fraction of available space
///
/// This is useful for making a child take up a percentage of its parent.
/// For example, width_factor: 0.5 makes the child half the parent's width.
///
/// # Layout Algorithm (Flutter-compatible)
///
/// 1. If factor is set: child gets tight constraint = parent_max * factor
/// 2. If factor is None: constraints pass through unchanged
/// 3. Child may overflow if factor > 1.0 (this is allowed!)
/// 4. This box sizes itself to child, then aligns child if parent constrains smaller
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderFractionallySizedBox;
///
/// // 50% width and height
/// let fractional = RenderFractionallySizedBox::new(Some(0.5), Some(0.5));
///
/// // 150% width (child will overflow)
/// let overflow = RenderFractionallySizedBox::new(Some(1.5), None);
/// ```
#[derive(Debug)]
pub struct RenderFractionallySizedBox {
    /// Width factor (typically 0.0 - 1.0, but can be > 1.0 for overflow)
    /// None means constraints pass through unchanged
    pub width_factor: Option<f32>,
    /// Height factor (typically 0.0 - 1.0, but can be > 1.0 for overflow)
    /// None means constraints pass through unchanged
    pub height_factor: Option<f32>,
    /// Alignment for positioning child when sizes differ
    pub alignment: Alignment,

    /// Cached child size for paint phase
    child_size: Size,
}

impl RenderFractionallySizedBox {
    /// Create new RenderFractionallySizedBox
    ///
    /// Note: Flutter allows factors > 1.0 for overflow scenarios
    pub fn new(width_factor: Option<f32>, height_factor: Option<f32>) -> Self {
        if let Some(w) = width_factor {
            assert!(w >= 0.0, "Width factor must be non-negative");
        }
        if let Some(h) = height_factor {
            assert!(h >= 0.0, "Height factor must be non-negative");
        }
        Self {
            width_factor,
            height_factor,
            alignment: Alignment::CENTER,
            child_size: Size::ZERO,
        }
    }

    /// Create with alignment
    pub fn with_alignment(
        width_factor: Option<f32>,
        height_factor: Option<f32>,
        alignment: Alignment,
    ) -> Self {
        let mut this = Self::new(width_factor, height_factor);
        this.alignment = alignment;
        this
    }

    /// Create with both width and height factors
    pub fn both(factor: f32) -> Self {
        Self::new(Some(factor), Some(factor))
    }

    /// Create with only width factor
    pub fn width(factor: f32) -> Self {
        Self::new(Some(factor), None)
    }

    /// Create with only height factor
    pub fn height(factor: f32) -> Self {
        Self::new(None, Some(factor))
    }

    /// Set new width factor
    pub fn set_width_factor(&mut self, factor: Option<f32>) {
        if let Some(w) = factor {
            assert!(w >= 0.0, "Width factor must be non-negative");
        }
        self.width_factor = factor;
    }

    /// Set new height factor
    pub fn set_height_factor(&mut self, factor: Option<f32>) {
        if let Some(h) = factor {
            assert!(h >= 0.0, "Height factor must be non-negative");
        }
        self.height_factor = factor;
    }

    /// Set alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }
}

impl Default for RenderFractionallySizedBox {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl<T: FullRenderTree> RenderBox<T, Single> for RenderFractionallySizedBox {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;

        // Calculate child constraints based on factors
        // Flutter: if factor is set, impose tight constraint = max * factor
        // If factor is None, pass through constraint unchanged
        let child_min_width = self
            .width_factor
            .map(|f| constraints.max_width * f)
            .unwrap_or(constraints.min_width);
        let child_max_width = self
            .width_factor
            .map(|f| constraints.max_width * f)
            .unwrap_or(constraints.max_width);
        let child_min_height = self
            .height_factor
            .map(|f| constraints.max_height * f)
            .unwrap_or(constraints.min_height);
        let child_max_height = self
            .height_factor
            .map(|f| constraints.max_height * f)
            .unwrap_or(constraints.max_height);

        let child_constraints = flui_types::constraints::BoxConstraints::new(
            child_min_width,
            child_max_width,
            child_min_height,
            child_max_height,
        );

        // Layout child with calculated constraints
        let child_size = ctx.layout_child(child_id, child_constraints);
        self.child_size = child_size;

        // Try to size ourselves to child, but constrain to parent
        constraints.constrain(child_size)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();

        // If child overflows, align it within our bounds
        // This uses the same alignment calculation as RenderAlign
        let child_offset = self
            .alignment
            .calculate_offset(self.child_size, self.child_size);

        ctx.paint_child(child_id, ctx.offset + child_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_fractionally_sized_box_new() {
        let fractional = RenderFractionallySizedBox::new(Some(0.5), Some(0.75));
        assert_eq!(fractional.width_factor, Some(0.5));
        assert_eq!(fractional.height_factor, Some(0.75));
        assert_eq!(fractional.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_fractionally_sized_box_both() {
        let fractional = RenderFractionallySizedBox::both(0.5);
        assert_eq!(fractional.width_factor, Some(0.5));
        assert_eq!(fractional.height_factor, Some(0.5));
    }

    #[test]
    fn test_render_fractionally_sized_box_width() {
        let fractional = RenderFractionallySizedBox::width(0.5);
        assert_eq!(fractional.width_factor, Some(0.5));
        assert_eq!(fractional.height_factor, None);
    }

    #[test]
    fn test_render_fractionally_sized_box_height() {
        let fractional = RenderFractionallySizedBox::height(0.75);
        assert_eq!(fractional.width_factor, None);
        assert_eq!(fractional.height_factor, Some(0.75));
    }

    #[test]
    fn test_render_fractionally_sized_box_overflow_allowed() {
        // Flutter allows factors > 1.0 for overflow scenarios
        let fractional = RenderFractionallySizedBox::new(Some(1.5), Some(2.0));
        assert_eq!(fractional.width_factor, Some(1.5));
        assert_eq!(fractional.height_factor, Some(2.0));
    }

    #[test]
    #[should_panic(expected = "Width factor must be non-negative")]
    fn test_render_fractionally_sized_box_invalid_width() {
        RenderFractionallySizedBox::new(Some(-0.5), None);
    }

    #[test]
    #[should_panic(expected = "Height factor must be non-negative")]
    fn test_render_fractionally_sized_box_invalid_height() {
        RenderFractionallySizedBox::new(None, Some(-0.1));
    }

    #[test]
    fn test_render_fractionally_sized_box_set_factors() {
        let mut fractional = RenderFractionallySizedBox::both(0.5);
        fractional.set_width_factor(Some(0.75));
        assert_eq!(fractional.width_factor, Some(0.75));
    }

    #[test]
    fn test_render_fractionally_sized_box_with_alignment() {
        let fractional =
            RenderFractionallySizedBox::with_alignment(Some(0.5), Some(0.5), Alignment::TOP_LEFT);
        assert_eq!(fractional.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_fractionally_sized_box_set_alignment() {
        let mut fractional = RenderFractionallySizedBox::both(0.5);
        fractional.set_alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(fractional.alignment, Alignment::BOTTOM_RIGHT);
    }
}
