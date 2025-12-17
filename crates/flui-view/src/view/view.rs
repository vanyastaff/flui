//! Base View trait - immutable UI configuration.
//!
//! Views are the declarative description of UI. They are:
//! - **Immutable**: Created fresh each build cycle
//! - **Short-lived**: Exist only for diffing, then dropped
//! - **Composable**: Build trees of nested Views
//!
//! This is equivalent to Flutter's `Widget` class.

use std::any::TypeId;

use downcast_rs::{impl_downcast, Downcast};
use dyn_clone::{clone_trait_object, DynClone};

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
pub trait View: Downcast + DynClone + Send + Sync + 'static {
    /// Create a new Element for this View.
    ///
    /// Called once when this View first appears in the tree.
    /// The Element manages the View's lifecycle and holds any mutable state.
    ///
    /// # Returns
    ///
    /// A boxed Element that will manage this View's lifecycle.
    fn create_element(&self) -> Box<dyn ElementBase>;

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

impl_downcast!(View);
clone_trait_object!(View);

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
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `Element` abstract class. Key methods:
/// - `mount()` / `unmount()` - lifecycle
/// - `update()` - update with new widget
/// - `rebuild()` / `performRebuild()` - rebuild children
/// - `activate()` / `deactivate()` - temporary removal
/// - `didChangeDependencies()` - inherited widget changed
pub trait ElementBase: Downcast + Send + Sync + 'static {
    // ========================================================================
    // Identity
    // ========================================================================

    /// Get the TypeId of the View that created this Element.
    fn view_type_id(&self) -> TypeId;

    /// Get the depth in the element tree (root = 0).
    fn depth(&self) -> usize;

    /// Get the slot position in parent's child list.
    fn slot(&self) -> usize {
        0
    }

    // ========================================================================
    // Lifecycle State
    // ========================================================================

    /// Get the current lifecycle state.
    fn lifecycle(&self) -> crate::element::Lifecycle;

    /// Check if this Element is currently mounted.
    fn mounted(&self) -> bool {
        matches!(
            self.lifecycle(),
            crate::element::Lifecycle::Active | crate::element::Lifecycle::Inactive
        )
    }

    // ========================================================================
    // Lifecycle Methods
    // ========================================================================

    /// Mount this Element into the tree.
    ///
    /// Called when the Element is first inserted. Sets up parent relationship
    /// and initializes state.
    fn mount(&mut self, parent: Option<flui_foundation::ElementId>, slot: usize);

    /// Unmount this Element (permanently removed).
    ///
    /// Called when the Element is removed from the tree permanently.
    /// Resources should be released.
    fn unmount(&mut self);

    /// Activate this Element (re-inserted into tree).
    ///
    /// Called when a previously deactivated Element is reinserted.
    fn activate(&mut self);

    /// Deactivate this Element (temporarily removed from tree).
    ///
    /// Called when the Element is removed but may be reinserted.
    /// State is preserved.
    fn deactivate(&mut self);

    // ========================================================================
    // Update & Rebuild
    // ========================================================================

    /// Update this Element with a new View of the same type.
    ///
    /// Called when the parent rebuilds and provides a new View configuration.
    /// The Element should update its internal state to match the new View.
    fn update(&mut self, new_view: &dyn View);

    /// Mark this Element as needing a rebuild.
    ///
    /// The Element will be rebuilt in the next build phase.
    fn mark_needs_build(&mut self);

    /// Rebuild this Element.
    ///
    /// Called by the framework when this Element is dirty.
    /// Calls `perform_build()` if needed.
    fn rebuild(&mut self, force: bool) {
        if force || self.lifecycle() == crate::element::Lifecycle::Active {
            self.perform_build();
        }
    }

    /// Perform the actual build phase.
    ///
    /// Subclasses override this to rebuild their children.
    fn perform_build(&mut self);

    // ========================================================================
    // Dependency Notifications
    // ========================================================================

    /// Called when a dependency (InheritedView) changes.
    ///
    /// Override this to respond to inherited data changes.
    /// Default implementation marks the element for rebuild.
    fn did_change_dependencies(&mut self) {
        self.mark_needs_build();
    }

    // ========================================================================
    // Slot Management
    // ========================================================================

    /// Update the slot position of this Element.
    ///
    /// Called when the Element's position in the parent's child list changes.
    fn update_slot(&mut self, _new_slot: usize) {
        // Default: no-op. Subclasses can override.
    }

    // ========================================================================
    // Child Management
    // ========================================================================

    /// Visit all child Elements.
    fn visit_children(&self, visitor: &mut dyn FnMut(flui_foundation::ElementId));

    /// Get the first child Element, if any.
    fn first_child(&self) -> Option<flui_foundation::ElementId> {
        let mut first = None;
        self.visit_children(&mut |id| {
            if first.is_none() {
                first = Some(id);
            }
        });
        first
    }

    /// Deactivate a child Element.
    ///
    /// Removes the child from the tree but preserves its state.
    fn deactivate_child(&mut self, _child: flui_foundation::ElementId) {
        // Default: no-op. Subclasses should implement.
    }

    // ========================================================================
    // Debug
    // ========================================================================

    /// Get a debug description of this Element.
    fn debug_description(&self) -> String {
        format!(
            "Element(type={:?}, lifecycle={:?}, depth={})",
            self.view_type_id(),
            self.lifecycle(),
            self.depth()
        )
    }

    // ========================================================================
    // RenderObject Access
    // ========================================================================

    /// Get the RenderObject managed by this Element, if any.
    ///
    /// Only RenderObjectElement implementations return Some.
    /// ComponentElements (Stateless, Stateful) return None.
    ///
    /// This is used by parent RenderObjectElements to attach child
    /// RenderObjects to the render tree.
    fn render_object_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Get the RenderObject managed by this Element mutably, if any.
    fn render_object_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }

    /// Get the first child element, if any.
    ///
    /// Used for traversing the element tree to find descendant RenderObjects.
    fn child_element(&self) -> Option<&dyn ElementBase> {
        None
    }

    /// Get the first child element mutably, if any.
    fn child_element_mut(&mut self) -> Option<&mut dyn ElementBase> {
        None
    }

    /// Called by parent to attach this element's RenderObject to the render tree.
    ///
    /// For RenderObjectElements, this returns the RenderObject that should be
    /// inserted into the parent's render object.
    ///
    /// For ComponentElements (Stateless, Stateful), this delegates to the child.
    ///
    /// # Flutter Equivalent
    ///
    /// This corresponds to the pattern where `attachRenderObject` calls
    /// `ancestorRenderObjectElement.insertRenderObjectChild(renderObject, slot)`.
    fn attach_to_render_tree(&mut self) -> Option<&mut dyn std::any::Any> {
        // Default: no RenderObject to attach
        // ComponentElements override to delegate to child
        // RenderElements override to return their RenderObject
        None
    }

    /// Get the RenderObject as a shared Arc for render tree attachment.
    ///
    /// This enables the Flutter-like pattern where RenderObjects are owned
    /// by Elements but referenced by parent RenderObjects in the render tree.
    ///
    /// # Returns
    ///
    /// An Arc containing the RenderObject, or None if this element doesn't
    /// have a RenderObject or doesn't support shared ownership.
    fn render_object_shared(
        &self,
    ) -> Option<std::sync::Arc<parking_lot::RwLock<dyn std::any::Any + Send + Sync>>> {
        None
    }

    // ========================================================================
    // Pipeline Owner Propagation (for RenderTree integration)
    // ========================================================================

    /// Set the PipelineOwner for this element.
    ///
    /// Called by parent elements to propagate the PipelineOwner down the tree.
    /// RenderObjectElements use this to insert their RenderObjects into the RenderTree.
    ///
    /// Default implementation does nothing - only RenderObjectElements need this.
    ///
    /// # Arguments
    /// * `owner` - Arc<dyn Any> that should be downcast to the concrete PipelineOwner type
    fn set_pipeline_owner_any(&mut self, _owner: std::sync::Arc<dyn std::any::Any + Send + Sync>) {
        // Default: no-op
    }

    /// Set the parent's RenderId for tree structure.
    ///
    /// Called by parent elements to establish parent-child relationships in RenderTree.
    /// Child RenderObjects will be attached as children of this RenderId.
    fn set_parent_render_id(&mut self, _parent_id: Option<flui_foundation::RenderId>) {
        // Default: no-op
    }
}

impl_downcast!(ElementBase);

#[cfg(test)]
mod tests {
    use super::*;

    // Basic compile-time checks
    fn _assert_view_is_object_safe(_: &dyn View) {}
    fn _assert_element_base_is_object_safe(_: &dyn ElementBase) {}
}
