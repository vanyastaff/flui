//! Multi-box adaptor trait for slivers with box children

use crate::traits::{RenderBox, RenderSliver};

/// Trait for slivers that manage multiple box children
///
/// RenderSliverMultiBoxAdaptor is used for scrollable lists and grids
/// where each item is a box render object:
/// - **RenderSliverList**: Scrollable list of boxes
/// - **RenderSliverGrid**: Scrollable grid of boxes
/// - **RenderSliverFixedExtentList**: List with fixed item heights
///
/// # Child Management
///
/// Unlike regular multi-child boxes, sliver adaptors typically support:
/// - Lazy rendering (only create visible children)
/// - Child recycling/pooling
/// - Infinite scrolling
///
/// # Ambassador Support
///
/// ```ignore
/// use ambassador::Delegate;
///
/// struct RenderSliverList {
///     children: Vec<Box<dyn RenderBox>>,
///     // ... other fields
/// }
///
/// impl RenderSliverMultiBoxAdaptor for RenderSliverList {
///     fn children(&self) -> impl Iterator<Item = &dyn RenderBox> {
///         self.children.iter().map(|b| &**b)
///     }
///
///     fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn RenderBox> {
///         self.children.iter_mut().map(|b| &mut **b)
///     }
///
///     fn child_count(&self) -> usize {
///         self.children.len()
///     }
/// }
/// ```
#[ambassador::delegatable_trait]
pub trait RenderSliverMultiBoxAdaptor: RenderSliver {
    /// Returns an iterator over immutable references to box children
    fn children(&self) -> impl Iterator<Item = &dyn RenderBox>;

    /// Returns an iterator over mutable references to box children
    fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn RenderBox>;

    /// Returns the number of children currently realized
    fn child_count(&self) -> usize;

    // Helper methods

    /// Returns the first child, if any
    fn first_child(&self) -> Option<&dyn RenderBox> {
        self.children().next()
    }

    /// Returns the last child, if any
    fn last_child(&self) -> Option<&dyn RenderBox> {
        self.children().last()
    }

    /// Returns whether this adaptor has any children
    fn has_children(&self) -> bool {
        self.child_count() > 0
    }
}
