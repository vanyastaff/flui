//! RenderConstrainedOverflowBox - Overflow box with custom constraints
//!
//! Allows the child to overflow the parent's constraints by applying custom
//! min/max width and height constraints to the child. The parent sizes itself
//! according to its incoming constraints, ignoring the child's size.
//!
//! This is useful for:
//! - Creating fixed-size widgets regardless of parent constraints
//! - Allowing children to exceed parent boundaries with specific size limits
//! - Transforming constraints passed to children
//!
//! Flutter reference: <https://api.flutter.dev/flutter/rendering/RenderConstrainedOverflowBox-class.html>

use crate::core::{BoxProtocol, LayoutContext, PaintContext, RenderBox, Single};
use flui_types::{Alignment, BoxConstraints, Size};

/// A render object that imposes different constraints on its child than it gets from its parent,
/// possibly allowing the child to overflow the parent.
///
/// The box sizes itself based on the parent's constraints, ignoring the child's dimensions.
/// The child is laid out with custom constraints (minWidth, maxWidth, minHeight, maxHeight),
/// which may allow it to be larger or smaller than the parent.
///
/// # Layout Algorithm
///
/// 1. Determine parent size from incoming constraints (uses maxWidth/maxHeight)
/// 2. Create child constraints by overriding parent constraints with custom min/max values
/// 3. Layout child with these custom constraints
/// 4. Position child according to alignment
/// 5. Return parent size (child size is ignored for parent sizing)
///
/// # Use Cases
///
/// - **Fixed-size rendering**: Always render at 50x50 regardless of parent
/// - **Overflow scenarios**: Allow child to exceed parent boundaries
/// - **Constraint transformation**: Apply custom constraints different from parent
///
/// # Example
///
/// ```rust,ignore
/// // Create a box that's always 100x100, but child can be any size up to 200x200
/// let overflow = RenderConstrainedOverflowBox::new()
///     .with_min_width(0.0)
///     .with_max_width(200.0)
///     .with_min_height(0.0)
///     .with_max_height(200.0);
/// ```
///
/// **Note**: Consider wrapping in RenderClipRect to avoid confusing hit testing behavior.
#[derive(Debug)]
pub struct RenderConstrainedOverflowBox {
    /// Minimum width constraint for child (overrides parent if set)
    pub min_width: Option<f32>,
    /// Maximum width constraint for child (overrides parent if set)
    pub max_width: Option<f32>,
    /// Minimum height constraint for child (overrides parent if set)
    pub min_height: Option<f32>,
    /// Maximum height constraint for child (overrides parent if set)
    pub max_height: Option<f32>,
    /// How to align the child within the parent
    pub alignment: Alignment,
    /// Cached parent size for paint phase
    cached_parent_size: Size,
    /// Cached child size for paint phase
    cached_child_size: Size,
}

impl RenderConstrainedOverflowBox {
    /// Create new constrained overflow box with default alignment (center)
    pub fn new() -> Self {
        Self {
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            alignment: Alignment::CENTER,
            cached_parent_size: Size::ZERO,
            cached_child_size: Size::ZERO,
        }
    }

    /// Set minimum width constraint for child
    pub fn with_min_width(mut self, min_width: f32) -> Self {
        self.min_width = Some(min_width);
        self
    }

    /// Set maximum width constraint for child
    pub fn with_max_width(mut self, max_width: f32) -> Self {
        self.max_width = Some(max_width);
        self
    }

    /// Set minimum height constraint for child
    pub fn with_min_height(mut self, min_height: f32) -> Self {
        self.min_height = Some(min_height);
        self
    }

    /// Set maximum height constraint for child
    pub fn with_max_height(mut self, max_height: f32) -> Self {
        self.max_height = Some(max_height);
        self
    }

    /// Set alignment for child positioning
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Set all constraints at once
    pub fn with_constraints(
        mut self,
        min_width: Option<f32>,
        max_width: Option<f32>,
        min_height: Option<f32>,
        max_height: Option<f32>,
    ) -> Self {
        self.min_width = min_width;
        self.max_width = max_width;
        self.min_height = min_height;
        self.max_height = max_height;
        self
    }

    /// Create child constraints by overriding parent constraints with custom values
    fn create_child_constraints(&self, parent_constraints: BoxConstraints) -> BoxConstraints {
        BoxConstraints::new(
            self.min_width.unwrap_or(parent_constraints.min_width),
            self.max_width.unwrap_or(parent_constraints.max_width),
            self.min_height.unwrap_or(parent_constraints.min_height),
            self.max_height.unwrap_or(parent_constraints.max_height),
        )
    }
}

impl Default for RenderConstrainedOverflowBox {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderBox<Single> for RenderConstrainedOverflowBox {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();

        // Parent sizes itself according to its incoming constraints
        // (uses biggest size available from parent)
        let parent_size = ctx.constraints.biggest();

        // Create custom constraints for child
        let child_constraints = self.create_child_constraints(ctx.constraints);

        // Layout child with custom constraints (may overflow parent)
        let child_size = ctx.layout_child(child_id, child_constraints);
        self.cached_child_size = child_size;
        self.cached_parent_size = parent_size;

        // Return parent size (ignoring child size)
        parent_size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();

        // Calculate child offset based on alignment
        // Note: child may be larger than parent (overflow)
        let child_offset = self
            .alignment
            .calculate_offset(self.cached_child_size, self.cached_parent_size);

        // Paint child at aligned offset (may paint outside parent bounds)
        ctx.paint_child(child_id, ctx.offset + child_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let overflow_box = RenderConstrainedOverflowBox::new();
        assert!(overflow_box.min_width.is_none());
        assert!(overflow_box.max_width.is_none());
        assert!(overflow_box.min_height.is_none());
        assert!(overflow_box.max_height.is_none());
        assert_eq!(overflow_box.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_builder_methods() {
        let overflow_box = RenderConstrainedOverflowBox::new()
            .with_min_width(10.0)
            .with_max_width(200.0)
            .with_min_height(20.0)
            .with_max_height(150.0)
            .with_alignment(Alignment::TOP_LEFT);

        assert_eq!(overflow_box.min_width, Some(10.0));
        assert_eq!(overflow_box.max_width, Some(200.0));
        assert_eq!(overflow_box.min_height, Some(20.0));
        assert_eq!(overflow_box.max_height, Some(150.0));
        assert_eq!(overflow_box.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_with_constraints() {
        let overflow_box = RenderConstrainedOverflowBox::new().with_constraints(
            Some(10.0),
            Some(100.0),
            Some(20.0),
            Some(80.0),
        );

        assert_eq!(overflow_box.min_width, Some(10.0));
        assert_eq!(overflow_box.max_width, Some(100.0));
        assert_eq!(overflow_box.min_height, Some(20.0));
        assert_eq!(overflow_box.max_height, Some(80.0));
    }

    #[test]
    fn test_create_child_constraints_no_override() {
        let overflow_box = RenderConstrainedOverflowBox::new();
        let parent = BoxConstraints::new(50.0, 200.0, 30.0, 150.0);

        let child = overflow_box.create_child_constraints(parent);

        // Should use parent constraints when no overrides
        assert_eq!(child.min_width, 50.0);
        assert_eq!(child.max_width, 200.0);
        assert_eq!(child.min_height, 30.0);
        assert_eq!(child.max_height, 150.0);
    }

    #[test]
    fn test_create_child_constraints_with_overrides() {
        let overflow_box = RenderConstrainedOverflowBox::new()
            .with_min_width(0.0)
            .with_max_width(300.0)
            .with_min_height(0.0)
            .with_max_height(250.0);

        let parent = BoxConstraints::new(50.0, 200.0, 30.0, 150.0);
        let child = overflow_box.create_child_constraints(parent);

        // Should use custom constraints
        assert_eq!(child.min_width, 0.0);
        assert_eq!(child.max_width, 300.0);
        assert_eq!(child.min_height, 0.0);
        assert_eq!(child.max_height, 250.0);
    }

    #[test]
    fn test_create_child_constraints_partial_override() {
        let overflow_box = RenderConstrainedOverflowBox::new()
            .with_max_width(300.0) // Only override max width
            .with_min_height(0.0); // Only override min height

        let parent = BoxConstraints::new(50.0, 200.0, 30.0, 150.0);
        let child = overflow_box.create_child_constraints(parent);

        // Mix of parent and custom constraints
        assert_eq!(child.min_width, 50.0); // From parent
        assert_eq!(child.max_width, 300.0); // Overridden
        assert_eq!(child.min_height, 0.0); // Overridden
        assert_eq!(child.max_height, 150.0); // From parent
    }

    #[test]
    fn test_default() {
        let overflow_box = RenderConstrainedOverflowBox::default();
        assert!(overflow_box.min_width.is_none());
        assert!(overflow_box.max_width.is_none());
        assert_eq!(overflow_box.alignment, Alignment::CENTER);
    }
}
