//! Multi-child render box trait

use crate::traits::RenderBox;

/// Trait for render boxes with multiple children
///
/// MultiChildRenderBox is used for layout objects that manage multiple children:
/// - **RenderFlex** (Row, Column): Flex layout
/// - **RenderStack**: Layered positioning
/// - **RenderWrap**: Wrapping layout
/// - **RenderFlow**: Custom flow layouts
///
/// # Child Storage
///
/// Children are typically stored using `BoxChildren<PD>` where PD is the
/// parent data type specific to the layout algorithm.
///
/// # Ambassador Support
///
/// ```ignore
/// use ambassador::Delegate;
///
/// #[derive(Delegate)]
/// #[delegate(MultiChildRenderBox, target = "children")]
/// struct RenderFlex {
///     children: BoxChildren<FlexParentData>,
///     direction: Axis,
/// }
///
/// impl MultiChildRenderBox for RenderFlex {
///     fn children(&self) -> impl Iterator<Item = &dyn RenderBox> {
///         self.children.iter()
///     }
///
///     fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn RenderBox> {
///         self.children.iter_mut()
///     }
///
///     fn child_count(&self) -> usize {
///         self.children.len()
///     }
/// }
/// ```
///
/// # Layout Pattern
///
/// Typical layout flow:
/// 1. Iterate over children
/// 2. Layout each child with constraints
/// 3. Store child positions in parent data
/// 4. Compute final parent size
#[ambassador::delegatable_trait]
pub trait MultiChildRenderBox: RenderBox {
    /// Returns an iterator over immutable references to children
    fn children(&self) -> impl Iterator<Item = &dyn RenderBox>;

    /// Returns an iterator over mutable references to children
    fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn RenderBox>;

    /// Returns the number of children
    fn child_count(&self) -> usize;

    // Helper methods with default implementations

    /// Returns the first child, if any
    fn first_child(&self) -> Option<&dyn RenderBox> {
        self.children().next()
    }

    /// Returns the last child, if any
    fn last_child(&self) -> Option<&dyn RenderBox> {
        self.children().last()
    }

    /// Returns the child at the given index, if any
    fn child_at(&self, index: usize) -> Option<&dyn RenderBox> {
        self.children().nth(index)
    }

    /// Returns whether this render box has any children
    fn has_children(&self) -> bool {
        self.child_count() > 0
    }

    /// Returns whether this render box is empty (has no children)
    fn is_empty(&self) -> bool {
        self.child_count() == 0
    }
}
