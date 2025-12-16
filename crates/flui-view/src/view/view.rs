//! Base View trait - immutable UI configuration.
//!
//! Views are the declarative description of UI. They are:
//! - **Immutable**: Created fresh each build cycle
//! - **Short-lived**: Exist only for diffing, then dropped
//! - **Composable**: Build trees of nested Views
//!
//! This is equivalent to Flutter's `Widget` class.

use std::any::TypeId;

/// Base trait for all Views.
///
/// A View is an immutable configuration for a piece of UI. Views are created
/// during the build phase and compared against previous Views to determine
/// what needs to change. Unlike Elements, Views are short-lived and recreated
/// each build cycle.
///
/// # Type Parameter
///
/// Each View type has an associated `Element` type that manages its lifecycle.
/// This association is determined at compile time, avoiding runtime type checks.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{View, StatelessView, BuildContext, IntoView};
///
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessView for Greeting {
///     fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
///         Text::new(format!("Hello, {}!", self.name))
///     }
/// }
/// ```
///
/// # Flutter Equivalent
///
/// This trait corresponds to Flutter's `Widget` abstract class:
/// - `create_element()` → `Widget.createElement()`
/// - `can_update()` → `Widget.canUpdate()` static method
pub trait View: Send + Sync + 'static {
    /// Create a new Element for this View.
    ///
    /// Called once when this View first appears in the tree.
    /// The Element manages the View's lifecycle and holds any mutable state.
    ///
    /// # Returns
    ///
    /// A boxed Element that will manage this View's lifecycle.
    fn create_element(&self) -> Box<dyn ElementBase>;

    /// Get this View as an Any reference for downcasting.
    ///
    /// This enables safe runtime downcasting of trait objects.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Get the type ID of this View for runtime type checking.
    ///
    /// Used by the framework to determine if two Views are of the same type.
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    /// Check if this View can update an existing Element.
    ///
    /// Returns `true` if the Element created by `old` can be updated with `self`.
    /// By default, Views of the same concrete type can update each other.
    ///
    /// Override this to add additional constraints (e.g., key matching).
    ///
    /// # Arguments
    ///
    /// * `old` - The previous View that created the Element
    ///
    /// # Returns
    ///
    /// `true` if the Element can be updated, `false` if it must be replaced.
    fn can_update(&self, old: &dyn View) -> bool {
        self.view_type_id() == old.view_type_id()
    }

    /// Get the Key associated with this View, if any.
    ///
    /// Keys are used for:
    /// - Preserving state across reorderings
    /// - GlobalKey lookups
    /// - Efficient reconciliation
    fn key(&self) -> Option<&dyn ViewKey> {
        None
    }
}

/// Trait for View keys used in reconciliation.
///
/// Keys help the framework match old and new Views during reconciliation.
pub trait ViewKey: Send + Sync + std::fmt::Debug {
    /// Get the type ID of this key for comparison.
    fn key_type_id(&self) -> TypeId;

    /// Check if this key equals another key.
    fn key_eq(&self, other: &dyn ViewKey) -> bool;

    /// Get the hash of this key for HashMap lookups.
    fn key_hash(&self) -> u64;
}

/// Base trait for Elements that can be boxed.
///
/// This is the object-safe version of Element for dynamic dispatch.
/// Specific Element types (StatelessElement, StatefulElement, etc.)
/// implement the full Element trait.
pub trait ElementBase: Send + Sync + 'static {
    /// Get the TypeId of the View that created this Element.
    fn view_type_id(&self) -> TypeId;

    /// Get the current lifecycle state.
    fn lifecycle(&self) -> crate::element::Lifecycle;

    /// Update this Element with a new View of the same type.
    ///
    /// # Safety
    ///
    /// Caller must ensure `new_view` is the same concrete type as the View
    /// that created this Element.
    fn update(&mut self, new_view: &dyn View);

    /// Mark this Element as needing a rebuild.
    fn mark_needs_build(&mut self);

    /// Perform the build phase.
    fn perform_build(&mut self);

    /// Mount this Element into the tree.
    fn mount(&mut self, parent: Option<flui_foundation::ElementId>, slot: usize);

    /// Deactivate this Element (temporarily removed from tree).
    fn deactivate(&mut self);

    /// Activate this Element (re-inserted into tree).
    fn activate(&mut self);

    /// Unmount this Element (permanently removed).
    fn unmount(&mut self);

    /// Visit all child Elements.
    fn visit_children(&self, visitor: &mut dyn FnMut(flui_foundation::ElementId));

    /// Get the depth in the element tree.
    fn depth(&self) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Basic compile-time checks
    fn _assert_view_is_object_safe(_: &dyn View) {}
    fn _assert_element_base_is_object_safe(_: &dyn ElementBase) {}
}
