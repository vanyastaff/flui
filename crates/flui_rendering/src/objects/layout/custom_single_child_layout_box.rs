//! RenderCustomSingleChildLayoutBox - Custom single-child layout with delegate
//!
//! This RenderObject delegates all layout decisions to a `SingleChildLayoutDelegate`,
//! allowing complete custom control over:
//! - Parent size calculation
//! - Child constraints
//! - Child positioning
//!
//! Similar to Flutter's RenderCustomSingleChildLayoutBox.

use crate::core::{BoxProtocol, LayoutContext, PaintContext, RenderBox, Single};
use flui_types::{BoxConstraints, Offset, Size};
use std::any::Any;
use std::fmt::Debug;

/// Delegate trait for custom single-child layout logic
///
/// This trait provides complete control over the layout process for a single child.
/// The delegate determines:
/// 1. The size of the parent container (via `get_size`)
/// 2. The constraints to apply to the child (via `get_constraints_for_child`)
/// 3. Where to position the child within the parent (via `get_position_for_child`)
///
/// # Layout Flow
///
/// 1. `get_size(constraints)` → parent_size
/// 2. `get_constraints_for_child(constraints)` → child_constraints
/// 3. Layout child with child_constraints → child_size
/// 4. `get_position_for_child(parent_size, child_size)` → child_offset
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleChildLayoutDelegate, RenderCustomSingleChildLayoutBox};
/// use flui_types::{BoxConstraints, Offset, Size};
///
/// #[derive(Debug)]
/// struct CenteredDelegate;
///
/// impl SingleChildLayoutDelegate for CenteredDelegate {
///     fn get_size(&self, constraints: BoxConstraints) -> Size {
///         constraints.biggest()  // Fill available space
///     }
///
///     fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
///         constraints.loosen()  // Child can be any size
///     }
///
///     fn get_position_for_child(&self, size: Size, child_size: Size) -> Offset {
///         // Center the child
///         Offset::new(
///             (size.width - child_size.width) / 2.0,
///             (size.height - child_size.height) / 2.0,
///         )
///     }
///
///     fn should_relayout(&self, _old: &dyn Any) -> bool {
///         false  // Never needs relayout
///     }
///
///     fn as_any(&self) -> &dyn Any {
///         self
///     }
/// }
/// ```
pub trait SingleChildLayoutDelegate: Debug + Send + Sync {
    /// Calculate the size of the parent container
    ///
    /// Called first during layout to determine the parent's size.
    /// The size must be within the given constraints.
    ///
    /// **IMPORTANT:** The size cannot depend on the child's size.
    /// This is called before the child is laid out.
    ///
    /// # Arguments
    /// * `constraints` - The constraints from the parent
    ///
    /// # Returns
    /// The size of this container (must satisfy constraints)
    fn get_size(&self, constraints: BoxConstraints) -> Size;

    /// Calculate constraints for the child
    ///
    /// Called after `get_size` to determine what constraints to apply to the child.
    ///
    /// # Arguments
    /// * `constraints` - The original constraints from the parent
    ///
    /// # Returns
    /// BoxConstraints to apply to the child
    fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints;

    /// Calculate the position of the child within the parent
    ///
    /// Called after the child has been laid out to determine where to position it.
    ///
    /// # Arguments
    /// * `size` - The size of the parent (from `get_size`)
    /// * `child_size` - The size of the child (after layout)
    ///
    /// # Returns
    /// Offset where the child should be positioned
    fn get_position_for_child(&self, size: Size, child_size: Size) -> Offset;

    /// Check if layout should be recomputed
    ///
    /// Return true if the delegate's state has changed in a way that requires relayout.
    /// This is used to optimize when a new delegate is provided.
    ///
    /// # Arguments
    /// * `old` - The previous delegate (type-erased)
    ///
    /// # Returns
    /// true if relayout is needed, false otherwise
    fn should_relayout(&self, old: &dyn Any) -> bool;

    /// For Any trait (downcasting)
    fn as_any(&self) -> &dyn Any;
}

/// RenderObject that delegates layout to a SingleChildLayoutDelegate
///
/// This render object doesn't implement any layout logic itself.
/// Instead, it calls methods on the provided delegate to:
/// - Determine its own size
/// - Calculate constraints for its child
/// - Position the child
///
/// This provides maximum flexibility for custom layouts that don't fit
/// standard patterns like Padding, Align, etc.
///
/// # Example Use Cases
///
/// - Custom alignment logic that depends on complex rules
/// - Layouts that need to size themselves independently of their child
/// - Positioning logic that varies based on parent size
/// - Responsive layouts with custom breakpoints
///
/// # Performance Note
///
/// The delegate is called during every layout pass, so keep computations
/// efficient. For simple cases, prefer specialized RenderObjects like
/// RenderAlign, RenderPadding, etc.
#[derive(Debug)]
pub struct RenderCustomSingleChildLayoutBox {
    /// The layout delegate
    delegate: Box<dyn SingleChildLayoutDelegate>,
    /// Cached parent size (from delegate)
    cached_size: Size,
    /// Cached child offset (from delegate)
    cached_child_offset: Offset,
}

impl RenderCustomSingleChildLayoutBox {
    /// Create new custom single child layout with given delegate
    ///
    /// # Arguments
    /// * `delegate` - The layout delegate that controls sizing and positioning
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let render = RenderCustomSingleChildLayoutBox::new(Box::new(CenteredDelegate));
    /// ```
    pub fn new(delegate: Box<dyn SingleChildLayoutDelegate>) -> Self {
        Self {
            delegate,
            cached_size: Size::ZERO,
            cached_child_offset: Offset::ZERO,
        }
    }

    /// Get a reference to the delegate
    pub fn delegate(&self) -> &dyn SingleChildLayoutDelegate {
        &*self.delegate
    }

    /// Set a new delegate
    ///
    /// If the new delegate's `should_relayout` method returns true,
    /// the render object will be marked for relayout.
    pub fn set_delegate(&mut self, new_delegate: Box<dyn SingleChildLayoutDelegate>) {
        let needs_relayout = new_delegate.should_relayout(self.delegate.as_any());
        self.delegate = new_delegate;

        if needs_relayout {
            // In a real implementation, would mark for relayout here
            // For now, the next layout() call will use the new delegate
        }
    }
}

impl RenderBox<Single> for RenderCustomSingleChildLayoutBox {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();

        // 1. Get parent size from delegate (cannot depend on child size)
        let parent_size = self.delegate.get_size(ctx.constraints);
        self.cached_size = parent_size;

        // 2. Get child constraints from delegate
        let child_constraints = self.delegate.get_constraints_for_child(ctx.constraints);

        // 3. Layout child with those constraints
        let child_size = ctx.layout_child(child_id, child_constraints);

        // 4. Get child position from delegate
        let child_offset = self
            .delegate
            .get_position_for_child(parent_size, child_size);
        self.cached_child_offset = child_offset;

        // Return parent size
        parent_size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();

        // Paint child at the offset calculated during layout
        let child_offset = ctx.offset + self.cached_child_offset;
        ctx.paint_child(child_id, child_offset);
    }
}

// ============================================================================
// Example Delegates
// ============================================================================

/// Simple delegate that centers the child
///
/// This is a basic example showing how to implement SingleChildLayoutDelegate.
/// For production use, prefer RenderAlign which is more optimized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CenterDelegate;

impl SingleChildLayoutDelegate for CenterDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        // Fill all available space
        constraints.biggest()
    }

    fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        // Child can be any size up to parent constraints
        constraints.loosen()
    }

    fn get_position_for_child(&self, size: Size, child_size: Size) -> Offset {
        // Center the child within the parent
        Offset::new(
            (size.width - child_size.width) / 2.0,
            (size.height - child_size.height) / 2.0,
        )
    }

    fn should_relayout(&self, old: &dyn Any) -> bool {
        // Check if old delegate is also CenterDelegate
        // If so, no relayout needed (delegates are equivalent)
        !old.is::<CenterDelegate>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Delegate that forces child to take fixed size and positions it at top-left
///
/// This demonstrates a more complex delegate with custom behavior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixedSizeDelegate {
    /// The fixed width for the child
    pub width: f32,
    /// The fixed height for the child
    pub height: f32,
}

impl FixedSizeDelegate {
    /// Create new fixed size delegate
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

impl SingleChildLayoutDelegate for FixedSizeDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        // Parent size matches child size
        constraints.constrain(Size::new(self.width, self.height))
    }

    fn get_constraints_for_child(&self, _constraints: BoxConstraints) -> BoxConstraints {
        // Child must be exactly the specified size
        BoxConstraints::tight(Size::new(self.width, self.height))
    }

    fn get_position_for_child(&self, _size: Size, _child_size: Size) -> Offset {
        // Child at top-left (0, 0)
        Offset::ZERO
    }

    fn should_relayout(&self, old: &dyn Any) -> bool {
        // Relayout if old delegate is not FixedSizeDelegate or has different size
        if let Some(old_delegate) = old.downcast_ref::<FixedSizeDelegate>() {
            old_delegate.width != self.width || old_delegate.height != self.height
        } else {
            true
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_center_delegate_get_size() {
        let delegate = CenterDelegate;
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let size = delegate.get_size(constraints);

        // Should return biggest size
        assert_eq!(size, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_center_delegate_get_constraints_for_child() {
        let delegate = CenterDelegate;
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let child_constraints = delegate.get_constraints_for_child(constraints);

        // Should loosen constraints (min = 0)
        assert_eq!(child_constraints.min_width, 0.0);
        assert_eq!(child_constraints.min_height, 0.0);
        assert_eq!(child_constraints.max_width, 100.0);
        assert_eq!(child_constraints.max_height, 100.0);
    }

    #[test]
    fn test_center_delegate_get_position_for_child() {
        let delegate = CenterDelegate;
        let parent_size = Size::new(100.0, 100.0);
        let child_size = Size::new(50.0, 30.0);

        let offset = delegate.get_position_for_child(parent_size, child_size);

        // Should center child
        assert_eq!(offset.dx, 25.0); // (100 - 50) / 2
        assert_eq!(offset.dy, 35.0); // (100 - 30) / 2
    }

    #[test]
    fn test_center_delegate_should_relayout() {
        let delegate1 = CenterDelegate;
        let delegate2 = CenterDelegate;

        // Same type - no relayout needed
        assert!(!delegate1.should_relayout(delegate2.as_any()));

        // Different type - relayout needed
        let other_delegate = FixedSizeDelegate::new(50.0, 50.0);
        assert!(delegate1.should_relayout(other_delegate.as_any()));
    }

    #[test]
    fn test_fixed_size_delegate() {
        let delegate = FixedSizeDelegate::new(50.0, 30.0);

        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        // Parent size should match child size
        let size = delegate.get_size(constraints);
        assert_eq!(size, Size::new(50.0, 30.0));

        // Child constraints should be tight
        let child_constraints = delegate.get_constraints_for_child(constraints);
        assert_eq!(child_constraints.min_width, 50.0);
        assert_eq!(child_constraints.max_width, 50.0);
        assert_eq!(child_constraints.min_height, 30.0);
        assert_eq!(child_constraints.max_height, 30.0);

        // Child should be at top-left
        let offset = delegate.get_position_for_child(size, Size::new(50.0, 30.0));
        assert_eq!(offset, Offset::ZERO);
    }

    #[test]
    fn test_fixed_size_delegate_should_relayout() {
        let delegate1 = FixedSizeDelegate::new(50.0, 30.0);
        let delegate2 = FixedSizeDelegate::new(50.0, 30.0);
        let delegate3 = FixedSizeDelegate::new(60.0, 30.0);

        // Same size - no relayout
        assert!(!delegate1.should_relayout(delegate2.as_any()));

        // Different size - relayout needed
        assert!(delegate1.should_relayout(delegate3.as_any()));

        // Different type - relayout needed
        assert!(delegate1.should_relayout(CenterDelegate.as_any()));
    }

    #[test]
    fn test_render_custom_single_child_layout_box_new() {
        let delegate = Box::new(CenterDelegate);
        let render = RenderCustomSingleChildLayoutBox::new(delegate);

        assert_eq!(render.cached_size, Size::ZERO);
        assert_eq!(render.cached_child_offset, Offset::ZERO);
    }

    #[test]
    fn test_render_custom_single_child_layout_box_set_delegate() {
        let delegate1 = Box::new(CenterDelegate);
        let mut render = RenderCustomSingleChildLayoutBox::new(delegate1);

        // Set new delegate
        let delegate2 = Box::new(FixedSizeDelegate::new(100.0, 100.0));
        render.set_delegate(delegate2);

        // Verify delegate was changed (check size calculation)
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = render.delegate().get_size(constraints);
        assert_eq!(size, Size::new(100.0, 100.0));
    }
}
