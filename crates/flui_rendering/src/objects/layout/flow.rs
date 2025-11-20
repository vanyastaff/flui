//! RenderFlow - Custom layout with delegate pattern

// TODO: Migrate to Render<A>
// use flui_core::render::{RuntimeArity, LayoutContext, PaintContext, LegacyRender};
use flui_painting::Canvas;
use flui_types::{BoxConstraints, Matrix4, Offset, Size};
use std::any::Any;
use std::fmt::Debug;

/// Context provided to FlowDelegate during paint
pub struct FlowPaintContext<'a> {
    /// Parent canvas
    pub canvas: &'a mut Canvas,
    /// Number of children
    pub child_count: usize,
    /// Size of each child (after layout)
    pub child_sizes: &'a [Size],
    /// Parent offset
    pub offset: Offset,
}

impl<'a> FlowPaintContext<'a> {
    /// Paint a child with transformation matrix
    pub fn paint_child(
        &mut self,
        index: usize,
        transform: Matrix4,
        tree: &flui_core::element::ElementTree,
        children: &[flui_core::element::ElementId],
        offset: Offset,
    ) {
        if index >= children.len() {
            return;
        }

        // Apply transformation and paint child
        self.canvas.save();
        self.canvas.set_transform(transform);
        let child_canvas = tree.paint_child(children[index], offset);
        self.canvas.append_canvas(child_canvas);
        self.canvas.restore();
    }
}

/// Delegate trait for custom Flow layout logic
pub trait FlowDelegate: Debug + Send + Sync {
    /// Calculate the size of the flow container
    fn get_size(&self, constraints: BoxConstraints) -> Size;

    /// Get constraints for a specific child
    fn get_constraints_for_child(&self, index: usize, constraints: BoxConstraints) -> BoxConstraints;

    /// Paint children with custom transformations
    fn paint_children(
        &self,
        context: &mut FlowPaintContext,
        tree: &flui_core::element::ElementTree,
        children: &[flui_core::element::ElementId],
        offset: Offset,
    );

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

    fn get_constraints_for_child(&self, _index: usize, constraints: BoxConstraints) -> BoxConstraints {
        // Give children loose constraints
        BoxConstraints::new(0.0, constraints.max_width, 0.0, constraints.max_height)
    }

    fn paint_children(
        &self,
        context: &mut FlowPaintContext,
        tree: &flui_core::element::ElementTree,
        children: &[flui_core::element::ElementId],
        offset: Offset,
    ) {
        let mut x = 0.0;

        for i in 0..context.child_count {
            let child_size = context.child_sizes[i];

            // Create translation matrix for this child
            let transform = Matrix4::translation(offset.dx + x, offset.dy, 0.0);

            context.paint_child(i, transform, tree, children, Offset::ZERO);

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

impl LegacyRender for RenderFlow {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let children = ctx.children.multi();
        let constraints = ctx.constraints;

        // Get container size from delegate
        let size = self.delegate.get_size(constraints);

        // Layout each child with constraints from delegate
        self.child_sizes.clear();
        for (i, &child_id) in children.iter().enumerate() {
            let child_constraints = self.delegate.get_constraints_for_child(i, constraints);
            let child_size = tree.layout_child(child_id, child_constraints);
            self.child_sizes.push(child_size);
        }

        self.size = size;
        size
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let tree = ctx.tree;
        let children = ctx.children.multi();
        let offset = ctx.offset;

        let mut canvas = Canvas::new();

        // Create paint context
        let mut paint_context = FlowPaintContext {
            canvas: &mut canvas,
            child_count: children.len(),
            child_sizes: &self.child_sizes,
            offset,
        };

        // Let delegate paint children with transformations
        self.delegate
            .paint_children(&mut paint_context, tree, children, offset);

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Variable
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

    #[test]
    fn test_arity_is_variable() {
        let delegate = Box::new(SimpleFlowDelegate::new(10.0));
        let flow = RenderFlow::new(delegate);

        assert_eq!(flow.arity(), RuntimeArity::Variable);
    }
}
