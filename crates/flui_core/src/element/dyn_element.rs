//! DynElement - Object-safe base trait for heterogeneous element collections
//!
//! This module defines the `DynElement` trait, which is object-safe and allows
//! elements to be stored in heterogeneous collections like `Vec<Box<dyn DynElement>>`.
//!
//! # Why DynElement?
//!
//! The `Element` trait has associated types, which makes it not object-safe.
//! This means you cannot create `Box<dyn Element>` or `Vec<Box<dyn Element>>`.
//!
//! `DynElement` solves this by being object-safe - it doesn't have associated types.
//! Any type that implements `Element` automatically implements `DynElement` via a blanket impl.
//!
//! # Usage
//!
//! ```rust,ignore
//! // For heterogeneous collections (element tree storage)
//! let elements: Vec<Box<dyn DynElement>> = vec![
//!     Box::new(ComponentElement::new(widget1)),
//!     Box::new(StatefulElement::new(widget2)),
//! ];
//!
//! // For concrete types with zero-cost
//! let element = ComponentElement::new(widget);
//! element.mount(parent, slot); // Uses Element trait, no boxing!
//! ```

use std::any::TypeId;
use std::fmt;
use std::sync::Arc;

use downcast_rs::{impl_downcast, Downcast};
use crate::foundation::Key;
use parking_lot::RwLock;

use crate::{DynWidget, ElementId, ElementTree};
use super::ElementLifecycle;

/// Object-safe base trait for all elements
///
/// This trait is automatically implemented for all types that implement `Element`.
/// It's used when you need trait objects (`Box<dyn DynElement>`) for heterogeneous
/// element collections (like the element tree).
///
/// # Design Pattern
///
/// Flui uses a two-trait pattern:
/// - **DynElement** (this trait) - Object-safe, for `Box<dyn DynElement>` collections
/// - **Element** - Has associated types, for zero-cost concrete usage
///
/// # Naming Convention
///
/// The `Dyn` prefix indicates this is the object-safe (dynamic dispatch) version,
/// avoiding confusion with `std::any::Any`.
///
/// # When to Use
///
/// - Use `Box<dyn DynElement>` when you need to store elements of different types
/// - Use `Element` trait bound when working with concrete element types
///
/// # Example
///
/// ```rust,ignore
/// struct ElementTree {
///     elements: HashMap<ElementId, Box<dyn DynElement>>,  // Heterogeneous storage
/// }
/// ```
pub trait DynElement: Downcast + fmt::Debug + Send + Sync {
    // ========== Identity & Hierarchy ==========
    //
    // Note: Element ID is NOT stored in the element itself.
    // It's the Slab index in ElementTree. When you need an element's ID,
    // get it from the context where you're working with the element.

    /// Get the parent element ID
    fn parent(&self) -> Option<ElementId>;

    /// Get the key if present
    fn key(&self) -> Option<&dyn Key>;

    // ========== Core Lifecycle Methods ==========

    /// Mount this element into the tree
    fn mount(&mut self, parent: Option<ElementId>, slot: usize);

    /// Unmount and clean up this element
    fn unmount(&mut self);

    /// Update this element with a new widget configuration (type-erased)
    ///
    /// This is the object-safe version that takes `Box<dyn DynWidget>`.
    /// For zero-cost updates with concrete types, use `Element::update()`.
    fn update_any(&mut self, new_widget: Box<dyn DynWidget>);

    /// Rebuild this element's subtree
    ///
    /// Returns a list of (parent_id, child_widget, slot) tuples for children
    /// that need to be mounted.
    ///
    /// # Parameters
    /// * `element_id` - The ID of this element (Slab index)
    fn rebuild(&mut self, element_id: ElementId) -> Vec<(ElementId, Box<dyn DynWidget>, usize)>;

    // ========== Dirty State Management ==========

    /// Check if this element is dirty (needs rebuild)
    fn is_dirty(&self) -> bool;

    /// Mark this element as dirty
    fn mark_dirty(&mut self);

    // ========== Lifecycle State ==========

    /// Get current lifecycle state
    fn lifecycle(&self) -> ElementLifecycle;

    /// Deactivate this element
    fn deactivate(&mut self);

    /// Activate this element
    fn activate(&mut self);

    // ========== Child Traversal ==========

    /// Iterate over child element IDs
    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_>;

    // ========== ComponentElement Support ==========

    /// Set tree reference for ComponentElements
    fn set_tree_ref(&mut self, tree: Arc<RwLock<ElementTree>>);

    /// Take old child ID before rebuild (for ComponentElement)
    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId>;

    /// Set child ID after mounting (for ComponentElement)
    fn set_child_after_mount(&mut self, child_id: ElementId);

    // ========== RenderObject Support ==========

    /// Get widget type ID for update checks
    fn widget_type_id(&self) -> TypeId;

    /// Get a reference to the widget configuration
    ///
    /// All elements hold a widget, so this always returns a value.
    /// The returned reference is valid as long as the element exists.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let widget_ref = element.widget();
    /// println!("Widget type: {}", widget_ref.type_name());
    /// ```
    fn widget(&self) -> &dyn crate::DynWidget;

    /// Get a reference to the state object
    ///
    /// Returns `Some` only for `StatefulElement`, `None` for all other element types.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(state) = element.state() {
    ///     // Access state methods
    /// }
    /// ```
    fn state(&self) -> Option<&dyn crate::State> {
        None  // Default: no state (only StatefulElement overrides this)
    }

    /// Get a mutable reference to the state object
    ///
    /// Returns `Some` only for `StatefulElement`, `None` for all other element types.
    ///
    /// # Warning
    ///
    /// Mutating state directly bypasses `setState()` and won't trigger rebuilds.
    /// Use `Context` and `setState()` instead for state updates that need to trigger rebuilds.
    fn state_mut(&mut self) -> Option<&mut dyn crate::State> {
        None  // Default: no state (only StatefulElement overrides this)
    }

    /// Get RenderObject if this element has one
    fn render_object(&self) -> Option<&dyn crate::DynRenderObject>;

    /// Get mutable RenderObject if this element has one
    fn render_object_mut(&mut self) -> Option<&mut dyn crate::DynRenderObject>;

    /// Take ownership of the render object
    ///
    /// Called by ElementTree when transferring ownership to parent RenderObject via adopt_child().
    /// This removes the RenderObject from this element and returns it.
    ///
    /// # Returns
    ///
    /// The owned render object, or None if this element has no render object
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // ElementTree transferring child RenderObject to parent
    /// if let Some(child_ro) = child_element.take_render_object() {
    ///     parent_ro.adopt_child(child_ro);  // Transfer ownership
    /// }
    /// ```
    fn take_render_object(&mut self) -> Option<Box<dyn crate::DynRenderObject>> {
        None // Default: no render object
    }

    /// Set render object (takes ownership)
    ///
    /// Used to restore a render object that was previously taken.
    /// Only RenderObjectElements implement this.
    ///
    /// # Arguments
    ///
    /// - `render_object`: The render object to set (takes ownership)
    fn set_render_object(&mut self, _render_object: Option<Box<dyn crate::DynRenderObject>>) {
        // Default: no-op (only RenderObjectElements implement this)
    }

    // ========== Advanced Lifecycle ==========

    /// Propagate dependency changes to this element
    fn did_change_dependencies(&mut self);

    /// Update child slot position
    fn update_slot_for_child(&mut self, child_id: ElementId, new_slot: usize);

    /// Forget a child element (for GlobalKey reparenting)
    fn forget_child(&mut self, child_id: ElementId);

    /// Reassemble this element (hot reload support)
    ///
    /// Called during hot reload to update element state with new code.
    /// For StatefulElement, this calls `state.reassemble()` and marks dirty.
    /// For other elements, this is a no-op (default implementation).
    ///
    /// # Hot Reload
    ///
    /// When code changes during development, reassemble() is called on all elements
    /// to give them a chance to update. StatefulElements use this to clear caches
    /// and update their state.
    fn reassemble(&mut self) {
        // Default: no-op (only StatefulElement overrides this)
    }

    // ========== InheritedWidget Dependency Tracking ==========

    /// Register a dependency on this element (for InheritedElement)
    ///
    /// This is called when an element calls `depend_on_inherited_widget_of_exact_type<T>()`.
    /// Only InheritedElement implements this; other elements do nothing.
    fn register_dependency(
        &mut self,
        _dependent_id: ElementId,
        _aspect: Option<Box<dyn std::any::Any + Send + Sync>>,
    ) {
        // Default: no-op (only InheritedElement implements this)
    }

    /// Get widget as specific type (for Context methods)
    ///
    /// Returns Some(widget) if this element's widget matches type T, None otherwise.
    /// Used by `depend_on_inherited_widget_of_exact_type<T>()`.
    fn widget_as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Check if widget is specific type (for Context methods)
    ///
    /// Returns true if this element's widget has the given TypeId.
    /// Used by `find_ancestor_inherited_element_of_type<T>()`.
    fn widget_has_type_id(&self, _type_id: TypeId) -> bool {
        false
    }

    // ========== Notification System ==========

    /// Visit notification during bubbling
    ///
    /// Called when a notification bubbles up through this element.
    /// Returns true to stop bubbling, false to continue.
    ///
    /// Default implementation continues bubbling (returns false).
    /// Override in elements that want to handle notifications (e.g., NotificationListenerElement).
    fn visit_notification(&self, _notification: &dyn crate::notification::AnyNotification) -> bool {
        false
    }
}

// Enable downcasting for DynElement trait objects
impl_downcast!(DynElement);

/// Boxed element trait object
///
/// Commonly used for heterogeneous collections of elements.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::BoxedElement;
///
/// let elements: Vec<BoxedElement> = vec![
///     Box::new(ComponentElement::new(widget1)),
///     Box::new(StatefulElement::new(widget2)),
/// ];
/// ```
pub type BoxedElement = Box<dyn DynElement>;
