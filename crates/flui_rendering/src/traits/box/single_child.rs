//! Single child render box trait

use crate::traits::{BoxHitTestResult, PaintingContext, RenderBox};
use crate::constraints::BoxConstraints;
use crate::geometry::Size;
use flui_types::Offset;

/// Trait for render boxes with zero or one child
///
/// This trait provides common functionality for render objects that have
/// at most one child. It's used as a base for more specialized traits like
/// RenderProxyBox and RenderShiftedBox.
///
/// # Ambassador Support
///
/// This trait is designed to work with the `ambassador` crate for delegation:
///
/// ```ignore
/// use ambassador::Delegate;
///
/// #[derive(Delegate)]
/// #[delegate(SingleChildRenderBox, target = "container")]
/// struct MyRenderBox {
///     container: ProxyBox,
///     // ... other fields
/// }
/// ```
#[ambassador::delegatable_trait]
pub trait SingleChildRenderBox: RenderBox {
    /// Returns a reference to the child, if any
    fn child(&self) -> Option<&dyn RenderBox>;

    /// Returns a mutable reference to the child, if any
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;

    // Default implementations that can be overridden

    /// Default layout implementation that delegates to child
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        if let Some(child) = self.child_mut() {
            child.perform_layout(constraints)
        } else {
            constraints.smallest()
        }
    }

    /// Default hit test implementation
    fn hit_test_children(&self, result: &mut dyn BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            child.hit_test(result, position)
        } else {
            false
        }
    }

    /// Default paint implementation
    fn paint(&self, context: &mut dyn PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset);
        }
    }

    /// Compute minimum intrinsic width
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.child()
            .map(|c| c.compute_min_intrinsic_width(height))
            .unwrap_or(0.0)
    }

    /// Compute maximum intrinsic width
    fn compute_max_intrinsic_width(&self, height: f32) -> f32 {
        self.child()
            .map(|c| c.compute_max_intrinsic_width(height))
            .unwrap_or(0.0)
    }

    /// Compute minimum intrinsic height
    fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
        self.child()
            .map(|c| c.compute_min_intrinsic_height(width))
            .unwrap_or(0.0)
    }

    /// Compute maximum intrinsic height
    fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
        self.child()
            .map(|c| c.compute_max_intrinsic_height(width))
            .unwrap_or(0.0)
    }
}

// Implement painting context methods (placeholder implementations)
impl dyn PaintingContext {
    /// Paints a child render box at the given offset
    pub fn paint_child(&mut self, _child: &dyn RenderBox, _offset: Offset) {
        // This will be implemented when we create the actual PaintingContext
        // For now, this is a placeholder to allow trait compilation
    }
}
