//! RenderSliverMultiBoxAdaptor trait - sliver with multiple box children.

use super::RenderSliver;
use crate::traits::r#box::RenderBox;

/// Trait for slivers with multiple box children (lists, grids).
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderSliverMultiBoxAdaptor` in Flutter.
///
/// # Examples
///
/// - `SliverList`: scrollable list of box widgets
/// - `SliverGrid`: scrollable grid of box widgets
/// - `SliverFixedExtentList`: list with fixed item extent
///
/// # Virtualization
///
/// Unlike `MultiChildRenderBox`, this trait is designed for virtualized
/// scrolling where children may be created/destroyed lazily as they
/// scroll into/out of view.
pub trait RenderSliverMultiBoxAdaptor: RenderSliver {
    // ========================================================================
    // Child Access
    // ========================================================================

    /// Returns an iterator over box children.
    fn children(&self) -> Box<dyn Iterator<Item = &dyn RenderBox> + '_>;

    /// Returns a mutable iterator over box children.
    fn children_mut(&mut self) -> Box<dyn Iterator<Item = &mut dyn RenderBox> + '_>;

    /// Returns the number of children currently materialized.
    ///
    /// Note: This may not be the total number of items in the list,
    /// only the number of children currently in the render tree.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `childCount` in Flutter's `ContainerRenderObjectMixin`.
    fn child_count(&self) -> usize;

    /// Returns the first child in the child list, if any.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `firstChild` in Flutter's `ContainerRenderObjectMixin`.
    fn first_child(&self) -> Option<&dyn RenderBox> {
        self.children().next()
    }

    /// Returns the last child in the child list, if any.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `lastChild` in Flutter's `ContainerRenderObjectMixin`.
    fn last_child(&self) -> Option<&dyn RenderBox> {
        self.children().last()
    }

    /// Returns the child at the given index, if it exists.
    fn child_at(&self, index: usize) -> Option<&dyn RenderBox> {
        self.children().nth(index)
    }

    /// Returns the child at the given index mutably, if it exists.
    fn child_at_mut(&mut self, index: usize) -> Option<&mut dyn RenderBox> {
        self.children_mut().nth(index)
    }

    /// Returns whether the child list is empty.
    fn is_empty(&self) -> bool {
        self.child_count() == 0
    }

    // ========================================================================
    // Child Modification
    // ========================================================================

    /// Adds a child to the end of the child list.
    fn add_child(&mut self, child: Box<dyn RenderBox>);

    /// Inserts a child at the specified index.
    fn insert_child(&mut self, index: usize, child: Box<dyn RenderBox>);

    /// Removes a child at the given index.
    fn remove_child(&mut self, index: usize) -> Option<Box<dyn RenderBox>>;

    /// Removes all children.
    fn clear_children(&mut self);
}
