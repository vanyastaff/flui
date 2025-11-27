//! RenderFlow - Custom layout with delegate pattern
//!
//! Implements the flow layout algorithm, optimized for efficiently repositioning
//! child widgets using transformation matrices during the paint phase.
//!
//! Flutter reference: <https://api.flutter.dev/flutter/rendering/RenderFlow-class.html>

use crate::core::{BoxProtocol, LayoutContext, PaintContext, RenderBox, Variable};
use flui_foundation::ElementId;
use flui_types::{BoxConstraints, Matrix4, Offset, Size};
use std::any::Any;
use std::fmt::Debug;

/// Context provided to FlowDelegate during paint
pub struct FlowPaintContext<'a, 'b, T>
where
    T: crate::core::PaintTree,
{
    /// Paint context reference
    pub paint_ctx: &'a mut PaintContext<'b, T, Variable>,
    /// Number of children
    pub child_count: usize,
    /// Size of each child (after layout)
    pub child_sizes: &'a [Size],
    /// Children IDs
    pub children: &'a [ElementId],
}

impl<'a, 'b, T> FlowPaintContext<'a, 'b, T>
where
    T: crate::core::PaintTree,
{
    /// Paint a child with transformation matrix
    ///
    /// The transformation matrix is applied to the child's coordinate system.
    /// This allows repositioning children efficiently without re-layout.
    ///
    /// # Arguments
    ///
    /// * `index` - Index of the child to paint
    /// * `transform` - 4x4 transformation matrix to apply
    /// * `offset` - Base offset for painting (typically the flow container's offset)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Translate child by (100, 50)
    /// let transform = Matrix4::translation(100.0, 50.0, 0.0);
    /// context.paint_child(0, transform, offset);
    ///
    /// // Rotate child 45 degrees around center
    /// let transform = Matrix4::rotation_z(std::f32::consts::PI / 4.0);
    /// context.paint_child(1, transform, offset);
    /// ```
    pub fn paint_child(&mut self, index: usize, transform: Matrix4, offset: Offset) {
        if index >= self.children.len() {
            return;
        }

        // Apply transformation using chaining API
        // The transform is applied BEFORE the offset translation
        self.paint_ctx.canvas().saved().transformed(transform);

        // Paint child - the offset is applied in the transformed coordinate system
        self.paint_ctx.paint_child(self.children[index], offset);

        self.paint_ctx.canvas().restored();
    }

    /// Paint a child without transformation (just offset)
    ///
    /// This is equivalent to `paint_child(index, Matrix4::IDENTITY, offset)` but
    /// avoids unnecessary save/restore overhead.
    pub fn paint_child_simple(&mut self, index: usize, offset: Offset) {
        if index >= self.children.len() {
            return;
        }
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
    fn paint_children<T>(&self, context: &mut FlowPaintContext<'_, '_, T>, offset: Offset)
    where
        T: crate::core::PaintTree;

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

    fn paint_children<T>(&self, context: &mut FlowPaintContext<'_, '_, T>, offset: Offset)
    where
        T: crate::core::PaintTree,
    {
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

/// RenderObject that implements custom layout via FlowDelegate
///
/// Uses a delegate pattern to allow custom layout logic without subclassing.
/// Optimized for repositioning children using transformation matrices.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderFlow, SimpleFlowDelegate};
///
/// let delegate = Box::new(SimpleFlowDelegate::new(10.0));
/// let flow = RenderFlow::new(delegate);
/// ```
pub struct RenderFlow<D: FlowDelegate + 'static> {
    /// Layout delegate
    delegate: D,

    // Cache for layout
    child_sizes: Vec<Size>,
    size: Size,
}

impl<D: FlowDelegate> RenderFlow<D> {
    /// Create new RenderFlow with delegate
    pub fn new(delegate: D) -> Self {
        Self {
            delegate,
            child_sizes: Vec::new(),
            size: Size::ZERO,
        }
    }

    /// Set new delegate
    pub fn set_delegate(&mut self, delegate: D) {
        self.delegate = delegate;
    }
}

impl<D: FlowDelegate> Debug for RenderFlow<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderFlow")
            .field("delegate", &"<delegate>")
            .field("child_sizes", &self.child_sizes)
            .field("size", &self.size)
            .finish()
    }
}

impl<D: FlowDelegate> RenderBox<Variable> for RenderFlow<D> {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Variable, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = ctx.constraints;
        let children = ctx.children;

        // Get container size from delegate
        let size = self.delegate.get_size(constraints);

        // Layout each child with constraints from delegate
        self.child_sizes.clear();
        for (i, child_id) in children.iter().enumerate() {
            let child_constraints = self.delegate.get_constraints_for_child(i, constraints);
            let child_size = ctx.layout_child(child_id, child_constraints);
            self.child_sizes.push(child_size);
        }

        self.size = size;
        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: crate::core::PaintTree,
    {
        let offset = ctx.offset;

        // Collect child IDs first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children.iter().collect();

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
        let delegate = SimpleFlowDelegate::new(10.0);
        let flow = RenderFlow::new(delegate);

        assert_eq!(flow.child_sizes.len(), 0);
        assert_eq!(flow.size, Size::ZERO);
    }

    #[test]
    fn test_render_flow_set_delegate() {
        let delegate1 = SimpleFlowDelegate::new(10.0);
        let mut flow = RenderFlow::new(delegate1);

        let delegate2 = SimpleFlowDelegate::new(20.0);
        flow.set_delegate(delegate2);

        // Delegate should be updated (can't easily verify without layout)
        assert_eq!(flow.child_sizes.len(), 0);
    }
}
