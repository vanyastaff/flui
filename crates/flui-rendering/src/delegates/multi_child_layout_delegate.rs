//! Multi-child layout delegate for custom layout algorithms with multiple
//! children.
//!
//! [`MultiChildLayoutDelegate`] allows users to implement custom layout
//! behavior for render objects with multiple children identified by IDs.

use std::{any::Any, fmt::Debug};

use flui_foundation::geometry::{Offset, Size};

use crate::constraints::BoxConstraints;

/// A delegate that provides custom layout behavior for multiple children.
///
/// Unlike single-child layout, multi-child layout requires identifying
/// children by ID strings. This allows the delegate to lay out children
/// in a specific order and position them relative to each other.
///
/// # Layout Context
///
/// The delegate works with a [`MultiChildLayoutContext`] that provides
/// methods to layout and position children. The context is provided
/// during the `perform_layout` call.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::constraints::BoxConstraints;
/// use flui_rendering::delegates::{MultiChildLayoutContext, MultiChildLayoutDelegate};
/// use flui_foundation::geometry::{Offset, Size};
///
/// #[derive(Debug)]
/// struct DialogLayoutDelegate {
///     padding: f64,
/// }
///
/// impl MultiChildLayoutDelegate for DialogLayoutDelegate {
///     fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, size: Size) {
///         let inner_width = size.width - 2.0 * self.padding;
///         let mut y = self.padding;
///
///         // Layout title
///         if context.has_child("title") {
///             let title_constraints = BoxConstraints::tight_for(Some(inner_width), None);
///             let title_size = context.layout_child("title", title_constraints);
///             context.position_child("title", Offset::new(self.padding, y));
///             y += title_size.height + self.padding;
///         }
///
///         // Layout content
///         if context.has_child("content") {
///             let content_constraints = BoxConstraints::tight_for(Some(inner_width), None);
///             let content_size = context.layout_child("content", content_constraints);
///             context.position_child("content", Offset::new(self.padding, y));
///         }
///     }
///
///     fn get_size(&self, constraints: BoxConstraints) -> Size {
///         constraints.biggest()
///     }
///
///     fn should_relayout(&self, old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
///         if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
///             self.padding != old.padding
///         } else {
///             true
///         }
///     }
/// }
/// ```
pub trait MultiChildLayoutDelegate: Send + Sync + Debug {
    /// Perform layout of children.
    ///
    /// Use the context to query, layout, and position children by their IDs.
    /// Children must be laid out before they can be positioned.
    ///
    /// # Arguments
    ///
    /// * `context` - The layout context providing child operations
    /// * `size` - The size of this render object
    fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, size: Size);

    /// Get the size of the parent for the given constraints.
    ///
    /// # Arguments
    ///
    /// * `constraints` - The constraints from the parent
    ///
    /// # Returns
    ///
    /// The size of this render object.
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }

    /// Whether to relayout when the delegate changes.
    ///
    /// # Arguments
    ///
    /// * `old_delegate` - The previous layout delegate
    ///
    /// # Returns
    ///
    /// `true` if layout should be recalculated, `false` otherwise.
    fn should_relayout(&self, old_delegate: &dyn MultiChildLayoutDelegate) -> bool;

    /// Returns self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Context for multi-child layout operations.
///
/// This trait is implemented by the render object and passed to the delegate
/// during layout. It provides methods to query, layout, and position children.
pub trait MultiChildLayoutContext {
    /// Check if a child with the given ID exists.
    fn has_child(&self, child_id: &str) -> bool;

    /// Layout a child with the given constraints and return its size.
    ///
    /// # Panics
    ///
    /// Panics if the child doesn't exist or has already been laid out.
    fn layout_child(&mut self, child_id: &str, constraints: BoxConstraints) -> Size;

    /// Position a child at the given offset.
    ///
    /// # Panics
    ///
    /// Panics if the child doesn't exist.
    fn position_child(&mut self, child_id: &str, offset: Offset);
}
