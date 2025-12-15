//! MultiChildRenderBox trait.

use super::RenderBox;

/// Trait for render boxes with multiple children.
///
/// # Flutter Equivalence
///
/// This corresponds to `ContainerRenderObjectMixin<RenderBox, ...>` in Flutter.
///
/// # Child Management
///
/// Children are managed as a doubly-linked list conceptually, allowing
/// efficient insertion and removal at any position. The trait provides
/// methods for accessing children by position (first, last, before, after)
/// as well as iterators for traversal.
pub trait MultiChildRenderBox: RenderBox {
    // ========================================================================
    // Child Access
    // ========================================================================

    /// Returns an iterator over children.
    fn children(&self) -> Box<dyn Iterator<Item = &dyn RenderBox> + '_>;

    /// Returns a mutable iterator over children.
    fn children_mut(&mut self) -> Box<dyn Iterator<Item = &mut dyn RenderBox> + '_>;

    /// Returns the number of children.
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

    // ========================================================================
    // Child Modification
    // ========================================================================

    /// Appends a child to the end of the child list.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `add` in Flutter's `ContainerRenderObjectMixin`.
    fn add_child(&mut self, child: Box<dyn RenderBox>);

    /// Inserts a child at the specified index.
    ///
    /// If `index` is 0, the child is inserted at the beginning.
    /// If `index` is greater than or equal to `child_count()`, the child
    /// is appended to the end.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `insert` in Flutter's `ContainerRenderObjectMixin`,
    /// but uses an index rather than an "after" reference for Rust safety.
    fn insert_child(&mut self, index: usize, child: Box<dyn RenderBox>);

    /// Removes a child at the given index.
    ///
    /// Returns the removed child, or `None` if the index is out of bounds.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `remove` in Flutter's `ContainerRenderObjectMixin`.
    fn remove_child(&mut self, index: usize) -> Option<Box<dyn RenderBox>>;

    /// Removes all children.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `removeAll` in Flutter's `ContainerRenderObjectMixin`.
    fn clear_children(&mut self);

    /// Adds all children from the given iterator.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `addAll` in Flutter's `ContainerRenderObjectMixin`.
    fn add_all(&mut self, children: impl IntoIterator<Item = Box<dyn RenderBox>>) {
        for child in children {
            self.add_child(child);
        }
    }

    /// Moves a child from one index to another.
    ///
    /// This is more efficient than removing and re-adding the child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `move` in Flutter's `ContainerRenderObjectMixin`.
    fn move_child(&mut self, from_index: usize, to_index: usize);

    // ========================================================================
    // Sibling Navigation
    // ========================================================================

    /// Returns the index of the given child, if it exists in the child list.
    ///
    /// This performs a linear search through the children.
    fn index_of(&self, child: &dyn RenderBox) -> Option<usize> {
        self.children()
            .enumerate()
            .find(|(_, c)| std::ptr::eq(*c, child))
            .map(|(i, _)| i)
    }

    /// Returns whether the child list is empty.
    fn is_empty(&self) -> bool {
        self.child_count() == 0
    }
}
