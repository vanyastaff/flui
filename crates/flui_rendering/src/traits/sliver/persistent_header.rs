//! RenderSliverPersistentHeader trait - sliver with persistent header.

use super::RenderSliver;
use crate::traits::r#box::RenderBox;

/// Trait for slivers with a persistent header (pins, floats, or scrolls).
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderSliverPersistentHeader` in Flutter.
///
/// # Variants
///
/// - **Pinned**: stays at top when scrolling
/// - **Floating**: appears when scrolling up
/// - **Scrolling**: scrolls with content
pub trait RenderSliverPersistentHeader: RenderSliver {
    /// Returns the box child (header), if any.
    fn child(&self) -> Option<&dyn RenderBox>;

    /// Returns the box child mutably, if any.
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;

    /// The minimum extent (height when collapsed).
    fn min_extent(&self) -> f32;

    /// The maximum extent (height when expanded).
    fn max_extent(&self) -> f32;
}
