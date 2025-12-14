//! RenderSliverSingleBoxAdapter trait - sliver wrapping a single box child.

use super::RenderSliver;
use crate::traits::r#box::RenderBox;

/// Trait for slivers that contain a single box child.
///
/// Used to embed box widgets inside scrollable content.
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderSliverSingleBoxAdapter` in Flutter.
///
/// # Examples
///
/// - `SliverToBoxAdapter`: wraps a single box widget for scrolling
/// - `SliverFillRemaining`: fills remaining viewport space
pub trait RenderSliverSingleBoxAdapter: RenderSliver {
    /// Returns the box child, if any.
    fn child(&self) -> Option<&dyn RenderBox>;

    /// Returns the box child mutably, if any.
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;

    /// Sets the box child.
    fn set_child(&mut self, child: Option<Box<dyn RenderBox>>);
}
