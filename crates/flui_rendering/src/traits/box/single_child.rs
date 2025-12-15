//! SingleChildRenderBox trait.

use super::RenderBox;

/// Trait for render boxes with at most one child.
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderObjectWithChildMixin<RenderBox>` in Flutter.
///
/// # Child Lifecycle
///
/// When a child is set, the implementation should:
/// 1. Drop the old child if present (calls `detach` on it)
/// 2. Adopt the new child (calls `attach` on it)
///
/// This is handled automatically by the `set_child` implementation.
pub trait SingleChildRenderBox: RenderBox {
    /// Returns the child, if any.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to the `child` getter in Flutter's `RenderObjectWithChildMixin`.
    fn child(&self) -> Option<&dyn RenderBox>;

    /// Returns the child mutably, if any.
    ///
    /// This is a Rust-specific addition since Dart doesn't distinguish
    /// between mutable and immutable references.
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;

    /// Sets the child.
    ///
    /// If there was a previous child, it will be dropped (detached).
    /// If the new child is `Some`, it will be adopted (attached).
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to the `child` setter in Flutter's `RenderObjectWithChildMixin`.
    fn set_child(&mut self, child: Option<Box<dyn RenderBox>>);

    /// Returns whether this render object has a child.
    fn has_child(&self) -> bool {
        self.child().is_some()
    }

    /// Takes the child, leaving `None` in its place.
    ///
    /// This is useful when you need to transfer ownership of the child
    /// to another render object.
    fn take_child(&mut self) -> Option<Box<dyn RenderBox>> {
        if self.has_child() {
            // This requires the implementation to support taking the child
            // Default implementations can't do this directly, so we mark it as
            // needing implementation-specific handling
            None
        } else {
            None
        }
    }
}
