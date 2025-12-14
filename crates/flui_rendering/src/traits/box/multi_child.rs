//! MultiChildRenderBox trait.

use super::RenderBox;

/// Trait for render boxes with multiple children.
///
/// # Flutter Equivalence
///
/// This corresponds to `ContainerRenderObjectMixin<RenderBox, ...>` in Flutter.
pub trait MultiChildRenderBox: RenderBox {
    /// Returns an iterator over children.
    fn children(&self) -> Box<dyn Iterator<Item = &dyn RenderBox> + '_>;

    /// Returns a mutable iterator over children.
    fn children_mut(&mut self) -> Box<dyn Iterator<Item = &mut dyn RenderBox> + '_>;

    /// Returns the number of children.
    fn child_count(&self) -> usize;

    /// Adds a child.
    fn add_child(&mut self, child: Box<dyn RenderBox>);

    /// Removes a child at the given index.
    fn remove_child(&mut self, index: usize) -> Option<Box<dyn RenderBox>>;

    /// Removes all children.
    fn clear_children(&mut self);
}
