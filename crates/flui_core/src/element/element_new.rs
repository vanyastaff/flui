//! Element struct - Unified element type for all views
//!
//! This module provides the unified `Element` struct that replaces the old
//! `enum Element` with a single struct containing a `ViewObject`.
//!
//! # Architecture
//!
//! Following Flutter's Element design:
//! - Element is the instantiation of a View at a particular location in the tree
//! - Element manages lifecycle (mount, update, unmount)
//! - Element bridges View tree and Render tree
//!
//! # Performance
//!
//! Using `Box<dyn ViewObject>` provides:
//! - Single allocation per element
//! - Dynamic dispatch for view operations
//! - Consistent memory layout
//! - Easy extension for new view types

use std::any::Any;
use std::fmt;

use crate::element::{ElementId, ElementLifecycle};
use crate::foundation::{AtomicElementFlags, ElementFlags, Slot};
use crate::render::RenderObject;
use crate::view::{BuildContext, ViewMode, ViewObject};

/// Element - Unified element type for all views
///
/// This struct represents a View instantiated at a particular location
/// in the element tree. It manages the lifecycle of the view and bridges
/// between the View tree and Render tree.
///
/// # Design
///
/// Following Flutter's architecture:
/// - `view_object`: The polymorphic view implementation
/// - `children`: Child element IDs
/// - Base fields for lifecycle management
///
/// # Thread Safety
///
/// Element is `Send` because:
/// - `ViewObject` requires `Send`
/// - All internal fields are thread-safe
pub struct Element {
    // ========== Lifecycle Fields ==========
    /// Parent element ID (None for root)
    parent: Option<ElementId>,

    /// Slot position in parent's child list
    slot: Option<Slot>,

    /// Current lifecycle state
    lifecycle: ElementLifecycle,

    /// Atomic flags for lock-free dirty tracking
    flags: AtomicElementFlags,

    // ========== View Fields ==========
    /// The polymorphic view object
    view_object: Box<dyn ViewObject>,

    /// Child element IDs
    children: Vec<ElementId>,
}

// Explicitly implement Send since ViewObject is Send
unsafe impl Send for Element {}

impl fmt::Debug for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Element")
            .field("parent", &self.parent)
            .field("slot", &self.slot)
            .field("lifecycle", &self.lifecycle)
            .field("mode", &self.view_object.mode())
            .field("children", &self.children)
            .finish()
    }
}

impl Element {
    /// Creates a new Element with the given view object.
    pub fn new(view_object: Box<dyn ViewObject>) -> Self {
        Self {
            parent: None,
            slot: None,
            lifecycle: ElementLifecycle::Initial,
            flags: AtomicElementFlags::new(),
            view_object,
            children: Vec::new(),
        }
    }

    // ========== Lifecycle Methods ==========

    /// Mount element to tree.
    ///
    /// Called when element is first added to the element tree.
    /// Sets parent, slot, and transitions to Active lifecycle state.
    #[inline]
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>) {
        debug_assert!(
            matches!(self.lifecycle, ElementLifecycle::Initial),
            "Cannot mount element in {:?} state",
            self.lifecycle
        );

        self.parent = parent;
        self.slot = slot;
        self.lifecycle = ElementLifecycle::Active;
        self.flags.insert(ElementFlags::MOUNTED);
        self.flags.insert(ElementFlags::ACTIVE);
    }

    /// Unmount element from tree.
    ///
    /// Called when element is being permanently removed from the tree.
    /// Transitions to Defunct lifecycle state and cleans up resources.
    #[inline]
    pub fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
        self.flags.remove(ElementFlags::MOUNTED);
        self.flags.remove(ElementFlags::ACTIVE);
    }

    /// Activate element.
    ///
    /// Called when element is reactivated after being deactivated.
    #[inline]
    pub fn activate(&mut self) {
        debug_assert!(
            matches!(self.lifecycle, ElementLifecycle::Inactive),
            "Cannot activate element in {:?} state",
            self.lifecycle
        );

        self.lifecycle = ElementLifecycle::Active;
        self.flags.insert(ElementFlags::ACTIVE);
    }

    /// Deactivate element.
    ///
    /// Called when element is temporarily deactivated (e.g., moved to cache).
    #[inline]
    pub fn deactivate(&mut self) {
        debug_assert!(
            matches!(self.lifecycle, ElementLifecycle::Active),
            "Cannot deactivate element in {:?} state",
            self.lifecycle
        );

        self.lifecycle = ElementLifecycle::Inactive;
        self.flags.remove(ElementFlags::ACTIVE);
    }

    /// Get current lifecycle state.
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.lifecycle
    }

    // ========== Parent/Slot Accessors ==========

    /// Get parent element ID.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Set parent element ID.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<ElementId>) {
        self.parent = parent;
    }

    /// Get slot position.
    #[inline]
    #[must_use]
    pub fn slot(&self) -> Option<Slot> {
        self.slot
    }

    /// Set slot position.
    #[inline]
    pub fn set_slot(&mut self, slot: Option<Slot>) {
        self.slot = slot;
    }

    // ========== Dirty Tracking ==========

    /// Check if element needs rebuild.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.flags.contains(ElementFlags::DIRTY)
    }

    /// Mark element as needing rebuild.
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.flags.insert(ElementFlags::DIRTY);
    }

    /// Clear dirty flag.
    #[inline]
    pub fn clear_dirty(&mut self) {
        self.flags.remove(ElementFlags::DIRTY);
    }

    /// Check if element needs layout.
    #[inline]
    #[must_use]
    pub fn needs_layout(&self) -> bool {
        self.flags.contains(ElementFlags::NEEDS_LAYOUT)
    }

    /// Mark element as needing layout.
    #[inline]
    pub fn mark_needs_layout(&mut self) {
        self.flags.insert(ElementFlags::NEEDS_LAYOUT);
    }

    /// Clear needs layout flag.
    #[inline]
    pub fn clear_needs_layout(&mut self) {
        self.flags.remove(ElementFlags::NEEDS_LAYOUT);
    }

    /// Check if element needs paint.
    #[inline]
    #[must_use]
    pub fn needs_paint(&self) -> bool {
        self.flags.contains(ElementFlags::NEEDS_PAINT)
    }

    /// Mark element as needing paint.
    #[inline]
    pub fn mark_needs_paint(&mut self) {
        self.flags.insert(ElementFlags::NEEDS_PAINT);
    }

    /// Clear needs paint flag.
    #[inline]
    pub fn clear_needs_paint(&mut self) {
        self.flags.remove(ElementFlags::NEEDS_PAINT);
    }

    // ========== View Object Access ==========

    /// Get view mode (Stateless, Stateful, etc.).
    #[inline]
    #[must_use]
    pub fn mode(&self) -> ViewMode {
        self.view_object.mode()
    }

    /// Build this element.
    ///
    /// Calls the view object's build method to produce child elements.
    /// Returns the old Element type until migration is complete.
    #[inline]
    pub fn build(&mut self, ctx: &BuildContext) -> crate::element::element::Element {
        self.view_object.build(ctx)
    }

    /// Initialize after mounting.
    #[inline]
    pub fn init(&mut self, ctx: &BuildContext) {
        self.view_object.init(ctx);
    }

    /// Called when dependencies change.
    #[inline]
    pub fn did_change_dependencies(&mut self, ctx: &BuildContext) {
        self.view_object.did_change_dependencies(ctx);
    }

    /// Update with new view configuration.
    #[inline]
    pub fn did_update(&mut self, new_view: &dyn Any, ctx: &BuildContext) {
        self.view_object.did_update(new_view, ctx);
    }

    /// Called when element is deactivated.
    #[inline]
    pub fn on_deactivate(&mut self, ctx: &BuildContext) {
        self.view_object.deactivate(ctx);
    }

    /// Called when element is permanently removed.
    #[inline]
    pub fn dispose(&mut self, ctx: &BuildContext) {
        self.view_object.dispose(ctx);
    }

    /// Get render object if this is a render view.
    #[inline]
    #[must_use]
    pub fn render_object(&self) -> Option<&dyn RenderObject> {
        self.view_object.render_object()
    }

    /// Get mutable render object if this is a render view.
    #[inline]
    #[must_use]
    pub fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        self.view_object.render_object_mut()
    }

    /// Access view object for downcasting.
    #[inline]
    #[must_use]
    pub fn view_object(&self) -> &dyn ViewObject {
        &*self.view_object
    }

    /// Mutable access to view object.
    #[inline]
    #[must_use]
    pub fn view_object_mut(&mut self) -> &mut dyn ViewObject {
        &mut *self.view_object
    }

    // ========== Child Management ==========

    /// Get child element IDs.
    #[inline]
    #[must_use]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Get mutable child element IDs.
    #[inline]
    #[must_use]
    pub fn children_mut(&mut self) -> &mut Vec<ElementId> {
        &mut self.children
    }

    /// Add a child element.
    #[inline]
    pub fn add_child(&mut self, child_id: ElementId) {
        self.children.push(child_id);
    }

    /// Remove a child element.
    #[inline]
    pub fn remove_child(&mut self, child_id: ElementId) {
        self.children.retain(|&id| id != child_id);
    }

    /// Clear all children.
    #[inline]
    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    /// Set children from iterator.
    #[inline]
    pub fn set_children(&mut self, children: impl IntoIterator<Item = ElementId>) {
        self.children = children.into_iter().collect();
    }

    /// Check if element has children.
    #[inline]
    #[must_use]
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Get first child (for single-child elements).
    #[inline]
    #[must_use]
    pub fn first_child(&self) -> Option<ElementId> {
        self.children.first().copied()
    }

    /// Forget a child (called when child is unmounted).
    #[inline]
    pub fn forget_child(&mut self, child_id: ElementId) {
        self.remove_child(child_id);
    }

    // ========== Predicates ==========

    /// Check if this is a render element (has render object).
    #[inline]
    #[must_use]
    pub fn is_render(&self) -> bool {
        self.view_object.render_object().is_some()
    }

    /// Check if this is a component element (no render object).
    #[inline]
    #[must_use]
    pub fn is_component(&self) -> bool {
        matches!(
            self.view_object.mode(),
            ViewMode::Stateless | ViewMode::Stateful
        )
    }

    /// Check if this is a provider element.
    #[inline]
    #[must_use]
    pub fn is_provider(&self) -> bool {
        matches!(self.view_object.mode(), ViewMode::Provider)
    }

    /// Get element category name for debugging.
    #[inline]
    #[must_use]
    pub fn category(&self) -> &'static str {
        match self.view_object.mode() {
            ViewMode::Stateless | ViewMode::Stateful => "Component",
            ViewMode::Animated => "Animated",
            ViewMode::Provider => "Provider",
            ViewMode::Proxy => "Proxy",
            ViewMode::RenderBox | ViewMode::RenderSliver => "Render",
        }
    }

    // ========== Event Handling ==========

    /// Handle an event.
    ///
    /// Default implementation returns false (event not handled).
    /// Override in specific view types for custom behavior.
    #[inline]
    pub fn handle_event(&mut self, _event: &flui_types::Event) -> bool {
        // TODO: Delegate to view_object when ViewObject has handle_event
        false
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl From<Box<dyn ViewObject>> for Element {
    fn from(view_object: Box<dyn ViewObject>) -> Self {
        Element::new(view_object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: Add tests after ViewObject wrappers are complete
}
