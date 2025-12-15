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
///
/// # Child Lifecycle
///
/// When a child is set, the implementation should properly handle
/// the attach/detach lifecycle of the child render object.
pub trait RenderSliverSingleBoxAdapter: RenderSliver {
    /// Returns the box child, if any.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to the `child` getter in Flutter's `RenderSliverSingleBoxAdapter`.
    fn child(&self) -> Option<&dyn RenderBox>;

    /// Returns the box child mutably, if any.
    ///
    /// This is a Rust-specific addition since Dart doesn't distinguish
    /// between mutable and immutable references.
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;

    /// Sets the box child.
    ///
    /// If there was a previous child, it will be dropped (detached).
    /// If the new child is `Some`, it will be adopted (attached).
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to the `child` setter in Flutter's `RenderSliverSingleBoxAdapter`.
    fn set_child(&mut self, child: Option<Box<dyn RenderBox>>);

    /// Returns whether this sliver has a child.
    fn has_child(&self) -> bool {
        self.child().is_some()
    }
}
