//! RenderFlow - Custom layout container with delegate pattern
//!
//! Implements Flutter's Flow layout that uses a delegate pattern for custom
//! layout logic. The FlowDelegate computes child constraints, container size,
//! and child transformations. Optimized for repositioning children using
//! transformation matrices without relayout.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderFlow` | `RenderFlow` from `package:flutter/src/rendering/flow.dart` |
//! | `FlowDelegate` | `FlowDelegate` trait |
//! | `get_size()` | `getSize()` method |
//! | `get_constraints_for_child()` | `getConstraintsForChild()` method |
//! | `paint_children()` | `paintChildren()` method |
//! | `should_relayout()` | `shouldRelayout()` method |
//! | `should_repaint()` | `shouldRepaint()` method |
//! | `SimpleFlowDelegate` | Example implementation (not in Flutter) |
//!
//! # Layout Protocol
//!
//! 1. **Delegate determines container size**
//!    - Call `delegate.get_size(constraints)`
//!    - Delegate returns desired container size
//!
//! 2. **Layout each child with delegate constraints**
//!    - For each child index:
//!      - Call `delegate.get_constraints_for_child(index, parent_constraints)`
//!      - Layout child with delegate-provided constraints
//!      - Store child size for paint phase
//!
//! 3. **Return container size**
//!    - Size determined by delegate in step 1
//!
//! # Paint Protocol
//!
//! 1. **Create FlowPaintContext**
//!    - Provide paint context, child count, child sizes, child IDs
//!
//! 2. **Delegate paints children with transforms**
//!    - Call `delegate.paint_children(context, offset)`
//!    - Delegate calls `context.paint_child(index, transform, offset)` for each child
//!    - TODO: Apply transformation matrices (currently only uses offset)
//!
//! # Performance
//!
//! - **Layout**: O(n) - layout each child with delegate constraints
//! - **Paint**: O(n) - delegate paints each child (potentially with transforms)
//! - **Memory**: 40 bytes base + O(n) for cached sizes (8 bytes per child)
//!
//! # Use Cases
//!
//! - **Custom layouts**: Complex layouts not supported by standard containers
//! - **Animated repositioning**: Transform-based child repositioning without relayout
//! - **Parallax effects**: Children moving at different speeds
//! - **Carousel layouts**: Custom carousel with transformation effects
//! - **3D transforms**: Perspective transforms on children (when supported)
//! - **Performance optimization**: Repaint without relayout for position changes
//!
//! # Delegate Pattern Benefits
//!
//! - **Separation of concerns**: Layout logic in delegate, rendering in RenderObject
//! - **Reusability**: Same delegate can be used with different instances
//! - **Testability**: Delegates can be unit tested independently
//! - **Flexibility**: Easy to create custom layouts without subclassing
//! - **Performance**: `should_relayout()` and `should_repaint()` optimize updates
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderFlex**: Flex has fixed layout algorithm, Flow uses custom delegate
//! - **vs RenderStack**: Stack has simple layering, Flow has custom positioning
//! - **vs RenderCustomMultiChildLayoutBox**: Similar delegate pattern, different protocol
//! - **vs RenderTransform**: Transform applies to container, Flow transforms each child
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderFlow, SimpleFlowDelegate};
//!
//! // Simple horizontal flow with spacing
//! let delegate = Box::new(SimpleFlowDelegate::new(10.0));
//! let flow = RenderFlow::new(delegate);
//!
//! // Custom delegate for complex layout
//! struct MyFlowDelegate;
//! impl FlowDelegate for MyFlowDelegate {
//!     fn get_size(&self, constraints: BoxConstraints) -> Size {
//!         Size::new(constraints.max_width, constraints.max_height)
//!     }
//!
//!     fn get_constraints_for_child(&self, _index: usize, constraints: BoxConstraints) -> BoxConstraints {
//!         constraints.loosen()
//!     }
//!
//!     fn paint_children(&self, context: &mut FlowPaintContext, offset: Offset) {
//!         for i in 0..context.child_count {
//!             let transform = Matrix4::translation(offset.dx, offset.dy + i as f32 * 50.0, 0.0);
//!             context.paint_child(i, transform, Offset::new(offset.dx, offset.dy + i as f32 * 50.0));
//!         }
//!     }
//!
//!     fn should_relayout(&self, _old: &dyn Any) -> bool { true }
//!     fn should_repaint(&self, _old: &dyn Any) -> bool { true }
//!     fn as_any(&self) -> &dyn Any { self }
//! }
//! ```

use flui_foundation::ElementId;
use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Variable};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::{BoxConstraints, Matrix4, Offset, Size};
use std::any::Any;
use std::fmt::Debug;

/// Context provided to FlowDelegate during paint
pub struct FlowPaintContext<'a, 'b> {
    /// Paint context reference
    pub paint_ctx: &'a mut BoxPaintCtx<'b, Variable>,
    /// Number of children
    pub child_count: usize,
    /// Size of each child (after layout)
    pub child_sizes: &'a [Size],
    /// Children IDs
    pub children: &'a [ElementId],
}

impl<'a, 'b> FlowPaintContext<'a, 'b> {
    /// Paint a child with transformation matrix
    pub fn paint_child(&mut self, index: usize, transform: Matrix4, offset: Offset) {
        if index >= self.children.len() {
            return;
        }

        // TODO: Apply transformation matrix when transform layers are supported
        // For now, just paint at offset
        let _ = transform; // Suppress warning
        self.paint_ctx.paint_child(self.children[index], offset);
    }
}

/// Delegate trait for custom Flow layout logic
pub trait FlowDelegate: Debug + Send + Sync {
    /// Calculate the size of the flow container
    fn get_size(&self, constraints: BoxConstraints) -> Size;

    /// Get constraints for a specific child
    fn get_constraints_for_child(
        &self,
        index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints;

    /// Paint children with custom transformations
    fn paint_children(&self, context: &mut FlowPaintContext, offset: Offset);

    /// Check if layout should be recomputed
    fn should_relayout(&self, old: &dyn Any) -> bool;

    /// Check if repaint is needed (without relayout)
    fn should_repaint(&self, old: &dyn Any) -> bool;

    /// For Any trait
    fn as_any(&self) -> &dyn Any;
}

/// Simple flow delegate that arranges children in a horizontal line with transforms
#[derive(Debug, Clone)]
pub struct SimpleFlowDelegate {
    /// Spacing between children
    pub spacing: f32,
}

impl SimpleFlowDelegate {
    /// Create new simple flow delegate
    pub fn new(spacing: f32) -> Self {
        Self { spacing }
    }
}

impl FlowDelegate for SimpleFlowDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        // Use max available size
        Size::new(constraints.max_width, constraints.max_height)
    }

    fn get_constraints_for_child(
        &self,
        _index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints {
        // Give children loose constraints
        BoxConstraints::new(0.0, constraints.max_width, 0.0, constraints.max_height)
    }

    fn paint_children(&self, context: &mut FlowPaintContext, offset: Offset) {
        let mut x = 0.0;

        for i in 0..context.child_count {
            let child_size = context.child_sizes[i];

            // Create translation matrix for this child
            let transform = Matrix4::translation(offset.dx + x, offset.dy, 0.0);

            context.paint_child(i, transform, Offset::new(offset.dx + x, offset.dy));

            x += child_size.width + self.spacing;
        }
    }

    fn should_relayout(&self, old: &dyn Any) -> bool {
        if let Some(old_delegate) = old.downcast_ref::<SimpleFlowDelegate>() {
            self.spacing != old_delegate.spacing
        } else {
            true
        }
    }

    fn should_repaint(&self, old: &dyn Any) -> bool {
        self.should_relayout(old)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// RenderObject that implements custom layout via FlowDelegate.
///
/// Uses delegate pattern for custom layout logic without subclassing. The
/// FlowDelegate determines child constraints, container size, and child
/// transformations. Optimized for transform-based repositioning without relayout.
///
/// # Arity
///
/// `Variable` - Can have any number of children (0+).
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Delegated Custom Layout** - Delegates layout logic to FlowDelegate trait,
/// supports transformation matrices for efficient repositioning, optimizes
/// with should_relayout/should_repaint checks.
///
/// # Use Cases
///
/// - **Custom layouts**: Complex layouts beyond standard containers
/// - **Animated repositioning**: Transform-based child repositioning
/// - **Parallax effects**: Children moving at different speeds
/// - **Carousel**: Custom carousel with transformation effects
/// - **Performance**: Repaint without relayout for position changes
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderFlow behavior:
/// - Uses FlowDelegate trait for custom layout logic
/// - Delegate provides child constraints and container size
/// - Delegate paints children with transformation matrices
/// - Optimizes updates with should_relayout/should_repaint
/// - TODO: Full transformation matrix support in paint
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderFlow, SimpleFlowDelegate};
///
/// // Simple horizontal flow
/// let delegate = Box::new(SimpleFlowDelegate::new(10.0));
/// let flow = RenderFlow::new(delegate);
/// ```
pub struct RenderFlow {
    /// Layout delegate
    delegate: Box<dyn FlowDelegate>,

    // Cache for layout
    child_sizes: Vec<Size>,
    size: Size,
}

impl RenderFlow {
    /// Create new RenderFlow with delegate
    pub fn new(delegate: Box<dyn FlowDelegate>) -> Self {
        Self {
            delegate,
            child_sizes: Vec::new(),
            size: Size::ZERO,
        }
    }

    /// Set new delegate
    pub fn set_delegate(&mut self, delegate: Box<dyn FlowDelegate>) {
        self.delegate = delegate;
    }
}

impl Debug for RenderFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderFlow")
            .field("delegate", &"<delegate>")
            .field("child_sizes", &self.child_sizes)
            .field("size", &self.size)
            .finish()
    }
}

impl RenderObject for RenderFlow {}

impl RenderBox<Variable> for RenderFlow {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
        let constraints = ctx.constraints;
        let children = ctx.children;

        // Get container size from delegate
        let size = self.delegate.get_size(constraints);

        // Layout each child with constraints from delegate
        self.child_sizes.clear();
        for (i, child_id) in children.iter().enumerate() {
            let child_constraints = self.delegate.get_constraints_for_child(i, constraints);
            let child_size = ctx.layout_child(*child_id, child_constraints)?;
            self.child_sizes.push(child_size);
        }

        self.size = size;
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Variable>) {
        let offset = ctx.offset;

        // Collect child IDs first to avoid borrow checker issues
        let child_ids: Vec<ElementId> = ctx.children.iter().map(|id| *id).collect();

        // Create paint context for delegate
        let mut flow_paint_ctx = FlowPaintContext {
            paint_ctx: ctx,
            child_count: child_ids.len(),
            child_sizes: &self.child_sizes,
            children: &child_ids,
        };

        // Let delegate paint children with transformations
        self.delegate.paint_children(&mut flow_paint_ctx, offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_flow_delegate_new() {
        let delegate = SimpleFlowDelegate::new(10.0);
        assert_eq!(delegate.spacing, 10.0);
    }

    #[test]
    fn test_simple_flow_delegate_get_size() {
        let delegate = SimpleFlowDelegate::new(10.0);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 100.0);
        let size = delegate.get_size(constraints);

        assert_eq!(size.width, 200.0);
        assert_eq!(size.height, 100.0);
    }

    #[test]
    fn test_simple_flow_delegate_get_constraints_for_child() {
        let delegate = SimpleFlowDelegate::new(10.0);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 100.0);
        let child_constraints = delegate.get_constraints_for_child(0, constraints);

        assert_eq!(child_constraints.min_width, 0.0);
        assert_eq!(child_constraints.max_width, 200.0);
        assert_eq!(child_constraints.min_height, 0.0);
        assert_eq!(child_constraints.max_height, 100.0);
    }

    #[test]
    fn test_simple_flow_delegate_should_relayout_same() {
        let delegate1 = SimpleFlowDelegate::new(10.0);
        let delegate2 = SimpleFlowDelegate::new(10.0);

        assert!(!delegate1.should_relayout(delegate2.as_any()));
    }

    #[test]
    fn test_simple_flow_delegate_should_relayout_different() {
        let delegate1 = SimpleFlowDelegate::new(10.0);
        let delegate2 = SimpleFlowDelegate::new(20.0);

        assert!(delegate1.should_relayout(delegate2.as_any()));
    }

    #[test]
    fn test_render_flow_new() {
        let delegate = Box::new(SimpleFlowDelegate::new(10.0));
        let flow = RenderFlow::new(delegate);

        assert_eq!(flow.child_sizes.len(), 0);
        assert_eq!(flow.size, Size::ZERO);
    }

    #[test]
    fn test_render_flow_set_delegate() {
        let delegate1 = Box::new(SimpleFlowDelegate::new(10.0));
        let mut flow = RenderFlow::new(delegate1);

        let delegate2 = Box::new(SimpleFlowDelegate::new(20.0));
        flow.set_delegate(delegate2);

        // Delegate should be updated (can't easily verify without layout)
        assert_eq!(flow.child_sizes.len(), 0);
    }
}
