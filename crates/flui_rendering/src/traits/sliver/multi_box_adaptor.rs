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
pub trait RenderSliverMultiBoxAdaptor: RenderSliver {
    /// Returns an iterator over box children.
    fn children(&self) -> Box<dyn Iterator<Item = &dyn RenderBox> + '_>;

    /// Returns a mutable iterator over box children.
    fn children_mut(&mut self) -> Box<dyn Iterator<Item = &mut dyn RenderBox> + '_>;

    /// Returns the number of children.
    fn child_count(&self) -> usize;
}
