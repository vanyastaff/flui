//! SingleChildRenderBox trait.

use super::RenderBox;

/// Trait for render boxes with at most one child.
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderObjectWithChildMixin<RenderBox>` in Flutter.
pub trait SingleChildRenderBox: RenderBox {
    /// Returns the child, if any.
    fn child(&self) -> Option<&dyn RenderBox>;

    /// Returns the child mutably, if any.
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;

    /// Sets the child.
    fn set_child(&mut self, child: Option<Box<dyn RenderBox>>);
}
