//! AnyElement - Object-safe base trait for heterogeneous element collections
//!
//! This module defines the `AnyElement` trait, which is object-safe and allows
//! elements to be stored in heterogeneous collections like `Vec<Box<dyn AnyElement>>`.
//!
//! # Why AnyElement?
//!
//! The `Element` trait has associated types, which makes it not object-safe.
//! This means you cannot create `Box<dyn Element>` or `Vec<Box<dyn Element>>`.
//!
//! `AnyElement` solves this by being object-safe - it doesn't have associated types.
//! Any type that implements `Element` automatically implements `AnyElement` via a blanket impl.
//!
//! # Usage
//!
//! ```rust,ignore
//! // For heterogeneous collections (element tree storage)
//! let elements: Vec<Box<dyn AnyElement>> = vec![
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

use crate::{AnyWidget, ElementId, ElementTree};
use super::ElementLifecycle;

/// Object-safe base trait for all elements
///
/// This trait is automatically implemented for all types that implement `Element`.
/// It's used when you need trait objects (`Box<dyn AnyElement>`) for heterogeneous
/// element collections (like the element tree).
///
/// # Design Pattern
///
/// Flui uses a two-trait pattern:
/// - **AnyElement** (this trait) - Object-safe, for `Box<dyn AnyElement>` collections
/// - **Element** - Has associated types, for zero-cost concrete usage
///
/// # When to Use
///
/// - Use `Box<dyn AnyElement>` when you need to store elements of different types
/// - Use `Element` trait bound when working with concrete element types
///
/// # Example
///
/// ```rust,ignore
/// struct ElementTree {
///     elements: HashMap<ElementId, Box<dyn AnyElement>>,  // Heterogeneous storage
/// }
/// ```
pub trait AnyElement: Downcast + fmt::Debug + Send + Sync {
    // ========== Identity & Hierarchy ==========

    /// Get the element's unique ID
    fn id(&self) -> ElementId;

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
    /// This is the object-safe version that takes `Box<dyn AnyWidget>`.
    /// For zero-cost updates with concrete types, use `Element::update()`.
    fn update_any(&mut self, new_widget: Box<dyn AnyWidget>);

    /// Rebuild this element's subtree
    ///
    /// Returns a list of (parent_id, child_widget, slot) tuples for children
    /// that need to be mounted.
    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn AnyWidget>, usize)>;

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

    /// Get RenderObject if this element has one
    fn render_object(&self) -> Option<&dyn crate::AnyRenderObject>;

    /// Get mutable RenderObject if this element has one
    fn render_object_mut(&mut self) -> Option<&mut dyn crate::AnyRenderObject>;

    // ========== Advanced Lifecycle ==========

    /// Propagate dependency changes to this element
    fn did_change_dependencies(&mut self);

    /// Update child slot position
    fn update_slot_for_child(&mut self, child_id: ElementId, new_slot: usize);

    /// Forget a child element (for GlobalKey reparenting)
    fn forget_child(&mut self, child_id: ElementId);
}

// Enable downcasting for AnyElement trait objects
impl_downcast!(AnyElement);
